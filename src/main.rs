use std::{
    env::var,
    fs::{read_to_string, write},
    sync::Arc,
    time::Duration,
};

use tokio::runtime::Runtime;
use veloren_client::{addr::ConnectionArgs, Client, Event};
use veloren_common::{
    clock::Clock,
    comp::{invite::InviteKind, ChatType, ControllerInputs},
    uuid::Uuid,
    ViewDistances,
};

const CRABO_UUID: &str = "3127d1eb-72ea-4342-a4fa-06dd2fb0bad9";

fn main() {
    let password_file = var("PASSWORD_FILE").expect("Provide PASSWORD_FILE environment variable.");
    let password = read_to_string(&password_file)
        .expect(&format!("Failed to read password from {password_file}"));

    let mut admin_list = vec![Uuid::parse_str(CRABO_UUID).unwrap()];

    write("admin_list.txt", format!("{admin_list:?}"))
        .expect("Failed to write initial admin list.");

    let mut client = connect_to_veloren(password);
    let mut clock = Clock::new(Duration::from_secs_f64(1.0));

    select_character(&mut client, &mut clock);

    loop {
        let events = client
            .tick(ControllerInputs::default(), clock.dt())
            .expect("Failed to run client.");

        for event in events {
            if let Event::Chat(message) = event {
                match message.chat_type {
                    ChatType::Tell(sender, _) | ChatType::Group(sender, _) => {
                        let sender_uuid = client.player_list().get(&sender).unwrap().uuid;

                        handle_message(
                            &mut client,
                            &mut admin_list,
                            message.into_content().as_plain().unwrap_or(""),
                            sender_uuid,
                        );
                    }
                    _ => {}
                }
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

fn select_character(client: &mut Client, clock: &mut Clock) {
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
}

fn handle_message(
    mut client: &mut Client,
    mut admin_list: &mut Vec<Uuid>,
    content: &str,
    sender: Uuid,
) {
    let mut words = content.split_whitespace();

    if let Some(command) = words.next() {
        match command {
            "admin" => {
                if admin_list.contains(&sender) {
                    adminify_players(&mut client, &mut admin_list, words)
                }
            }
            "inv" => invite_players(&mut client, words),
            "kick" => {
                if admin_list.contains(&sender) {
                    kick_players(&mut client, words)
                }
            }
            _ => {}
        }
    }
}

fn adminify_players<'a, T: Iterator<Item = &'a str>>(
    client: &mut Client,
    admin_list: &mut Vec<Uuid>,
    names: T,
) {
    for name in names {
        let find_id = client.player_list().iter().find_map(|(_, info)| {
            if info.player_alias == name {
                Some(info.uuid)
            } else {
                None
            }
        });

        if let Some(player_id) = find_id {
            admin_list.push(player_id);
        }
    }
}

fn invite_players<'a, T: Iterator<Item = &'a str>>(client: &mut Client, names: T) {
    for name in names {
        let find_id = client.player_list().iter().find_map(|(id, info)| {
            if info.player_alias == name {
                Some(id)
            } else {
                None
            }
        });

        if let Some(player_id) = find_id {
            client.send_invite(player_id.clone(), InviteKind::Group);
        }
    }
}

fn kick_players<'a, T: Iterator<Item = &'a str>>(client: &mut Client, names: T) {
    for name in names {
        let find_id = client.player_list().iter().find_map(|(id, info)| {
            if info.player_alias == name {
                Some(id)
            } else {
                None
            }
        });

        if let Some(player_id) = find_id {
            client.kick_from_group(player_id.clone());
        }
    }
}
