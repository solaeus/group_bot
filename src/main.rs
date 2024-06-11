use std::{env::var, fs::read_to_string, sync::Arc, time::Duration};

use tokio::runtime::Runtime;
use veloren_client::{addr::ConnectionArgs, Client, Event};
use veloren_common::{
    clock::Clock,
    comp::{invite::InviteKind, ChatType, ControllerInputs},
    ViewDistances,
};

const CRABO_UUID: &str = "3127d1eb-72ea-4342-a4fa-06dd2fb0bad9";

fn main() {
    let password_file = var("PASSWORD_FILE").expect("Provide PASSWORD_FILE environment variable.");
    let password = read_to_string(&password_file)
        .expect(&format!("Failed to read password from {password_file}"));

    let mut client = connect_to_veloren(password);
    let mut clock = Clock::new(Duration::from_secs_f64(1.0));

    client.load_character_list();

    while client.character_list().loading {
        println!("Loading characters...");

        client
            .tick(ControllerInputs::default(), clock.dt())
            .expect("Failed to run client.");
        clock.tick();
    }

    let character_id = client
        .character_list()
        .characters
        .first()
        .unwrap()
        .character
        .id
        .unwrap();

    client.request_character(
        character_id,
        ViewDistances {
            terrain: 0,
            entity: 0,
        },
    );

    loop {
        let events = client
            .tick(ControllerInputs::default(), clock.dt())
            .expect("Failed to run client.");

        for event in events {
            match event {
                Event::Chat(message) => {
                    if let ChatType::Tell(from, _) = message.chat_type {
                        if let Some(content) = message.content().as_plain() {
                            println!("{from}: {content}");

                            if content == "inv" {
                                client.send_invite(from, InviteKind::Group);
                            } else if content.starts_with("inv") {
                                let player_names =
                                    content.trim_start_matches("inv").split_whitespace();

                                for name in player_names {
                                    if let Some(player_id) =
                                        client.player_list().iter().find_map(|(id, info)| {
                                            if info.player_alias == name {
                                                Some(id)
                                            } else {
                                                None
                                            }
                                        })
                                    {
                                        client.send_invite(player_id.clone(), InviteKind::Group);
                                    }
                                }
                            }

                            if content.starts_with("kick") {
                                let player_names =
                                    content.trim_start_matches("kick").split_whitespace();
                                let sender_info = client.player_list().get(&from).unwrap();

                                if sender_info.uuid.to_string() == CRABO_UUID {
                                    for name in player_names {
                                        if let Some(player_id) =
                                            client.player_list().iter().find_map(|(id, info)| {
                                                if info.player_alias == name {
                                                    Some(id)
                                                } else {
                                                    None
                                                }
                                            })
                                        {
                                            client.kick_from_group(player_id.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        client.cleanup();
        clock.tick();
    }
}

fn connect_to_veloren(password: String) -> Client {
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
            "crabobot",
            &password,
            None,
            |provider| provider == "https://auth.veloren.net",
            &|_| {},
            |_| {},
            Default::default(),
        ))
        .expect("Failed to create client instance.")
}
