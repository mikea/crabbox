#![cfg(feature = "rpi")]

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use rppal::gpio::{Gpio, InputPin, Trigger};
use tracing::{debug, error, info};

use crate::{commands::Command, config::GpioConfig, crabbox::Crabbox};

pub struct Button {
    _pin: InputPin,
}

impl Button {
    pub fn new(
        gpio: &Gpio,
        pin_number: u8,
        debounce: Duration,
        on_press: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut pin = gpio.get(pin_number)?.into_input_pullup();
        pin.set_async_interrupt(Trigger::FallingEdge, Some(debounce), move |_level| {
            on_press();
        })?;

        Ok(Self { _pin: pin })
    }
}

pub struct LongPressButton {
    _pin: InputPin,
    _timer: Arc<Mutex<Timer>>,
}

impl LongPressButton {
    pub fn new(
        gpio: &Gpio,
        pin_number: u8,
        debounce: Duration,
        hold_duration: Duration,
        on_hold: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut pin = gpio.get(pin_number)?.into_input_pullup();
        let timer = Arc::new(Mutex::new(Timer::new(hold_duration, on_hold)));
        let timer_for_interrupt = Arc::clone(&timer);
        pin.set_async_interrupt(Trigger::Both, Some(debounce), move |event| {
            match event.trigger {
                Trigger::FallingEdge => Timer::arm(&timer_for_interrupt),
                Trigger::RisingEdge => Timer::reset(&timer_for_interrupt),
                _ => {}
            }
        })?;

        Ok(Self {
            _pin: pin,
            _timer: timer,
        })
    }
}

pub struct GpioController {
    _play_button: Option<Button>,
    _next_button: Option<Button>,
    _prev_button: Option<Button>,
    _volume_up_button: Option<Button>,
    _volume_down_button: Option<Button>,
    _shutdown_button: Option<LongPressButton>,
}

impl GpioController {
    pub fn new(
        config: &GpioConfig,
        crabbox: Arc<Mutex<Crabbox>>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pins_configured = config.play.is_some()
            || config.next.is_some()
            || config.prev.is_some()
            || config.volume_up.is_some()
            || config.volume_down.is_some()
            || config.shutdown.is_some();

        if !pins_configured {
            info!("GPIO control disabled: no pins configured");
            return Ok(Self {
                _play_button: None,
                _next_button: None,
                _prev_button: None,
                _volume_up_button: None,
                _volume_down_button: None,
                _shutdown_button: None,
            });
        }

        let gpio = Gpio::new()?;
        let debounce_duration = Duration::from_millis(config.debounce_ms);

        let play_button = config
            .play
            .map(|pin| {
                Button::new(
                    &gpio,
                    pin,
                    debounce_duration,
                    make_sender(&crabbox, Command::PlayPause { filter: None }, "PlayPause"),
                )
            })
            .transpose()?;

        let next_button = config
            .next
            .map(|pin| {
                Button::new(
                    &gpio,
                    pin,
                    debounce_duration,
                    make_sender(&crabbox, Command::Next, "Next"),
                )
            })
            .transpose()?;

        let prev_button = config
            .prev
            .map(|pin| {
                Button::new(
                    &gpio,
                    pin,
                    debounce_duration,
                    make_sender(&crabbox, Command::Prev, "Prev"),
                )
            })
            .transpose()?;

        let volume_up_button = config
            .volume_up
            .map(|pin| {
                Button::new(
                    &gpio,
                    pin,
                    debounce_duration,
                    make_sender(&crabbox, Command::VolumeUp, "VolumeUp"),
                )
            })
            .transpose()?;

        let volume_down_button = config
            .volume_down
            .map(|pin| {
                Button::new(
                    &gpio,
                    pin,
                    debounce_duration,
                    make_sender(&crabbox, Command::VolumeDown, "VolumeDown"),
                )
            })
            .transpose()?;

        let shutdown_button = config
            .shutdown
            .map(|pin| {
                LongPressButton::new(
                    &gpio,
                    pin,
                    debounce_duration,
                    Duration::from_secs(5),
                    Arc::new(make_sender(
                        &crabbox,
                        Command::Shutdown,
                        "Shutdown (long press)",
                    )),
                )
            })
            .transpose()?;

        if let Some(pin) = config.play {
            info!("GPIO control enabled (play/pause pin {})", pin);
        }
        if let Some(pin) = config.next {
            info!("GPIO control enabled (next pin {})", pin);
        }
        if let Some(pin) = config.prev {
            info!("GPIO control enabled (prev pin {})", pin);
        }
        if let Some(pin) = config.volume_up {
            info!("GPIO control enabled (volume up pin {})", pin);
        }
        if let Some(pin) = config.volume_down {
            info!("GPIO control enabled (volume down pin {})", pin);
        }
        if let Some(pin) = config.shutdown {
            info!("GPIO control enabled (shutdown pin {}, hold 5s)", pin);
        }

        Ok(Self {
            _play_button: play_button,
            _next_button: next_button,
            _prev_button: prev_button,
            _volume_up_button: volume_up_button,
            _volume_down_button: volume_down_button,
            _shutdown_button: shutdown_button,
        })
    }
}

#[derive(Clone)]
pub struct Timer {
    duration: Duration,
    on_fire: Arc<dyn Fn() + Send + Sync + 'static>,
    generation: u64,
}

impl Timer {
    pub fn new(duration: Duration, on_fire: Arc<dyn Fn() + Send + Sync + 'static>) -> Self {
        Self {
            duration,
            on_fire,
            generation: 0,
        }
    }

    pub fn arm(timer: &Arc<Mutex<Self>>) {
        let (generation, duration, on_fire, timer_ref) = {
            let mut guard = timer.lock().expect("timer lock poisoned");
            guard.generation = guard.generation.wrapping_add(1);
            (
                guard.generation,
                guard.duration,
                Arc::clone(&guard.on_fire),
                Arc::clone(timer),
            )
        };

        thread::spawn(move || {
            thread::sleep(duration);
            let should_fire = timer_ref
                .lock()
                .map(|t| t.generation == generation)
                .unwrap_or(false);
            if should_fire {
                on_fire();
            }
        });
    }

    pub fn reset(timer: &Arc<Mutex<Self>>) {
        if let Ok(mut guard) = timer.lock() {
            guard.generation = guard.generation.wrapping_add(1);
        }
    }
}

fn make_sender(
    crabbox: &Arc<Mutex<Crabbox>>,
    cmd: Command,
    label: &'static str,
) -> impl Fn() + Send + Sync + 'static {
    let crabbox = Arc::clone(crabbox);
    move || {
        debug!("{label}");
        let sender = crabbox.lock().ok().map(|c| c.sender());
        if let Some(err) = sender.and_then(|s| s.blocking_send(cmd.clone()).err()) {
            error!("Failed to send {label} command from GPIO interrupt: {err}");
        }
    }
}

impl Drop for GpioController {
    fn drop(&mut self) {
        info!("GPIO control stopped");
    }
}
