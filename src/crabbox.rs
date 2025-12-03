use std::path::PathBuf;

use crate::config::MusicDirectory;
use walkdir::WalkDir;

#[derive(Debug)]
pub struct Crabbox {
    pub library: Vec<PathBuf>,
}

impl Crabbox {
    pub fn new(config: &crate::config::Config) -> Self {
        let library = collect_music_files(&config.music);
        Self { library }
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
