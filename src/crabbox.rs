use std::{
    fs::File,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use rodio::{OutputStream, OutputStreamBuilder, Sink};
use tokio::{runtime::Builder, sync::mpsc};
use tracing::{debug, error};
use walkdir::WalkDir;

use crate::config::{Config, MusicDirectory};

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Play,
    PlayPause,
    Stop,
}

#[derive(Default)]
struct PlaybackStatus {
    current: Option<PathBuf>,
}

pub struct Crabbox {
    pub library: Vec<PathBuf>,
    command_tx: mpsc::Sender<Command>,
    status: Arc<Mutex<PlaybackStatus>>,
}

impl Crabbox {
    pub fn new(config: &Config) -> Self {
        let library = collect_music_files(&config.music);
        let (tx, rx) = mpsc::channel(16);
        let status = Arc::new(Mutex::new(PlaybackStatus::default()));

        thread::spawn({
            let status = Arc::clone(&status);
            let library = library.clone();
            move || {
                // Run playback logic on a single-threaded runtime so we can hold
                // non-Send audio types without fighting the async scheduler.
                let rt = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build playback runtime");
                rt.block_on(process_commands(rx, library, status));
            }
        });

        Self {
            library,
            command_tx: tx,
            status,
        }
    }

    pub fn sender(&self) -> mpsc::Sender<Command> {
        self.command_tx.clone()
    }

    pub fn current_track(&self) -> Option<PathBuf> {
        self.status.lock().ok()?.current.clone()
    }
}

async fn process_commands(
    mut rx: mpsc::Receiver<Command>,
    library: Vec<PathBuf>,
    status: Arc<Mutex<PlaybackStatus>>,
) {
    let mut player = Player::default();

    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::Play => {
                if let Some(track) = play_first(&library, &mut player) {
                    set_current_track(&status, Some(track));
                    debug!("Command received: Play");
                }
            }
            Command::PlayPause => {
                match toggle_play_pause(&library, &mut player) {
                    ToggleResult::Started(track) => set_current_track(&status, Some(track)),
                    ToggleResult::Stopped => set_current_track(&status, None),
                    ToggleResult::Toggled => {}
                }
                debug!("Command received: PlayPause");
            }
            Command::Stop => {
                player.stop();
                set_current_track(&status, None);
                debug!("Command received: Stop");
            }
        }
    }
}

fn play_first(library: &[PathBuf], player: &mut Player) -> Option<PathBuf> {
    let Some(track) = library.first() else {
        error!("No tracks available to play");
        return None;
    };

    player.stop();

    match player.play(track) {
        Ok(()) => Some(track.clone()),
        Err(err) => {
            error!("{err}");
            None
        }
    }
}

fn set_current_track(status: &Arc<Mutex<PlaybackStatus>>, track: Option<PathBuf>) {
    if let Ok(mut guard) = status.lock() {
        guard.current = track;
    }
}

fn toggle_play_pause(library: &[PathBuf], player: &mut Player) -> ToggleResult {
    if player.has_sink() {
        if player.is_paused() {
            player.resume();
        } else {
            player.pause();
        }
        ToggleResult::Toggled
    } else {
        match play_first(library, player) {
            Some(track) => ToggleResult::Started(track),
            None => ToggleResult::Stopped,
        }
    }
}

enum ToggleResult {
    Started(PathBuf),
    Toggled,
    Stopped,
}

#[derive(Default)]
struct Player {
    sink: Option<Sink>,
    stream: Option<OutputStream>,
}

impl Player {
    fn play(&mut self, track: &PathBuf) -> Result<(), String> {
        let stream = OutputStreamBuilder::open_default_stream()
            .map_err(|err| format!("Failed to open default audio output: {err}"))?;

        let file = File::open(track)
            .map_err(|err| format!("Failed to open file {}: {err}", track.display()))?;

        let sink = rodio::play(stream.mixer(), file)
            .map_err(|err| format!("Failed to start file {}: {err}", track.display()))?;

        self.stream = Some(stream);
        self.sink = Some(sink);

        Ok(())
    }

    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.stream = None;
    }

    fn has_sink(&self) -> bool {
        self.sink.is_some()
    }

    fn is_paused(&self) -> bool {
        self.sink.as_ref().map(Sink::is_paused).unwrap_or(false)
    }

    fn pause(&mut self) {
        if let Some(sink) = self.sink.as_ref() {
            sink.pause();
        }
    }

    fn resume(&mut self) {
        if let Some(sink) = self.sink.as_ref() {
            sink.play();
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
