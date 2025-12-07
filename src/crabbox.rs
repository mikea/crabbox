use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
};

use chrono::Utc;
use rand::{rng, seq::SliceRandom};
use tokio::{runtime::Builder, sync::mpsc};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::{
    commands::Command,
    config::{Config, MusicDirectory},
    glob::Glob,
    player::{Player, ToggleResult, play_blocking, play_track, toggle_play_pause},
    state::State,
    tag::TagId,
};
use toml_edit::{DocumentMut, Value, table, value};

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
    pub tags: Vec<(TagId, Command)>,
    pub last_tag: Option<TagId>,
    pub last_tag_command: Option<Command>,
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
        let mut tracks = collect_music_files(&self.directories);

        let Some(filter) = filter else {
            return tracks;
        };

        match Glob::new(&filter) {
            Ok(glob) => {
                tracks.retain(|path| glob.is_match_path(path));
                tracks
            }
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

    fn from_state(state: State) -> Self {
        let mut queue = Self {
            tracks: state.queue,
            current: state.position,
        };

        if let Some(idx) = queue.current
            && idx >= queue.tracks.len()
        {
            warn!(
                idx,
                len = queue.tracks.len(),
                "Restored queue position out of bounds"
            );
            queue.current = None;
        }

        queue
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
            debug!("{track:?}");
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
    state_file: Option<PathBuf>,
    config_path: PathBuf,
    config_backup_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
enum QueueOrder {
    Ordered,
    Shuffled,
}

impl Crabbox {
    pub fn new(config: &Config) -> Arc<Mutex<Self>> {
        let library = Library::new(&config.music);
        let state_file = config.state_file.clone();
        let queue = match state_file.as_ref().filter(|path| path.exists()) {
            Some(path) => match State::load(path) {
                Ok(state) => {
                    let queue = Queue::from_state(state);
                    info!(?path, "Restored playback state from file");
                    queue.log();
                    queue
                }
                Err(err) => {
                    warn!(?path, "Failed to load playback state: {err}");
                    Queue::empty()
                }
            },
            None => Queue::empty(),
        };
        let tags = config.tags.clone();
        let (tx, rx) = mpsc::channel(16);
        let status = PlaybackStatus {
            current: queue.current_track(),
            ..PlaybackStatus::default()
        };
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
            state_file,
            config_path: config.path.clone(),
            config_backup_dir: config.backup_dir.clone(),
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
        let last_tag_command = self
            .status
            .last_tag
            .and_then(|tag| self.tags.get(&tag).cloned());
        let mut tags: Vec<_> = self
            .tags
            .iter()
            .map(|(id, command)| (*id, command.clone()))
            .collect();
        tags.sort_by(|(left, _), (right, _)| left.to_string().cmp(&right.to_string()));

        CrabboxSnapshot {
            current: self.status.current.clone(),
            queue: self.queue.tracks.clone(),
            queue_position: self.queue.current,
            tags,
            last_tag: self.status.last_tag,
            last_tag_command,
        }
    }

    pub fn music_directories(&self) -> Vec<PathBuf> {
        self.library.directories.clone()
    }

    fn process_command(&mut self, cmd: Command, player: &mut Player) {
        match cmd {
            Command::Play { filter } => {
                let filter = filter.as_deref();
                debug!(?filter, "Command received: Play");
                self.rebuild_queue(filter, QueueOrder::Ordered);
                player.stop();

                let track = self.queue.current_track();

                self.play_queue_track(track, player);
            }
            Command::PlayPause { filter } => {
                self.on_play_pause(player, filter.as_ref());
            }
            Command::Shuffle { filter } => {
                let filter = filter.as_deref();
                self.rebuild_queue(filter, QueueOrder::Shuffled);
                player.stop();

                let track = self.queue.current_track();

                self.play_queue_track(track, player);
                debug!(?filter, "Command received: Shuffle");
            }
            Command::Stop => {
                player.stop();
                self.status.current = None;
                self.save_state();
                debug!("Command received: Stop");
            }
            Command::Next => {
                let track = self.queue.next_track();

                self.play_queue_track(track, player);
                debug!("Command received: Next");
            }
            Command::Prev => {
                let track = self.queue.prev_track();

                self.play_queue_track(track, player);
                debug!("Command received: Prev");
            }
            Command::TrackDone => {
                let track = self.queue.next_track();

                self.play_queue_track(track, player);
                debug!("Command received: TrackDone");
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
                self.save_state();
                if let Some(sound) = self.shutdown_sound.as_ref()
                    && let Err(err) = play_blocking(sound, self.default_volume)
                {
                    warn!("Failed to play shutdown sound {}: {err}", sound.display());
                }
                if let Err(err) = shutdown_now() {
                    warn!("Failed to trigger shutdown: {err}");
                }
            }
            Command::AssignTag { id, command } => {
                self.assign_tag(id, command.as_deref());
                debug!(?id, "Command received: AssignTag");
            }
            #[cfg(feature = "rpi")]
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

    fn on_play_pause(&mut self, player: &mut Player, filter: Option<&String>) {
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
            match play_track(track, player, true) {
                Some(track) => ToggleResult::Started(track),
                None => ToggleResult::Stopped,
            }
        } else {
            toggle_play_pause(track, player, true)
        };

        match toggle_result {
            ToggleResult::Started(track) => {
                self.status.current = Some(track.clone());
            }
            ToggleResult::Stopped => self.status.current = None,
            ToggleResult::Toggled => {}
        }
        self.save_state();
        debug!(?filter, "Command received: PlayPause");
    }

    fn play_queue_track(&mut self, track: Option<PathBuf>, player: &mut Player) {
        match play_track(track, player, true) {
            Some(track) => {
                self.status.current = Some(track.clone());
            }
            None => self.status.current = None,
        }
        self.save_state();
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
        self.save_state();
    }

    fn assign_tag(&mut self, id: TagId, command: Option<&str>) {
        let parsed_command = command
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(Command::from_str)
            .transpose();

        match parsed_command {
            Ok(Some(parsed_command)) => {
                self.tags.insert(id, parsed_command.clone());
                if let Err(err) = self.persist_tag_mapping(id, Some(&parsed_command)) {
                    warn!(?id, ?err, "Failed to save tag mapping to config");
                }
            }
            Ok(None) => {
                self.tags.remove(&id);
                if let Err(err) = self.persist_tag_mapping(id, None) {
                    warn!(?id, ?err, "Failed to remove tag mapping from config");
                }
            }
            Err(err) => warn!(?id, command, "Invalid command for tag: {err}"),
        }
    }

    fn persist_tag_mapping(&self, id: TagId, command: Option<&Command>) -> Result<(), String> {
        let config_raw = fs::read_to_string(&self.config_path).map_err(|err| err.to_string())?;
        let mut document: DocumentMut = config_raw
            .parse::<DocumentMut>()
            .map_err(|err| err.to_string())?;

        self.backup_config_file().map_err(|err| err.to_string())?;

        let tags = document.as_table_mut().entry("tags").or_insert_with(table);

        let Some(tags) = tags.as_table_mut() else {
            return Err("[tags] is not a table".to_string());
        };

        let tag_key = id.to_string();
        match command {
            Some(command) => {
                if let Some(existing) = tags.get_mut(&tag_key) {
                    if let Some(value_mut) = existing.as_value_mut() {
                        *value_mut = Value::from(command.to_string());
                    } else {
                        *existing = value(command.to_string());
                    }
                } else {
                    tags.insert(&tag_key, value(command.to_string()));
                }
            }
            None => {
                tags.remove(&tag_key);
            }
        }

        fs::write(&self.config_path, document.to_string()).map_err(|err| err.to_string())
    }

    fn backup_config_file(&self) -> Result<(), std::io::Error> {
        let Some(backup_dir) = self.config_backup_dir.as_ref() else {
            return Ok(());
        };

        fs::create_dir_all(backup_dir)?;
        let filename = self.config_path.file_name().map_or_else(
            || String::from("config.toml"),
            |name| name.to_string_lossy().into_owned(),
        );
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S");
        let backup_name = format!("{filename}.{timestamp}");
        let backup_path = backup_dir.join(backup_name);

        fs::copy(&self.config_path, backup_path)?;
        Ok(())
    }

    fn save_state(&self) {
        let Some(path) = self.state_file.as_ref() else {
            return;
        };

        let state = State {
            queue: self.queue.tracks.clone(),
            position: self.queue.current,
        };

        if let Err(err) = state.save(path) {
            warn!(?path, "Failed to save playback state: {err}");
        }
    }
}

async fn process_commands(
    mut rx: mpsc::Receiver<Command>,
    crabbox: Arc<Mutex<Crabbox>>,
    default_volume: f32,
) {
    let sender = {
        let crabbox = crabbox.lock().expect("failed to lock crabbox");
        crabbox.command_tx.clone()
    };
    let mut player = Player::new(default_volume, sender);

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

    files.sort();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn crabbox_with_config(config_path: PathBuf, backup_dir: Option<PathBuf>) -> Crabbox {
        let (tx, _rx) = mpsc::channel(1);

        Crabbox {
            library: Library {
                directories: vec![],
            },
            queue: Queue::empty(),
            tags: HashMap::new(),
            command_tx: tx,
            status: PlaybackStatus::default(),
            shutdown_sound: None,
            default_volume: 1.0,
            state_file: None,
            config_path,
            config_backup_dir: backup_dir,
        }
    }

    #[test]
    fn list_tracks_returns_sorted_paths() {
        let tmp = tempdir().expect("tempdir");
        let dir_a = tmp.path().join("a_dir");
        let dir_b = tmp.path().join("b_dir");
        fs::create_dir_all(&dir_a).expect("create dir a");
        fs::create_dir_all(&dir_b).expect("create dir b");

        let path_a = dir_a.join("track.mp3");
        let path_b = dir_b.join("track.mp3");
        fs::write(&path_b, "audio").expect("write track_b");
        fs::write(&path_a, "audio").expect("write track_a");

        let library = Library {
            directories: vec![dir_b, dir_a],
        };

        let tracks = library.list_tracks(None);

        let mut expected = vec![path_a, path_b];
        expected.sort();

        assert_eq!(tracks, expected);
    }

    #[test]
    fn persist_tag_mapping_creates_backup_before_saving() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("config.toml");
        let backup_dir = tmp.path().join("backups");
        let initial_config = r#"[[music]]
dir = "/music"

[server]
web = "0.0.0.0:8080"

[tags]
0A1B2C3D = "PLAY"
"#;

        fs::write(&config_path, initial_config).expect("write config");

        let crabbox = crabbox_with_config(config_path.clone(), Some(backup_dir.clone()));

        crabbox
            .persist_tag_mapping(
                TagId::from_hex_str("0A1B2C3D").unwrap(),
                Some(&Command::Stop),
            )
            .expect("persist tag");

        let backups: Vec<_> = fs::read_dir(&backup_dir)
            .expect("read backup dir")
            .map(|entry| entry.expect("dir entry").path())
            .collect();

        assert_eq!(backups.len(), 1, "expected exactly one backup file");

        let backup_contents = fs::read_to_string(&backups[0]).expect("backup contents");
        assert_eq!(backup_contents, initial_config);

        let updated_config = fs::read_to_string(config_path).expect("updated config");
        assert!(updated_config.contains("0A1B2C3D = \"STOP\""));
    }

    #[test]
    fn persist_tag_mapping_preserves_existing_tags_table() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("config.toml");
        let initial_config = r#"[[music]]
dir = "/music"

[server]
web = "0.0.0.0:8080"

[tags]
# keep this comment
ABCD1234 = "PLAY"
# untouched entry
DEADBEEF = "SHUFFLE 80s/*"
"#;

        fs::write(&config_path, initial_config).expect("write config");

        let crabbox = crabbox_with_config(config_path.clone(), None);

        crabbox
            .persist_tag_mapping(
                TagId::from_hex_str("ABCD1234").unwrap(),
                Some(&Command::Stop),
            )
            .expect("persist tag");

        let updated_config = fs::read_to_string(config_path).expect("updated config");

        assert!(updated_config.contains("# keep this comment"));
        assert!(updated_config.contains("# untouched entry"));
        assert!(updated_config.contains("DEADBEEF = \"SHUFFLE 80s/*\""));
        assert!(updated_config.contains("ABCD1234 = \"STOP\""));
    }

    #[test]
    fn persist_tag_mapping_removes_entry_without_touching_other_tags() {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("config.toml");
        let initial_config = r#"[[music]]
dir = "/music"

[server]
web = "0.0.0.0:8080"

[tags]
# primary tag comment
ABCD1234 = "PLAY"
# preserve this entry
DEADBEEF = "SHUFFLE 80s/*"
"#;

        fs::write(&config_path, initial_config).expect("write config");

        let crabbox = crabbox_with_config(config_path.clone(), None);

        crabbox
            .persist_tag_mapping(TagId::from_hex_str("ABCD1234").unwrap(), None)
            .expect("persist tag");

        let updated_config = fs::read_to_string(config_path).expect("updated config");

        assert!(updated_config.contains("# preserve this entry"));
        assert!(updated_config.contains("DEADBEEF = \"SHUFFLE 80s/*\""));
        assert!(!updated_config.contains("ABCD1234"));
    }
}
