use std::{sync::Arc, time::Duration};

use log::info;
use rand::{thread_rng, Rng};
use tokio::runtime::Runtime;
use veloren_client::{addr::ConnectionArgs, Client, Event as VelorenEvent};
use veloren_common::{
    clock::Clock,
    comp::{chat::GenericChatMsg, invite::InviteKind, ChatType, ControllerInputs},
    uid::Uid,
    uuid::Uuid,
    ViewDistances,
};

use crate::Config;

pub struct Bot {
    client: Client,
    clock: Clock,
    admin_list: Vec<Uuid>,
    ban_list: Vec<Uuid>,
}

impl Bot {
    pub fn new(
        username: &str,
        password: &str,
        admin_list: Vec<Uuid>,
        ban_list: Vec<Uuid>,
    ) -> Result<Self, String> {
        info!("Connecting to veloren");

        let client = connect_to_veloren(username, password)?;
        let clock = Clock::new(Duration::from_secs_f64(1.0 / 60.0));

        Ok(Bot {
            client,
            clock,
            admin_list,
            ban_list,
        })
    }

    pub fn select_character(&mut self) -> Result<(), String> {
        info!("Selecting a character");

        self.client.load_character_list();

        while self.client.character_list().loading {
            self.client
                .tick(ControllerInputs::default(), self.clock.dt())
                .map_err(|error| format!("{error:?}"))?;
            self.clock.tick();
        }

        let character_id = self
            .client
            .character_list()
            .characters
            .first()
            .expect("No characters to select")
            .character
            .id
            .expect("Failed to get character ID");

        self.client.request_character(
            character_id,
            ViewDistances {
                terrain: 0,
                entity: 0,
            },
        );

        Ok(())
    }

    pub fn tick(&mut self) -> Result<(), String> {
        let veloren_events = self
            .client
            .tick(ControllerInputs::default(), self.clock.dt())
            .map_err(|error| format!("{error:?}"))?;

        for veloren_event in veloren_events {
            self.handle_veloren_event(veloren_event)?;
        }

        self.client.cleanup();
        self.clock.tick();

        Ok(())
    }

    fn handle_veloren_event(&mut self, event: VelorenEvent) -> Result<(), String> {
        if let VelorenEvent::Chat(message) = event {
            self.handle_message(message)?;
        }

        Ok(())
    }

    fn handle_message(&mut self, message: GenericChatMsg<String>) -> Result<(), String> {
        let content = message.content().as_plain().unwrap_or("");
        let sender_uid = match &message.chat_type {
            ChatType::Tell(from, _) | ChatType::Group(from, _) | ChatType::Say(from) => from,
            _ => return Ok(()),
        };
        let sender_info = self
            .client
            .player_list()
            .into_iter()
            .find_map(|(uid, player_info)| {
                if uid == sender_uid {
                    Some(player_info.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| format!("Failed to find info for uid {sender_uid}"))?;

        // First, process commands with no arguments

        if content == "inv" {
            info!("Inviting {}", sender_info.player_alias);

            self.client.send_invite(*sender_uid, InviteKind::Group);
        }

        if content == "kick_all" && self.admin_list.contains(&sender_info.uuid) {
            info!("Kicking everyone");

            let group_members = self.client.group_members().clone();

            for (uid, _) in group_members {
                self.client.kick_from_group(uid);
            }
        }

        if content == "cheese" {
            info!("{} loves cheese!", sender_info.player_alias);

            let uid = self.find_uid(&sender_info.player_alias)?;

            if self.client.group_members().contains_key(&uid) {
                let content = format!("{} loves cheese!", sender_info.player_alias);

                match &message.chat_type {
                    ChatType::Tell(_, _) | ChatType::Say(_) => {
                        self.client.send_command("say".to_string(), vec![content])
                    }
                    _ => self.client.send_command("group".to_string(), vec![content]),
                }
            }
        }

        if content == "info" {
            info!("Printing info");

            let mut members_message = "Members:".to_string();

            for (uid, _) in self.client.group_members() {
                let alias = self
                    .client
                    .player_list()
                    .get(uid)
                    .unwrap()
                    .player_alias
                    .clone();

                members_message.extend_one(' ');
                members_message.extend(alias.chars().into_iter());
            }

            let mut admins_message = "Admins:".to_string();

            for uuid in &self.admin_list {
                for (_, info) in self.client.player_list() {
                    if &info.uuid == uuid {
                        admins_message.extend_one(' ');
                        admins_message.extend(info.player_alias.chars());

                        break;
                    }
                }
            }

            let mut banned_message = "Banned:".to_string();

            for uuid in &self.ban_list {
                for (_, info) in self.client.player_list() {
                    if &info.uuid == uuid {
                        banned_message.extend_one(' ');
                        banned_message.extend(info.player_alias.chars());

                        break;
                    }
                }
            }
            match &message.chat_type {
                ChatType::Tell(_, _) => {
                    self.client.send_command(
                        "tell".to_string(),
                        vec![sender_info.player_alias.clone(), members_message],
                    );
                    self.client.send_command(
                        "tell".to_string(),
                        vec![sender_info.player_alias.clone(), admins_message],
                    );
                    self.client.send_command(
                        "tell".to_string(),
                        vec![sender_info.player_alias, banned_message],
                    );
                }
                ChatType::Group(_, _) => {
                    self.client
                        .send_command("group".to_string(), vec![members_message]);
                    self.client
                        .send_command("group".to_string(), vec![admins_message]);
                    self.client
                        .send_command("group".to_string(), vec![banned_message]);
                }
                _ => {}
            }

            return Ok(());
        }

        // Process commands that use one or more arguments

        let mut words = content.split_whitespace();
        let command = if let Some(command) = words.next() {
            command
        } else {
            return Ok(());
        };

        match command {
            "admin" => {
                if self.admin_list.contains(&sender_info.uuid) || self.admin_list.is_empty() {
                    for word in words {
                        info!("Adminifying {word}");

                        self.adminify_player(word)?;
                    }
                }
            }
            "ban" => {
                if self.admin_list.contains(&sender_info.uuid) {
                    for word in words {
                        info!("Banning {word}");

                        let uid = self.find_uid(word)?;

                        self.client.kick_from_group(uid);
                        self.ban_player(word)?;
                    }
                }
            }
            "inv" => {
                if !self.ban_list.contains(&sender_info.uuid) {
                    for word in words {
                        info!("Inviting {word}");

                        let uid = self.find_uid(word)?;

                        self.client.send_invite(uid, InviteKind::Group);
                    }
                }
            }
            "kick" => {
                if self.admin_list.contains(&sender_info.uuid) {
                    for word in words {
                        info!("Kicking {word}");

                        let uid = self.find_uid(word)?;

                        self.client.kick_from_group(uid);
                    }
                }
            }
            "roll" => {
                for word in words {
                    let max = word
                        .parse::<u64>()
                        .map_err(|error| format!("Failed to parse integer: {error}"))?;
                    let random = thread_rng().gen_range(1..max);

                    match message.chat_type {
                        ChatType::Tell(_, _) => self.client.send_command(
                            "tell".to_string(),
                            vec![
                                sender_info.player_alias.clone(),
                                format!("Rolled a die with {} sides and got {random}.", max),
                            ],
                        ),
                        ChatType::Say(_) => self.client.send_command(
                            "say".to_string(),
                            vec![format!("Rolled a die with {} sides and got {random}.", max)],
                        ),
                        ChatType::Group(_, _) => self.client.send_command(
                            "group".to_string(),
                            vec![format!("Rolled a die with {} sides and got {random}.", max)],
                        ),
                        _ => return Ok(()),
                    }
                }
            }
            "unban" => {
                if self.admin_list.contains(&sender_info.uuid) {
                    for word in words {
                        info!("Unbanning {word}");

                        self.unban_player(word)?;
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn adminify_player(&mut self, name: &str) -> Result<(), String> {
        let uuid = self.find_uuid(name)?;

        if !self.admin_list.contains(&uuid) && !self.ban_list.contains(&uuid) {
            self.admin_list.push(uuid);
        }

        let old_config = Config::read()?;
        let new_config = Config {
            username: old_config.username,
            password: old_config.password,
            admin_list: self.admin_list.clone(),
            ban_list: old_config.ban_list,
        };

        new_config.write()?;

        Ok(())
    }

    fn ban_player(&mut self, name: &str) -> Result<(), String> {
        let uuid = self.find_uuid(name)?;

        if !self.admin_list.contains(&uuid) && !self.ban_list.contains(&uuid) {
            self.ban_list.push(uuid);
        }

        let old_config = Config::read()?;
        let new_config = Config {
            username: old_config.username,
            password: old_config.password,
            admin_list: old_config.admin_list,
            ban_list: self.ban_list.clone(),
        };

        new_config.write()?;

        Ok(())
    }

    fn unban_player(&mut self, name: &str) -> Result<(), String> {
        let uuid = self.find_uuid(name)?;

        if let Some(uuid) = self
            .ban_list
            .iter()
            .enumerate()
            .find_map(|(index, banned)| if &uuid == banned { Some(index) } else { None })
        {
            self.ban_list.remove(uuid);
        }

        let old_config = Config::read()?;
        let new_config = Config {
            username: old_config.username,
            password: old_config.password,
            admin_list: old_config.admin_list,
            ban_list: self.ban_list.clone(),
        };

        new_config.write()?;

        Ok(())
    }

    fn find_uid(&self, name: &str) -> Result<Uid, String> {
        self.client
            .player_list()
            .iter()
            .find_map(|(uid, info)| {
                if info.player_alias == name {
                    Some(uid.clone())
                } else {
                    None
                }
            })
            .ok_or(format!("Failed to find uid for player {}", name))
    }

    fn find_uuid(&self, name: &str) -> Result<Uuid, String> {
        self.client
            .player_list()
            .iter()
            .find_map(|(_, info)| {
                if info.player_alias == name {
                    Some(info.uuid)
                } else {
                    None
                }
            })
            .ok_or(format!("Failed to find uuid for player {}", name))
    }
}

fn connect_to_veloren(username: &str, password: &str) -> Result<Client, String> {
    let runtime = Arc::new(Runtime::new().unwrap());
    let runtime2 = Arc::clone(&runtime);

    runtime
        .block_on(Client::new(
            ConnectionArgs::Tcp {
                hostname: "server.veloren.net".to_string(),
                prefer_ipv6: false,
            },
            runtime2,
            &mut None,
            username,
            password,
            None,
            |provider| provider == "https://auth.veloren.net",
            &|_| {},
            |_| {},
            Default::default(),
        ))
        .map_err(|error| format!("{error:?}"))
}
