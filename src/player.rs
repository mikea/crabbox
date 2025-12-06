use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use rodio::{OutputStream, OutputStreamBuilder, Sink};
use tokio::task::JoinHandle;
use tokio::{sync::mpsc, task};
use tracing::{error, info};

use crate::commands::Command;

pub const VOLUME_STEP: f32 = 0.05;
pub const MAX_VOLUME: f32 = 1.0;
pub const MIN_VOLUME: f32 = 0.01;

pub struct Player {
    sink: Option<Arc<Sink>>,
    stream: Option<OutputStream>,
    volume: f32,
    track_end_task: Option<JoinHandle<()>>,
    command_sender: mpsc::Sender<Command>,
}

impl Player {
    pub fn new(volume: f32, command_sender: mpsc::Sender<Command>) -> Self {
        Self {
            volume,
            sink: None,
            stream: None,
            track_end_task: None,
            command_sender,
        }
    }

    fn new_stream() -> Result<OutputStream, String> {
        OutputStreamBuilder::open_default_stream()
            .map_err(|err| format!("Failed to open default audio output: {err}"))
    }

    pub fn play(&mut self, track: &Path, notify: bool) -> Result<(), String> {
        let stream = Self::new_stream()?;

        let file = File::open(track)
            .map_err(|err| format!("Failed to open file {}: {err}", track.display()))?;

        let sink = rodio::play(stream.mixer(), file)
            .map_err(|err| format!("Failed to start file {}: {err}", track.display()))?;
        sink.set_volume(self.volume);

        self.stream = Some(stream);
        self.sink = Some(Arc::new(sink));

        if notify {
            self.watch_for_track_end();
        }

        Ok(())
    }

    pub fn stop(&mut self) {
        self.cancel_track_end_task();
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        if let Some(_stream) = self.stream.take() {
            // todo stop playback so that it is not logged?
        }
    }

    pub fn has_sink(&self) -> bool {
        self.sink.is_some()
    }

    pub fn is_paused(&self) -> bool {
        self.sink
            .as_ref()
            .is_some_and(|s| Sink::is_paused(s.as_ref()))
    }

    pub fn pause(&mut self) {
        if let Some(sink) = self.sink.as_deref() {
            sink.pause();
        }
    }

    pub fn resume(&mut self) {
        if let Some(sink) = self.sink.as_deref() {
            sink.play();
        }
    }

    pub fn volume_up(&mut self) {
        self.adjust_volume(VOLUME_STEP);
    }

    pub fn volume_down(&mut self) {
        self.adjust_volume(-VOLUME_STEP);
    }

    fn adjust_volume(&mut self, delta: f32) {
        let new_volume = (self.volume + delta).clamp(MIN_VOLUME, MAX_VOLUME);
        self.volume = new_volume;
        if let Some(sink) = self.sink.as_deref() {
            sink.set_volume(new_volume);
        }
        info!("Volume set to {:.2}", new_volume);
    }

    fn cancel_track_end_task(&mut self) {
        if let Some(task) = self.track_end_task.take() {
            task.abort();
        }
    }

    pub fn watch_for_track_end(&mut self) {
        self.cancel_track_end_task();

        let Some(sink) = self.sink.clone() else {
            return;
        };

        let sender = self.command_sender.clone();

        let handle = task::spawn(async move {
            let wait_result = task::spawn_blocking(move || sink.sleep_until_end()).await;

            if wait_result.is_ok() {
                let _ = sender.send(Command::TrackDone).await;
            }
        });

        self.track_end_task = Some(handle);
    }

    pub fn wait_until_end(&self) {
        if let Some(sink) = self.sink.as_ref() {
            sink.sleep_until_end();
        }
    }
}

pub fn play_track(track: Option<PathBuf>, player: &mut Player, notify: bool) -> Option<PathBuf> {
    let Some(track) = track else {
        error!("No tracks available to play");
        return None;
    };

    player.stop();

    match player.play(track.as_path(), notify) {
        Ok(()) => Some(track),
        Err(err) => {
            error!("{err}");
            None
        }
    }
}

pub fn toggle_play_pause(
    track: Option<PathBuf>,
    player: &mut Player,
    notify: bool,
) -> ToggleResult {
    if player.has_sink() {
        if player.is_paused() {
            player.resume();
        } else {
            player.pause();
        }
        ToggleResult::Toggled
    } else {
        match play_track(track, player, notify) {
            Some(track) => ToggleResult::Started(track),
            None => ToggleResult::Stopped,
        }
    }
}

pub enum ToggleResult {
    Started(PathBuf),
    Toggled,
    Stopped,
}

impl Drop for Player {
    fn drop(&mut self) {
        // Ensure playback is stopped cleanly before the stream is dropped.
        self.stop();
    }
}

pub fn play_blocking(track: &Path, volume: f32) -> Result<(), String> {
    let (tx, _rx) = mpsc::channel(1);
    let mut player = Player::new(volume, tx);
    player.play(track, false)?;
    player.wait_until_end();
    Ok(())
}
