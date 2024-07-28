#![feature(duration_constructors)]

mod bot;
mod config;

use std::{env::var, fs::read_to_string};

use bot::Bot;
use config::{Config, Secrets};
use env_logger::Env;
use log::error;

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();

    let secrets = {
        let secrets_path =
            var("SECRETS").expect("Provide a SECRETS variable specifying the secrets file");
        let file_content = read_to_string(secrets_path).expect("Failed to read secrets");

        toml::from_str::<Secrets>(&file_content).expect("Failed to parse secrets")
    };
    let config = {
        if let Ok(config_path) = var("CONFIG") {
            let file_content = read_to_string(config_path).expect("Failed to read config");

            toml::from_str::<Config>(&file_content).expect("Failed to parse config")
        } else {
            Config::default()
        }
    };
    let game_server = config
        .game_server
        .unwrap_or_else(|| "server.veloren.net".to_string());
    let auth_server = config
        .auth_server
        .unwrap_or_else(|| "https://auth.veloren.net".to_string());
    let mut bot = Bot::new(
        game_server,
        &auth_server,
        secrets.username,
        &secrets.password,
        &secrets.character,
        secrets.admins,
        config.position,
        config.orientation,
    )
    .expect("Failed to create bot");

    loop {
        match bot.tick() {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                error!("{error}");
            }
        }
    }
}
