use std::{
    fs::File,
    path::{Path, PathBuf},
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
    sink: Option<Sink>,
    stream: Option<OutputStream>,
    volume: f32,
    track_end_task: Option<JoinHandle<()>>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            sink: None,
            stream: None,
            volume: 1.0,
            track_end_task: None,
        }
    }
}

impl Player {
    pub fn new(volume: f32) -> Self {
        Self {
            volume,
            ..Default::default()
        }
    }

    fn new_stream(&mut self) -> Result<OutputStream, String> {
        OutputStreamBuilder::open_default_stream()
            .map_err(|err| format!("Failed to open default audio output: {err}"))
    }

    pub fn play(&mut self, track: &Path) -> Result<(), String> {
        let stream = self.new_stream()?;

        let file = File::open(track)
            .map_err(|err| format!("Failed to open file {}: {err}", track.display()))?;

        let sink = rodio::play(stream.mixer(), file)
            .map_err(|err| format!("Failed to start file {}: {err}", track.display()))?;
        sink.set_volume(self.volume);

        self.stream = Some(stream);
        self.sink = Some(sink);

        Ok(())
    }

    pub fn stop(&mut self) {
        self.cancel_track_end_task();
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.stream = None;
    }

    pub fn has_sink(&self) -> bool {
        self.sink.is_some()
    }

    pub fn is_paused(&self) -> bool {
        self.sink.as_ref().map(Sink::is_paused).unwrap_or(false)
    }

    pub fn pause(&mut self) {
        if let Some(sink) = self.sink.as_ref() {
            sink.pause();
        }
    }

    pub fn resume(&mut self) {
        if let Some(sink) = self.sink.as_ref() {
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
        if let Some(sink) = self.sink.as_ref() {
            sink.set_volume(new_volume);
        }
        info!("Volume set to {:.2}", new_volume);
    }

    fn cancel_track_end_task(&mut self) {
        if let Some(task) = self.track_end_task.take() {
            task.abort();
        }
    }

    pub fn watch_for_track_end(&mut self, sender: mpsc::Sender<Command>) {
        self.cancel_track_end_task();

        let Some(sink) = self.sink.as_ref().cloned() else {
            return;
        };

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

pub fn play_track(track: Option<PathBuf>, player: &mut Player) -> Option<PathBuf> {
    let Some(track) = track else {
        error!("No tracks available to play");
        return None;
    };

    player.stop();

    match player.play(track.as_path()) {
        Ok(()) => Some(track),
        Err(err) => {
            error!("{err}");
            None
        }
    }
}

pub fn toggle_play_pause(track: Option<PathBuf>, player: &mut Player) -> ToggleResult {
    if player.has_sink() {
        if player.is_paused() {
            player.resume();
        } else {
            player.pause();
        }
        ToggleResult::Toggled
    } else {
        match play_track(track, player) {
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

pub fn play_blocking(track: &Path, volume: f32) -> Result<(), String> {
    let mut player = Player::new(volume);
    player.play(track)?;
    player.wait_until_end();
    Ok(())
}
