use std::collections::HashSet;

use cosmic::cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};
use kdeconnect::device::{ConnectedId, Linked};

use crate::{APP_ID, CONFIG_VERSION};

#[derive(Debug, Default, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct ConnectConfig {
    pub last_connections: HashSet<Linked>,
    pub paired: Vec<ConnectedId>,
}

impl ConnectConfig {
    pub fn config_handler() -> Option<cosmic_config::Config> {
        cosmic_config::Config::new(APP_ID, CONFIG_VERSION).ok()
    }
    pub fn config() -> ConnectConfig {
        match Self::config_handler() {
            Some(config_handler) => {
                ConnectConfig::get_entry(&config_handler).unwrap_or_else(|(errs, config)| {
                    tracing::info!("errors loading config: {:?}", errs);
                    config
                })
            }
            None => ConnectConfig::default(),
        }
    }
}
