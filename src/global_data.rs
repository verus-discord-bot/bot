use serenity::prelude::TypeMapKey;

pub struct AppConfig;

impl TypeMapKey for AppConfig {
    type Value = crate::configuration::Settings;
}
