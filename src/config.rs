use serde::Deserialize;

#[derive(Deserialize)]
/// Sensitive configuration values.
pub struct Secrets {
    pub username: String,
    pub password: String,
    pub character: String,
    pub admin_list: Vec<String>,
}
