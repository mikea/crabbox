use std::path::PathBuf;

use tokio::sync::mpsc;
use walkdir::WalkDir;

use crate::config::{Config, MusicDirectory};

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Play,
    Stop,
}

#[derive(Debug)]
pub struct Crabbox {
    pub library: Vec<PathBuf>,
    command_tx: mpsc::Sender<Command>,
}

impl Crabbox {
    pub fn new(config: &Config) -> Self {
        let library = collect_music_files(&config.music);
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(async move {
            process_commands(rx).await;
        });

        Self {
            library,
            command_tx: tx,
        }
    }

    pub fn sender(&self) -> mpsc::Sender<Command> {
        self.command_tx.clone()
    }
}

async fn process_commands(mut rx: mpsc::Receiver<Command>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::Play => println!("Command received: Play"),
            Command::Stop => println!("Command received: Stop"),
        }
    }
}

fn collect_music_files(directories: &[MusicDirectory]) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for dir in directories {
        for entry in WalkDir::new(&dir.dir).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }

            if let Some(ext) = entry.path().extension().and_then(|os| os.to_str())
                && is_music_extension(ext)
            {
                files.push(entry.into_path());
            }
        }
    }

    files
}

fn is_music_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "opus" | "alac"
    )
}
