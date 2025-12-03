use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "music")]
    pub music: Vec<MusicDirectory>,
}

#[derive(Debug, Deserialize)]
pub struct MusicDirectory {
    pub dir: PathBuf,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let raw = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&raw)?;

        if config.music.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "config must include at least one [[music]] entry",
            )
            .into());
        }

        Ok(config)
    }
}
