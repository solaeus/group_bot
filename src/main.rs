mod bot;

use std::{
    env::var,
    fs::{read_to_string, write},
};

use bot::Bot;
use serde::{Deserialize, Serialize};
use veloren_common::uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct Config {
    pub username: String,
    pub password: String,
    pub admin_list: Vec<Uuid>,
    pub ban_list: Vec<Uuid>,
}

impl Config {
    fn read() -> Result<Self, String> {
        let config_path = var("CONFIG_PATH").map_err(|error| error.to_string())?;
        let config_file_content = read_to_string(config_path).map_err(|error| error.to_string())?;

        toml::from_str::<Config>(&config_file_content).map_err(|error| error.to_string())
    }

    fn write(&self) -> Result<(), String> {
        let config_path = var("CONFIG_PATH").map_err(|error| error.to_string())?;
        let config_string = toml::to_string(self).map_err(|error| error.to_string())?;

        write(config_path, config_string).map_err(|error| error.to_string())
    }
}

fn main() {
    env_logger::init();

    let config = Config::read().unwrap();
    let mut bot = Bot::new(
        &config.username,
        &config.password,
        config.admin_list,
        config.ban_list,
    )
    .expect("Failed to create bot");

    bot.select_character().expect("Failed to select character");

    loop {
        bot.tick().expect("Failed to run bot")
    }
}
