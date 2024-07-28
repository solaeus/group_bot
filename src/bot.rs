/// A bot that buys, sells and trades with players.
///
/// See [main.rs] for an example of how to run this bot.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use log::{debug, error, info};
use tokio::runtime::Runtime;
use vek::{Quaternion, Vec3};
use veloren_client::{addr::ConnectionArgs, Client, Event as VelorenEvent, WorldExt};
use veloren_common::{
    clock::Clock,
    comp::{ChatType, ControllerInputs, Ori, Pos},
    outcome::Outcome,
    time::DayPeriod,
    uid::Uid,
    uuid::Uuid,
    ViewDistances,
};

const CLIENT_TPS: Duration = Duration::from_millis(33);

/// An active connection to the Veloren server that will attempt to run every time the `tick`
/// function is called.
pub struct Bot {
    username: String,
    position: Pos,
    orientation: Ori,
    admins: Vec<String>,

    client: Client,
    clock: Clock,

    last_action: Instant,
}

impl Bot {
    /// Connect to the official veloren server, select the specified character
    /// and return a Bot instance ready to run.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        game_server: String,
        auth_server: &str,
        username: String,
        password: &str,
        character: &str,
        admins: Vec<String>,
        position: Option<[f32; 3]>,
        orientation: Option<f32>,
    ) -> Result<Self, String> {
        info!("Connecting to veloren");

        let mut client = connect_to_veloren(game_server, auth_server, &username, password)?;
        let mut clock = Clock::new(CLIENT_TPS);

        client.load_character_list();

        while client.character_list().loading {
            client
                .tick(ControllerInputs::default(), clock.dt())
                .map_err(|error| format!("{error:?}"))?;
            clock.tick();
        }

        let character_id = client
            .character_list()
            .characters
            .iter()
            .find(|character_item| character_item.character.alias == character)
            .ok_or_else(|| format!("No character named {character}"))?
            .character
            .id
            .ok_or("Failed to get character ID")?;

        info!("Selecting a character");

        // This loop waits and retries requesting the character in the case that the character has
        // logged out too recently.
        while client.position().is_none() {
            client.request_character(
                character_id,
                ViewDistances {
                    terrain: 4,
                    entity: 4,
                },
            );

            client
                .tick(ControllerInputs::default(), clock.dt())
                .map_err(|error| format!("{error:?}"))?;
            clock.tick();
        }

        let position = if let Some(coords) = position {
            Pos(coords.into())
        } else {
            client.position().map(Pos).ok_or("Failed to get position")?
        };
        let orientation = if let Some(orientation) = orientation {
            Ori::new(Quaternion::rotation_z(orientation.to_radians()))
        } else {
            client.current::<Ori>().ok_or("Failed to get orientation")?
        };

        Ok(Bot {
            username,
            position,
            orientation,
            admins,
            client,
            clock,
            last_action: Instant::now(),
        })
    }

    /// Run the bot for a single tick. This should be called in a loop. Returns `true` if the loop
    /// should continue running.
    pub fn tick(&mut self) -> Result<bool, String> {
        let veloren_events = self
            .client
            .tick(ControllerInputs::default(), self.clock.dt())
            .map_err(|error| format!("{error:?}"))?;

        for event in veloren_events {
            self.handle_veloren_event(event)?;
        }

        if self.last_action.elapsed() > Duration::from_millis(300) {
            self.handle_lantern();
            self.handle_position_and_orientation()?;

            self.last_action = Instant::now();
        }

        self.clock.tick();

        Ok(true)
    }

    /// Consume and manage a client-side Veloren event. Returns a boolean indicating whether the
    /// bot should continue processing events.
    fn handle_veloren_event(&mut self, event: VelorenEvent) -> Result<(), String> {
        match event {
            VelorenEvent::Chat(message) => {
                let sender = if let ChatType::Tell(uid, _) = message.chat_type {
                    uid
                } else {
                    return Ok(());
                };
                let content = message.content().as_plain().unwrap_or_default();
                let mut split_content = content.split(' ');
                let command = split_content.next().unwrap_or_default();
                let price_correction_message = "Use the format 'price [search_term]'";
                let correction_message = match command {
                    "ori" => {
                        if self.is_user_admin(&sender)? {
                            if let Some(new_rotation) = split_content.next() {
                                let new_rotation = new_rotation
                                    .parse::<f32>()
                                    .map_err(|error| error.to_string())?;

                                self.orientation =
                                    Ori::new(Quaternion::rotation_z(new_rotation.to_radians()));

                                None
                            } else {
                                Some("Use the format 'ori [0-360]'")
                            }
                        } else {
                            Some(price_correction_message)
                        }
                    }
                    "pos" => {
                        if self.is_user_admin(&sender)? {
                            if let (Some(x), Some(y), Some(z)) = (
                                split_content.next(),
                                split_content.next(),
                                split_content.next(),
                            ) {
                                self.position = Pos(Vec3::new(
                                    x.parse::<f32>().map_err(|error| error.to_string())?,
                                    y.parse::<f32>().map_err(|error| error.to_string())?,
                                    z.parse::<f32>().map_err(|error| error.to_string())?,
                                ));

                                None
                            } else {
                                Some("Use the format 'pos [x] [y] [z]'.")
                            }
                        } else {
                            Some(price_correction_message)
                        }
                    }
                    _ => Some(price_correction_message),
                };

                if let Some(message) = correction_message {
                    let player_name = self
                        .find_player_alias(&sender)
                        .ok_or("Failed to find player name")?
                        .to_string();

                    self.client
                        .send_command("tell".to_string(), vec![player_name, message.to_string()]);
                }
            }
            VelorenEvent::Kicked(message) => {
                error!("Kicked from server: {message:?}");
            }
            _ => (),
        }

        Ok(())
    }

    /// Use the lantern at night and put it away during the day.
    fn handle_lantern(&mut self) {
        let day_period = self.client.state().get_day_period();

        match day_period {
            DayPeriod::Night => {
                if !self.client.is_lantern_enabled() {
                    self.client.enable_lantern();
                }
            }
            DayPeriod::Morning | DayPeriod::Noon | DayPeriod::Evening => {
                if self.client.is_lantern_enabled() {
                    self.client.disable_lantern();
                }
            }
        }
    }

    /// Determines if the Uid belongs to an admin.
    fn is_user_admin(&self, uid: &Uid) -> Result<bool, String> {
        let sender_name = self.find_player_alias(uid).ok_or("Failed to find name")?;

        if self.admins.contains(sender_name) {
            Ok(true)
        } else {
            let sender_uuid = self
                .find_uuid(uid)
                .ok_or("Failed to find uuid")?
                .to_string();

            Ok(self.admins.contains(&sender_uuid))
        }
    }

    /// Moves the character to the configured position and orientation.
    fn handle_position_and_orientation(&mut self) -> Result<(), String> {
        if let Some(current_position) = self.client.current::<Pos>() {
            if current_position != self.position {
                debug!(
                    "Updating position from {} to {}",
                    current_position.0, self.position.0
                );

                let entity = self.client.entity();
                let ecs = self.client.state_mut().ecs();
                let mut position_state = ecs.write_storage::<Pos>();

                position_state
                    .insert(entity, self.position)
                    .map_err(|error| error.to_string())?;
            }
        }

        if let Some(current_orientation) = self.client.current::<Ori>() {
            if current_orientation != self.orientation {
                debug!(
                    "Updating orientation from {:?} to {:?}",
                    current_orientation, self.orientation
                );

                let entity = self.client.entity();
                let ecs = self.client.state_mut().ecs();
                let mut orientation_state = ecs.write_storage::<Ori>();

                orientation_state
                    .insert(entity, self.orientation)
                    .map_err(|error| error.to_string())?;
            }
        }

        Ok(())
    }

    /// Finds the name of a player by their Uid.
    fn find_player_alias<'a>(&'a self, uid: &Uid) -> Option<&'a String> {
        self.client.player_list().iter().find_map(|(id, info)| {
            if id == uid {
                return Some(&info.player_alias);
            }

            None
        })
    }

    /// Finds the Uuid of a player by their Uid.
    fn find_uuid(&self, target: &Uid) -> Option<Uuid> {
        self.client.player_list().iter().find_map(|(uid, info)| {
            if uid == target {
                Some(info.uuid)
            } else {
                None
            }
        })
    }

    /// Finds the Uid of a player by their name.
    fn _find_uid<'a>(&'a self, name: &str) -> Option<&'a Uid> {
        self.client.player_list().iter().find_map(|(id, info)| {
            if info.player_alias == name {
                Some(id)
            } else {
                None
            }
        })
    }
}

fn connect_to_veloren(
    game_server: String,
    auth_server: &str,
    username: &str,
    password: &str,
) -> Result<Client, String> {
    let runtime = Arc::new(Runtime::new().unwrap());
    let runtime2 = Arc::clone(&runtime);

    runtime
        .block_on(Client::new(
            ConnectionArgs::Tcp {
                hostname: game_server,
                prefer_ipv6: false,
            },
            runtime2,
            &mut None,
            username,
            password,
            None,
            |provider| provider == auth_server,
            &|_| {},
            |_| {},
            Default::default(),
        ))
        .map_err(|error| format!("{error:?}"))
}
