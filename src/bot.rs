/// A bot that buys, sells and trades with players.
///
/// See [main.rs] for an example of how to run this bot.
use std::{sync::Arc, time::Duration};

use log::info;
use tokio::runtime::Runtime;
use veloren_client::{addr::ConnectionArgs, Client, ClientType, Event as VelorenEvent};
use veloren_common::{
    clock::Clock,
    comp::{invite::InviteKind, ChatType, ControllerInputs},
    uid::Uid,
    ViewDistances,
};

const CLIENT_TPS: Duration = Duration::from_millis(33);

/// An active connection to the Veloren server that will attempt to run every time the `tick`
/// function is called.
pub struct Bot {
    admins: Vec<String>,

    client: Client,
    clock: Clock,
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

        Ok(Bot {
            admins,
            client,
            clock,
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

        if !self.client.is_dead() {
            self.client.send_command("kill".to_string(), Vec::new());
        }

        self.clock.tick();

        Ok(true)
    }

    /// Consume and manage a client-side Veloren event. Returns a boolean indicating whether the
    /// bot should continue processing events.
    fn handle_veloren_event(&mut self, event: VelorenEvent) -> Result<(), String> {
        match event {
            VelorenEvent::Chat(message) => {
                if !matches!(
                    message.chat_type,
                    ChatType::Tell(_, _) | ChatType::Group(_, _)
                ) {
                    return Ok(());
                }

                let sender_uid = if let Some(uid) = message.uid() {
                    uid
                } else {
                    return Ok(());
                };
                let sender_name = self.find_player_alias(&sender_uid)?.clone();

                let message_parts: Vec<&str> = message
                    .content()
                    .as_plain()
                    .unwrap_or("")
                    .split_whitespace()
                    .collect();
                let command = message_parts.get(0).ok_or("Failed to get command")?;
                let args = &message_parts[1..];

                match *command {
                    "inv" => {
                        if !self.admins.contains(&sender_name) {
                            self.client.send_command(
                                "tell".to_string(),
                                vec![sender_name, "You are not an admin".to_string()],
                            );

                            return Ok(());
                        }

                        if args.is_empty() {
                            self.client.send_invite(sender_uid, InviteKind::Group);
                        } else {
                            for arg in args {
                                let target_uid =
                                    self.find_uid(arg).ok_or("Failed to find target uid")?;

                                self.client.send_invite(*target_uid, InviteKind::Group);
                                self.client.send_command(
                                    "tell".to_string(),
                                    vec![sender_name.clone(), format!("Invited {}", arg)],
                                );
                            }
                        }
                    }
                    "kick" => {
                        if !self.admins.contains(&sender_name) {
                            self.client.send_command(
                                "tell".to_string(),
                                vec![sender_name, "You are not an admin".to_string()],
                            );

                            return Ok(());
                        }

                        if args.is_empty() {
                            self.client.send_command(
                                "tell".to_string(),
                                vec![sender_name, "You must specify a player to kick".to_string()],
                            );

                            return Ok(());
                        }

                        for arg in args {
                            let target_uid =
                                self.find_uid(arg).ok_or("Failed to find target uid")?;

                            self.client.kick_from_group(*target_uid);
                        }
                    }
                    "admin" => {
                        if !self.admins.contains(&sender_name) {
                            self.client.send_command(
                                "tell".to_string(),
                                vec![sender_name, "You are not an admin".to_string()],
                            );

                            return Ok(());
                        }

                        if args.is_empty() {
                            self.client.send_command(
                                "tell".to_string(),
                                vec![
                                    sender_name,
                                    "You must specify a player to promote".to_string(),
                                ],
                            );

                            return Ok(());
                        }

                        for arg in args {
                            let target_uid =
                                self.find_uid(arg).ok_or("Failed to find target uid")?;
                            let target_name = self.find_player_alias(target_uid)?.clone();

                            self.admins.push(target_name.clone());
                            self.client.send_command(
                                "tell".to_string(),
                                vec![sender_name.clone(), format!("Promoted {}", target_name)],
                            );
                        }
                    }
                    _ => {}
                }
            }
            _ => (),
        }

        Ok(())
    }

    /// Finds the name of a player by their Uid.
    fn find_player_alias<'a>(&'a self, uid: &Uid) -> Result<&'a String, String> {
        self.client
            .player_list()
            .iter()
            .find_map(|(id, info)| {
                if id == uid {
                    return Some(&info.player_alias);
                }

                None
            })
            .ok_or("Failed to find player alias".to_string())
    }

    /// Finds the Uid of a player by their name.
    fn find_uid<'a>(&'a self, name: &str) -> Option<&'a Uid> {
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
            ClientType::Bot { privileged: false },
        ))
        .map_err(|error| format!("{error:?}"))
}
