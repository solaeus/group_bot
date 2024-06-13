mod bot;

use std::{
    env::var,
    fs::{read_to_string, write},
};

use bot::Bot;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Config {
    pub username: String,
    pub password: String,
    pub admin_list: Vec<String>,
}

impl Config {
    fn read() -> Self {
        let config_path = var("CONFIG_PATH").expect("Provide CONFIG_PATH environment variable");
        let config_file_content = read_to_string(config_path).expect("Failed to read config file");

        toml::from_str::<Config>(&config_file_content).expect("Failed to deserialize config file.")
    }

    fn write(&self) -> Result<(), String> {
        let config_path = var("CONFIG_PATH").expect("Provide CONFIG_PATH environment variable");
        let config_string = toml::to_string(self).expect("Failed to serialize Config");

        write(config_path, config_string).map_err(|error| error.to_string())
    }
}

fn main() {
    let config = Config::read();
    let mut bot = Bot::new(&config.username, &config.password, config.admin_list)
        .expect("Failed to create bot");

    bot.select_character().expect("Failed to select character");

    loop {
        bot.tick().expect("Failed to run bot")
    }
}
