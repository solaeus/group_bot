/// Configuration used to initiate the bot.
///
/// The Config struct is used to store configuration values that are not sensitive. The Secrets
/// struct is used to store sensitive information that should not be shared. This should be read
/// from a separate file that is not checked into version control. In production, use a secure
/// means of storing this information, such as the secret manager for Podman.
use serde::Deserialize;

#[derive(Default, Deserialize)]
/// Non-sensitive configuration values.
///
/// See the [module-level documentation](index.html) for more information.
pub struct Config {
    pub game_server: Option<String>,
    pub auth_server: Option<String>,
    pub position: Option<[f32; 3]>,
    pub orientation: Option<f32>,
}

#[derive(Deserialize)]
/// Sensitive configuration values.
///
/// See the [module-level documentation](index.html) for more information.
pub struct Secrets {
    pub username: String,
    pub password: String,
    pub character: String,
    pub admins: Vec<String>,
}
