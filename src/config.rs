use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "music")]
    pub music: Vec<MusicDirectory>,
    pub server: ServerConfig,
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
}

#[cfg(feature = "rpi")]
#[derive(Debug, Deserialize)]
pub struct GpioConfig {
    pub play: u8,
    #[serde(default = "default_gpio_debounce_ms")]
    pub debounce_ms: u64,
    #[serde(default)]
    pub next: Option<u8>,
    #[serde(default)]
    pub prev: Option<u8>,
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

        #[cfg(feature = "rpi")]
        let _ = (&config.gpio, &config.rfid);

        Ok(config)
    }
}

#[cfg(feature = "rpi")]
const fn default_gpio_debounce_ms() -> u64 {
    200
}
