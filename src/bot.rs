use std::{sync::Arc, time::Duration};

use log::info;
use tokio::runtime::Runtime;
use veloren_client::{addr::ConnectionArgs, Client, Event as VelorenEvent};
use veloren_common::{
    clock::Clock,
    comp::{invite::InviteKind, ChatType, ControllerInputs},
    uid::Uid,
    uuid::Uuid,
    ViewDistances,
};
use veloren_common_net::msg::PlayerInfo;

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
            match message.chat_type {
                ChatType::Tell(sender, _) | ChatType::Group(sender, _) => {
                    let sender_info = self.client.player_list().get(&sender).unwrap().clone();

                    self.handle_message(
                        message.into_content().as_plain().unwrap_or(""),
                        &sender_info,
                    )?;
                }
                ChatType::Offline(uid) => {
                    self.client.kick_from_group(uid);
                }
                ChatType::CommandError => {
                    eprintln!("Command Error!")
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_message(&mut self, content: &str, sender: &PlayerInfo) -> Result<(), String> {
        if content == "inv" {
            info!("Inviting {}", sender.player_alias);

            let uid = self.find_uid(&sender.player_alias)?;

            self.client.send_invite(uid, InviteKind::Group);
        }

        if content == "kick_all" && self.admin_list.contains(&sender.uuid) {
            info!("Kicking everyone");

            let group_members = self.client.group_members().clone();

            for (uid, _) in group_members {
                self.client.kick_from_group(uid);
            }
        }

        let mut words = content.split_whitespace();
        let command = if let Some(command) = words.next() {
            command
        } else {
            return Ok(());
        };

        match command {
            "admin" => {
                if self.admin_list.contains(&sender.uuid) || self.admin_list.is_empty() {
                    for word in words {
                        info!("Adminifying {word}");

                        self.adminify_player(word)?;
                    }
                }
            }
            "ban" => {
                if self.admin_list.contains(&sender.uuid) {
                    for word in words {
                        info!("Banning {word}");

                        let uid = self.find_uid(word)?;

                        self.client.kick_from_group(uid);
                        self.ban_player(word)?;
                    }
                }
            }
            "cheese" => {
                info!("Saying 'I love cheese!' to {}", sender.player_alias);

                let uid = self.find_uid(&sender.player_alias)?;

                if self.client.group_members().contains_key(&uid) {
                    self.client
                        .send_command("group".to_string(), vec!["I love cheese!".to_string()])
                }
            }
            "inv" => {
                if !self.ban_list.contains(&sender.uuid) {
                    for word in words {
                        info!("Inviting {word}");

                        let uid = self.find_uid(word)?;

                        self.client.send_invite(uid, InviteKind::Group);
                    }
                }
            }
            "kick" => {
                if self.admin_list.contains(&sender.uuid) {
                    for word in words {
                        info!("Kicking {word}");

                        let uid = self.find_uid(word)?;

                        self.client.kick_from_group(uid);
                    }
                }
            }
            "unban" => {
                if self.admin_list.contains(&sender.uuid) {
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
