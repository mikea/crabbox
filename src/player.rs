use std::{fs::File, path::PathBuf};

use rodio::{OutputStream, OutputStreamBuilder, Sink};
use tracing::{error, info};

pub const VOLUME_STEP: f32 = 0.05;
pub const MAX_VOLUME: f32 = 2.0;

pub struct Player {
    sink: Option<Sink>,
    stream: Option<OutputStream>,
    volume: f32,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            sink: None,
            stream: None,
            volume: 1.0,
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

    pub fn play(&mut self, track: &PathBuf) -> Result<(), String> {
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
        let new_volume = (self.volume + delta).clamp(0.0, MAX_VOLUME);
        self.volume = new_volume;
        if let Some(sink) = self.sink.as_ref() {
            sink.set_volume(new_volume);
        }
        info!("Volume set to {:.2}", new_volume);
    }
}

pub fn play_track(track: Option<PathBuf>, player: &mut Player) -> Option<PathBuf> {
    let Some(track) = track else {
        error!("No tracks available to play");
        return None;
    };

    player.stop();

    match player.play(&track) {
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
