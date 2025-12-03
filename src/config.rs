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

        Ok(config)
    }
}
