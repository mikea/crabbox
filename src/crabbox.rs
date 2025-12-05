use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use rand::{rng, seq::SliceRandom};
use tokio::{runtime::Builder, sync::mpsc};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::{
    commands::Command,
    config::{Config, MusicDirectory},
    glob::Glob,
    player::{Player, ToggleResult, play_blocking, play_track, toggle_play_pause},
    tag::TagId,
};

#[derive(Default)]
struct PlaybackStatus {
    current: Option<PathBuf>,
    last_tag: Option<TagId>,
}

#[derive(Clone, Default)]
pub struct CrabboxSnapshot {
    pub current: Option<PathBuf>,
    pub queue: Vec<PathBuf>,
    pub queue_position: Option<usize>,
    pub library: Vec<PathBuf>,
    pub last_tag: Option<TagId>,
}

#[derive(Clone, Default)]
pub struct Library {
    directories: Vec<PathBuf>,
}

impl Library {
    fn new(directories: &[MusicDirectory]) -> Self {
        Self {
            directories: directories.iter().map(|d| d.dir.clone()).collect(),
        }
    }

    pub fn list_tracks(&self, filter: Option<String>) -> Vec<PathBuf> {
        let tracks = collect_music_files(&self.directories);

        let Some(filter) = filter else {
            return tracks;
        };

        match Glob::new(&filter) {
            Ok(glob) => tracks
                .into_iter()
                .filter(|path| glob.is_match_path(path))
                .collect(),
            Err(err) => {
                warn!(?filter, "Invalid glob: {err}");
                Vec::new()
            }
        }
    }
}

pub struct Queue {
    tracks: Vec<PathBuf>,
    current: Option<usize>,
}

impl Queue {
    fn from_tracks_ordered(tracks: Vec<PathBuf>) -> Self {
        let current = if tracks.is_empty() { None } else { Some(0) };
        Self { tracks, current }
    }

    fn empty() -> Self {
        Self {
            tracks: Vec::new(),
            current: None,
        }
    }

    fn from_tracks_shuffled(mut tracks: Vec<PathBuf>) -> Self {
        tracks.shuffle(&mut rng());
        let current = if tracks.is_empty() { None } else { Some(0) };

        Self { tracks, current }
    }

    fn current_track(&self) -> Option<PathBuf> {
        self.current.and_then(|idx| self.tracks.get(idx)).cloned()
    }

    fn track_at(&self, idx: usize) -> Option<PathBuf> {
        self.tracks.get(idx).cloned()
    }

    fn next_track(&mut self) -> Option<PathBuf> {
        if self.tracks.is_empty() {
            return None;
        }

        let next_idx = match self.current {
            Some(idx) => (idx + 1) % self.tracks.len(),
            None => 0,
        };

        self.current = Some(next_idx);
        self.track_at(next_idx)
    }

    fn prev_track(&mut self) -> Option<PathBuf> {
        if self.tracks.is_empty() {
            return None;
        }

        let prev_idx = match self.current {
            Some(idx) => (idx + self.tracks.len() - 1) % self.tracks.len(),
            None => 0,
        };

        self.current = Some(prev_idx);
        self.track_at(prev_idx)
    }

    fn log(&self) {
        info!("new queue: {} tracks", self.tracks.len());
        for track in &self.tracks {
            debug!("{track:?}")
        }
    }
}

pub struct Crabbox {
    pub library: Library,
    pub queue: Queue,
    tags: HashMap<TagId, Command>,
    command_tx: mpsc::Sender<Command>,
    status: PlaybackStatus,
    shutdown_sound: Option<PathBuf>,
    default_volume: f32,
}

enum QueueOrder {
    Ordered,
    Shuffled,
}

impl Crabbox {
    pub fn new(config: &Config) -> Arc<Mutex<Self>> {
        let library = Library::new(&config.music);
        let queue = Queue::empty();
        let tags = config.tags.clone();
        let (tx, rx) = mpsc::channel(16);
        let status = PlaybackStatus::default();
        let shutdown_sound = config.server.shutdown_sound.clone();
        let default_volume = config.default_volume;

        let crabbox = Arc::new(Mutex::new(Self {
            library,
            queue,
            tags,
            command_tx: tx,
            status,
            shutdown_sound,
            default_volume,
        }));

        thread::spawn({
            let playback_crabbox = Arc::clone(&crabbox);
            let default_volume = config.default_volume;
            move || {
                // Run playback logic on a single-threaded runtime so we can hold
                // non-Send audio types without fighting the async scheduler.
                let rt = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build playback runtime");
                rt.block_on(process_commands(rx, playback_crabbox, default_volume));
            }
        });

        crabbox
    }

    pub fn sender(&self) -> mpsc::Sender<Command> {
        self.command_tx.clone()
    }

    pub fn snapshot(&self) -> CrabboxSnapshot {
        CrabboxSnapshot {
            current: self.status.current.clone(),
            queue: self.queue.tracks.clone(),
            queue_position: self.queue.current,
            library: self.library.list_tracks(None),
            last_tag: self.status.last_tag,
        }
    }

    fn process_command(&mut self, cmd: Command, player: &mut Player) {
        match cmd {
            Command::Play { filter } => {
                let filter = filter.as_deref();
                debug!(?filter, "Command received: Play");
                self.rebuild_queue(filter, QueueOrder::Ordered);
                player.stop();

                let track = self.queue.current_track();

                if let Some(track) = play_track(track, player) {
                    self.status.current = Some(track);
                }
            }
            Command::PlayPause { filter } => {
                let filter = filter.as_deref();
                let queue_rebuilt = if let Some(filter) = filter {
                    self.rebuild_queue(Some(filter), QueueOrder::Ordered);
                    true
                } else {
                    false
                };

                if queue_rebuilt {
                    player.stop();
                }

                let track = self.queue.current_track();

                let toggle_result = if queue_rebuilt {
                    match play_track(track, player) {
                        Some(track) => ToggleResult::Started(track),
                        None => ToggleResult::Stopped,
                    }
                } else {
                    toggle_play_pause(track, player)
                };

                match toggle_result {
                    ToggleResult::Started(track) => self.status.current = Some(track),
                    ToggleResult::Stopped => self.status.current = None,
                    ToggleResult::Toggled => {}
                }
                debug!(?filter, "Command received: PlayPause");
            }
            Command::Shuffle { filter } => {
                let filter = filter.as_deref();
                self.rebuild_queue(filter, QueueOrder::Shuffled);
                player.stop();

                let track = self.queue.current_track();

                if let Some(track) = play_track(track, player) {
                    self.status.current = Some(track);
                    debug!(?filter, "Command received: Shuffle");
                }
            }
            Command::Stop => {
                player.stop();
                self.status.current = None;
                debug!("Command received: Stop");
            }
            Command::Next => {
                let track = self.queue.next_track();

                if let Some(track) = play_track(track, player) {
                    self.status.current = Some(track);
                    debug!("Command received: Next");
                }
            }
            Command::Prev => {
                let track = self.queue.prev_track();

                if let Some(track) = play_track(track, player) {
                    self.status.current = Some(track);
                    debug!("Command received: Prev");
                }
            }
            Command::VolumeUp => {
                player.volume_up();
                debug!("Command received: VolumeUp");
            }
            Command::VolumeDown => {
                player.volume_down();
                debug!("Command received: VolumeDown");
            }
            Command::Shutdown => {
                debug!("Command received: Shutdown");
                player.stop();
                self.status.current = None;
                if let Some(sound) = self.shutdown_sound.as_ref() {
                    if let Err(err) = play_blocking(sound, self.default_volume) {
                        warn!("Failed to play shutdown sound {}: {err}", sound.display());
                    }
                }
                if let Err(err) = shutdown_now() {
                    warn!("Failed to trigger shutdown: {err}");
                }
            }
            Command::Tag { id } => {
                self.status.last_tag = Some(id);
                match self.tags.get(&id).cloned() {
                    Some(Command::Tag { .. }) => {
                        warn!(?id, "Tag is mapped to another tag command; ignoring");
                    }
                    Some(mapped) => self.process_command(mapped, player),
                    None => debug!(?id, "No command mapped for tag"),
                }
            }
        }
    }

    fn rebuild_queue(&mut self, filter: Option<&str>, order: QueueOrder) {
        let tracks = self.library.list_tracks(filter.map(str::to_string));

        if tracks.is_empty() {
            if let Some(filter) = filter {
                warn!(filter, "Filter matched no tracks");
            } else {
                warn!("Library is empty");
            }
        }

        self.queue = match order {
            QueueOrder::Ordered => Queue::from_tracks_ordered(tracks),
            QueueOrder::Shuffled => Queue::from_tracks_shuffled(tracks),
        };
        self.queue.log();
        self.status.current = None;
    }
}

async fn process_commands(
    mut rx: mpsc::Receiver<Command>,
    crabbox: Arc<Mutex<Crabbox>>,
    default_volume: f32,
) {
    let mut player = Player::new(default_volume);

    while let Some(cmd) = rx.recv().await {
        if let Ok(mut crabbox) = crabbox.lock() {
            crabbox.process_command(cmd, &mut player);
        }
    }
}

fn collect_music_files(directories: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for dir in directories {
        for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
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

fn shutdown_now() -> std::io::Result<()> {
    use std::process::Command;

    Command::new("sudo")
        .args(["shutdown", "now"])
        .spawn()
        .map(|_| ())
}
