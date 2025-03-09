/// A bot that buys, sells and trades with players.
///
/// See [main.rs] for an example of how to run this bot.
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use log::info;
use tokio::runtime::Runtime;
use veloren_client::{addr::ConnectionArgs, Client, ClientType, Event as VelorenEvent};
use veloren_common::{
    clock::Clock,
    comp::{
        invite::InviteKind,
        item::{ItemDesc, ItemI18n},
        ChatType, ControllerInputs,
    },
    uid::Uid,
    ViewDistances,
};

const CLIENT_TPS: Duration = Duration::from_millis(33);
const BOT_EVENT_INTERVAL: Duration = Duration::from_secs(1);

enum BotEvent {
    InvitePlayer(Uid),
    KickPlayer(Uid),
    SendTell(String, String),
}

/// An active connection to the Veloren server that will attempt to run every time the `tick`
/// function is called.
pub struct Bot {
    admins: Vec<String>,

    client: Client,
    clock: Clock,

    events: VecDeque<BotEvent>,
    last_bot_event: Instant,

    item_i18n: ItemI18n,
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
            events: VecDeque::new(),
            last_bot_event: Instant::now(),
            item_i18n: ItemI18n::new_expect(),
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

        if self.last_bot_event.elapsed() >= BOT_EVENT_INTERVAL {
            if let Some(next_bot_event) = self.events.pop_front() {
                self.handle_bot_event(next_bot_event)?;
            }

            self.last_bot_event = Instant::now();
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
                    return Err("Failed to get sender UID".to_string());
                };
                let sender_name = self.find_player_alias(&sender_uid)?.clone();
                let message_parts: Vec<&str> = message
                    .content()
                    .as_plain()
                    .unwrap_or("")
                    .split_whitespace()
                    .collect();
                let command = message_parts.first().ok_or("Failed to get command")?;
                let args = &message_parts[1..];

                match *command {
                    "inv" => {
                        if !self.admins.contains(&sender_name) {
                            self.events.push_back(BotEvent::SendTell(
                                sender_name.clone(),
                                "You are not an admin".to_string(),
                            ));

                            return Ok(());
                        }

                        if args.is_empty() {
                            self.events.push_back(BotEvent::InvitePlayer(sender_uid));
                        } else {
                            for arg in args {
                                if let Some(uid) = self.find_uid(arg) {
                                    self.events.push_back(BotEvent::InvitePlayer(*uid));
                                    self.events.push_back(BotEvent::SendTell(
                                        sender_name.clone(),
                                        format!("Invited {}", arg),
                                    ));
                                } else {
                                    self.events.push_back(BotEvent::SendTell(
                                        sender_name.clone(),
                                        format!("Failed to find player {}", arg),
                                    ));
                                }
                            }
                        }
                    }
                    "kick" => {
                        if !self.admins.contains(&sender_name) {
                            self.events.push_back(BotEvent::SendTell(
                                sender_name,
                                "You are not an admin".to_string(),
                            ));

                            return Ok(());
                        }

                        if args.is_empty() {
                            self.events.push_back(BotEvent::SendTell(
                                sender_name,
                                "You must specify a player to kick".to_string(),
                            ));

                            return Ok(());
                        }

                        for arg in args {
                            if let Some(uid) = self.find_uid(arg) {
                                self.events.push_back(BotEvent::KickPlayer(*uid));
                            } else {
                                self.events.push_back(BotEvent::SendTell(
                                    sender_name.clone(),
                                    format!("Failed to find player {}", arg),
                                ));
                            }
                        }
                    }
                    "admin" => {
                        if !self.admins.contains(&sender_name) {
                            self.events.push_back(BotEvent::SendTell(
                                sender_name,
                                "You are not an admin".to_string(),
                            ));

                            return Ok(());
                        }

                        if args.is_empty() {
                            self.events.push_back(BotEvent::SendTell(
                                sender_name,
                                "You must specify a player to promote".to_string(),
                            ));

                            return Ok(());
                        }

                        for arg in args {
                            if !self.client.players().any(|player| player == *arg) {
                                self.events.push_back(BotEvent::SendTell(
                                    sender_name.clone(),
                                    format!("Failed to find player {}", arg),
                                ));

                                continue;
                            }

                            self.admins.push(arg.to_string());
                            self.events.push_back(BotEvent::SendTell(
                                sender_name.clone(),
                                format!("Promoted {}", arg),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            VelorenEvent::GroupInventoryUpdate(item, uid) => {
                let (item_name, _) = item.i18n(&self.item_i18n);
                let recipient_name = self.find_player_alias(&uid)?;

                if item_name.as_plain().unwrap_or("") == "Dwarven Cheese" {
                    self.events.push_back(BotEvent::SendTell(
                        recipient_name.clone(),
                        format!("Congratulations on the cheese, {recipient_name}!"),
                    ));
                }
            }
            _ => (),
        }

        Ok(())
    }

    fn handle_bot_event(&mut self, event: BotEvent) -> Result<(), String> {
        match event {
            BotEvent::InvitePlayer(uid) => {
                self.client.send_invite(uid, InviteKind::Group);
            }
            BotEvent::KickPlayer(uid) => {
                self.client.kick_from_group(uid);
            }
            BotEvent::SendTell(name, message) => {
                self.client
                    .send_command("tell".to_string(), vec![name, message]);
            }
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
