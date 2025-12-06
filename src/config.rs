use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{commands::Command, tag::TagId};

use serde::Deserialize;
use tracing::warn;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "music")]
    pub music: Vec<MusicDirectory>,
    pub server: ServerConfig,
    #[serde(default = "default_volume")]
    pub default_volume: f32,
    #[serde(default)]
    pub tags: HashMap<TagId, Command>,
    #[serde(default)]
    pub state_file: Option<PathBuf>,
    #[cfg(feature = "rpi")]
    pub gpio: Option<GpioConfig>,
    #[cfg(feature = "rpi")]
    pub rfid: Option<RfidConfig>,
}

#[derive(Debug, Deserialize)]
pub struct MusicDirectory {
    pub dir: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub pipe: Option<PathBuf>,
    pub web: String,
    #[serde(default)]
    pub startup_sound: Option<PathBuf>,
    #[serde(default)]
    pub shutdown_sound: Option<PathBuf>,
}

#[cfg(feature = "rpi")]
#[derive(Debug, Deserialize)]
pub struct GpioConfig {
    #[serde(default)]
    pub play: Option<u8>,
    #[serde(default = "default_gpio_debounce_ms")]
    pub debounce_ms: u64,
    #[serde(default)]
    pub next: Option<u8>,
    #[serde(default)]
    pub prev: Option<u8>,
    #[serde(default)]
    pub volume_up: Option<u8>,
    #[serde(default)]
    pub volume_down: Option<u8>,
    #[serde(default)]
    pub shutdown: Option<u8>,
}

#[cfg(feature = "rpi")]
#[derive(Debug, Deserialize)]
pub struct RfidConfig {
    pub bus: u8,
    pub irq: u8,
    #[serde(default)]
    pub reset: Option<u8>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let raw = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&raw)?;

        if config.music.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "config must include at least one [[music]] entry",
            )
            .into());
        }

        if config.server.web.trim().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "[server].web must be a non-empty address",
            )
            .into());
        }

        if let Some(sound) = &config.server.startup_sound {
            if !sound.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "startup_sound must point to an existing file",
                )
                .into());
            }
        }

        if let Some(sound) = &config.server.shutdown_sound {
            if !sound.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "shutdown_sound must point to an existing file",
                )
                .into());
            }
        }

        #[cfg(feature = "rpi")]
        let _ = (&config.gpio, &config.rfid);

        if config.state_file.is_none() {
            warn!("State file not configured; state will not persist between runs");
        }

        Ok(config)
    }
}

const fn default_volume() -> f32 {
    1.0
}

#[cfg(feature = "rpi")]
const fn default_gpio_debounce_ms() -> u64 {
    200
}
