#![cfg(feature = "rpi")]

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rppal::gpio::{Gpio, InputPin, Trigger};
use tracing::{debug, error, info};

use crate::{
    config::GpioConfig,
    crabbox::{Command, Crabbox},
};

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
        let last_trigger = Arc::new(Mutex::new(Instant::now()));

        pin.set_async_interrupt(Trigger::FallingEdge, {
            let last_trigger = Arc::clone(&last_trigger);
            move |_level| {
                let now = Instant::now();
                let mut last = match last_trigger.lock() {
                    Ok(lock) => lock,
                    Err(err) => {
                        error!("Failed to lock debounce state: {err}");
                        return;
                    }
                };

                if now.duration_since(*last) < debounce {
                    return;
                }

                *last = now;
                on_press();
            }
        })?;

        Ok(Self { _pin: pin })
    }
}

pub struct GpioController {
    _play_button: Button,
}

impl GpioController {
    pub fn new(
        config: &GpioConfig,
        crabbox: Arc<Crabbox>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let gpio = Gpio::new()?;
        let debounce_duration = Duration::from_millis(config.debounce_ms);

        let play_button = Button::new(&gpio, config.play, debounce_duration, move || {
            debug!("PlayPause");
            if let Err(err) = crabbox.sender().blocking_send(Command::PlayPause) {
                error!("Failed to send command from GPIO interrupt: {err}");
            }
        })?;

        info!("GPIO control enabled (play/pause pin {})", config.play);

        Ok(Self {
            _play_button: play_button,
        })
    }
}

impl Drop for GpioController {
    fn drop(&mut self) {
        info!("GPIO control stopped");
    }
}
