use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub queue: Vec<PathBuf>,
    pub position: Option<usize>,
}

impl State {
    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }
}
