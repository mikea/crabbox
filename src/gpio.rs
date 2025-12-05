#![cfg(feature = "rpi")]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
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

        pin.set_async_interrupt(Trigger::FallingEdge, Some(debounce), move |_level| {
            on_press();
        })?;

        Ok(Self { _pin: pin })
    }
}

pub struct GpioController {
    _play_button: Button,
    _next_button: Option<Button>,
    _prev_button: Option<Button>,
}

impl GpioController {
    pub fn new(
        config: &GpioConfig,
        crabbox: Arc<Mutex<Crabbox>>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let gpio = Gpio::new()?;
        let debounce_duration = Duration::from_millis(config.debounce_ms);

        let play_button = Button::new(&gpio, config.play, debounce_duration, {
            let crabbox = Arc::clone(&crabbox);
            move || {
                debug!("PlayPause");
                let sender = crabbox.lock().ok().map(|c| c.sender());
                sender
                    .and_then(|s| s.blocking_send(Command::PlayPause { filter: None }).err())
                    .into_iter()
                    .for_each(|err| error!("Failed to send command from GPIO interrupt: {err}"));
            }
        })?;

        let next_button = config
            .next
            .map(|pin| {
                Button::new(&gpio, pin, debounce_duration, {
                    let crabbox = Arc::clone(&crabbox);
                    move || {
                        debug!("Next");
                        let sender = crabbox.lock().ok().map(|c| c.sender());
                        sender
                            .and_then(|s| s.blocking_send(Command::Next).err())
                            .into_iter()
                            .for_each(|err| {
                                error!("Failed to send NEXT command from GPIO interrupt: {err}")
                            });
                    }
                })
            })
            .transpose()?;

        let prev_button = config
            .prev
            .map(|pin| {
                Button::new(&gpio, pin, debounce_duration, {
                    let crabbox = Arc::clone(&crabbox);
                    move || {
                        debug!("Prev");
                        let sender = crabbox.lock().ok().map(|c| c.sender());
                        sender
                            .and_then(|s| s.blocking_send(Command::Prev).err())
                            .into_iter()
                            .for_each(|err| {
                                error!("Failed to send PREV command from GPIO interrupt: {err}")
                            });
                    }
                })
            })
            .transpose()?;

        info!("GPIO control enabled (play/pause pin {})", config.play);
        if let Some(pin) = config.next {
            info!("GPIO control enabled (next pin {})", pin);
        }
        if let Some(pin) = config.prev {
            info!("GPIO control enabled (prev pin {})", pin);
        }

        Ok(Self {
            _play_button: play_button,
            _next_button: next_button,
            _prev_button: prev_button,
        })
    }
}

impl Drop for GpioController {
    fn drop(&mut self) {
        info!("GPIO control stopped");
    }
}
