use std::{sync::Arc, time::Duration};

use tokio::runtime::Runtime;
use veloren_client::{addr::ConnectionArgs, Client, Event};
use veloren_common::{
    clock::Clock,
    comp::{invite::InviteKind, ChatMode, ChatType, ControllerInputs},
    uid::Uid,
    ViewDistances,
};

use crate::Config;

pub struct Bot {
    client: Client,
    clock: Clock,
    admin_list: Vec<String>,
    ban_list: Vec<String>,
}

impl Bot {
    pub fn new(
        username: &str,
        password: &str,
        admin_list: Vec<String>,
        ban_list: Vec<String>,
    ) -> Result<Self, String> {
        let client = connect_to_veloren(username, password)?;
        let clock = Clock::new(Duration::from_secs_f64(1.0));

        Ok(Bot {
            client,
            clock,
            admin_list,
            ban_list,
        })
    }

    pub fn select_character(&mut self) -> Result<(), String> {
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
            .unwrap();

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
        let events = self
            .client
            .tick(ControllerInputs::default(), self.clock.dt())
            .expect("Failed to run client.");

        for event in events {
            self.handle_event(event)?;
        }

        self.client.cleanup();
        self.clock.tick();

        Ok(())
    }

    fn handle_event(&mut self, event: Event) -> Result<(), String> {
        if let Event::Chat(message) = event {
            match message.chat_type {
                ChatType::Tell(sender, _) | ChatType::Group(sender, _) => {
                    let sender_uuid = self
                        .client
                        .player_list()
                        .get(&sender)
                        .unwrap()
                        .uuid
                        .to_string();

                    self.handle_message(
                        message.into_content().as_plain().unwrap_or(""),
                        &sender_uuid,
                    )?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_message(&mut self, content: &str, sender: &String) -> Result<(), String> {
        let mut names = content.split_whitespace();

        if let Some(command) = names.next() {
            match command {
                "admin" => {
                    if self.admin_list.contains(sender) {
                        self.adminify_players(names)?;
                    }
                }
                "ban" => {
                    if self.admin_list.contains(sender) {
                        self.kick_players(names.clone());
                        self.ban_players(names)?;
                    }
                }
                "cheese" => {
                    self.client.chat_mode = ChatMode::Group;
                    self.client.send_chat("I love cheese!".to_string());
                }
                "inv" => {
                    if !self.ban_list.contains(sender) {
                        self.invite_players(names)
                    }
                }
                "kick" => {
                    if self.admin_list.contains(sender) {
                        self.kick_players(names)
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn adminify_players<'a, T: Iterator<Item = &'a str>>(
        &mut self,
        names: T,
    ) -> Result<(), String> {
        for name in names {
            if let Some(player_id) = self.find_uuid(&name) {
                if !self.admin_list.contains(&player_id) {
                    self.admin_list.push(player_id);
                }
            }
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

    fn ban_players<'a, T: Iterator<Item = &'a str>>(&mut self, names: T) -> Result<(), String> {
        for name in names {
            if let Some(player_id) = self.find_uuid(&name) {
                if !self.ban_list.contains(&player_id) {
                    self.ban_list.push(player_id);
                }
            }
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

    fn invite_players<'a, T: Iterator<Item = &'a str>>(&mut self, names: T) {
        for name in names {
            if let Some(player_id) = self.find_uid(&name) {
                self.client
                    .send_invite(player_id.clone(), InviteKind::Group);
            }
        }
    }

    fn kick_players<'a, T: Iterator<Item = &'a str>>(&mut self, names: T) {
        for name in names {
            if let Some(player_id) = self.find_uid(&name) {
                self.client.kick_from_group(player_id.clone());
            }
        }
    }

    fn find_uid<'a>(&'a self, name: &str) -> Option<&'a Uid> {
        self.client.player_list().iter().find_map(|(id, info)| {
            if info.player_alias == name {
                Some(id)
            } else {
                None
            }
        })
    }

    fn find_uuid(&self, name: &str) -> Option<String> {
        self.client.player_list().iter().find_map(|(_, info)| {
            if info.player_alias == name {
                Some(info.uuid.to_string())
            } else {
                None
            }
        })
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
