#![cfg(feature = "rpi")]

use std::{
    error::Error,
    sync::{Arc, Mutex, mpsc as std_mpsc},
    thread,
    time::Duration,
};

use rppal::{
    gpio::{Gpio, InputPin, OutputPin, Trigger},
    spi::{Bus, Mode, SlaveSelect, Spi},
};
use tracing::{debug, error, info};

use tokio::sync::mpsc;

use crate::{commands::Command, config::RfidConfig, tag::TagId};

const PCD_TRANSCEIVE: u8 = 0x0C;
const PCD_RESETPHASE: u8 = 0x0F;

const PICC_REQIDL: u8 = 0x26;
const PICC_ANTICOLL: u8 = 0x93;

const COMMAND_REG: u8 = 0x01;
const COM_IRQ_REG: u8 = 0x04;
const ERROR_REG: u8 = 0x06;
const FIFO_DATA_REG: u8 = 0x09;
const FIFO_LEVEL_REG: u8 = 0x0A;
const CONTROL_REG: u8 = 0x0C;
const BIT_FRAMING_REG: u8 = 0x0D;
const MODE_REG: u8 = 0x11;
const TX_CONTROL_REG: u8 = 0x14;
const TX_AUTO_REG: u8 = 0x15;
const T_MODE_REG: u8 = 0x2A;
const T_PRESCALER_REG: u8 = 0x2B;
const T_RELOAD_REG_H: u8 = 0x2C;
const T_RELOAD_REG_L: u8 = 0x2D;

pub struct Reader {
    _irq_pin: InputPin,
    _reset_pin: Option<OutputPin>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Reader {
    pub fn new(
        config: &RfidConfig,
        command_tx: mpsc::Sender<Command>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let (bus, slave_select) = resolve_spi(config)?;
        let spi = Spi::new(bus, slave_select, 1_000_000, Mode::Mode0)?;
        let spi = Arc::new(Mutex::new(spi));

        let gpio = Gpio::new()?;
        let mut irq_pin = gpio.get(config.irq)?.into_input_pullup();
        let reset_pin = if let Some(pin) = config.reset {
            let mut pin = gpio.get(pin)?.into_output();
            pin.set_low();
            thread::sleep(Duration::from_millis(10));
            pin.set_high();
            Some(pin)
        } else {
            None
        };

        let (tx, rx) = std_mpsc::channel();
        let trigger_tx = tx.clone();

        irq_pin.set_async_interrupt(Trigger::FallingEdge, None, move |_level| {
            let _ = trigger_tx.send(());
        })?;

        // Kick off an initial poll in case the IRQ line is already low.
        let _ = tx.send(());

        let worker = thread::spawn({
            move || {
                let mut rc522 = Rc522::new(spi);
                if let Err(err) = rc522.init() {
                    error!("RFID init failed: {err}");
                    return;
                }

                loop {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(_) | Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                    }

                    match rc522.poll_for_tag() {
                        Ok(Some(uid)) => handle_tag(&uid, &command_tx),
                        Ok(None) => {}
                        Err(err) => error!("RFID poll failed: {err}"),
                    }
                }
            }
        });

        info!("RFID SPI initialized on {} {}", bus, slave_select);

        Ok(Self {
            _irq_pin: irq_pin,
            _reset_pin: reset_pin,
            worker: Some(worker),
        })
    }
}

fn handle_tag(uid: &[u8; 4], command_tx: &mpsc::Sender<Command>) {
    let tag_id = TagId::from_uid(*uid);
    info!("RFID tag detected UID {tag_id}");

    if let Err(err) = command_tx.blocking_send(Command::Tag { id: tag_id }) {
        error!("Failed to send RFID tag command {tag_id}: {err}");
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        info!("RFID reader stopped");
    }
}

struct Rc522 {
    spi: Arc<Mutex<Spi>>,
}

impl Rc522 {
    fn new(spi: Arc<Mutex<Spi>>) -> Self {
        Self { spi }
    }

    fn init(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.reset()?;
        self.write_reg(T_MODE_REG, 0x8D)?;
        self.write_reg(T_PRESCALER_REG, 0x3E)?;
        self.write_reg(T_RELOAD_REG_L, 30)?;
        self.write_reg(T_RELOAD_REG_H, 0)?;
        self.write_reg(TX_AUTO_REG, 0x40)?;
        self.write_reg(MODE_REG, 0x3D)?;
        self.antenna_on()?;
        Ok(())
    }

    fn poll_for_tag(&mut self) -> Result<Option<[u8; 4]>, Box<dyn Error + Send + Sync>> {
        if !self.check_for_card()? {
            return Ok(None);
        }

        self.anticollision()
    }

    fn check_for_card(&mut self) -> Result<bool, Box<dyn Error + Send + Sync>> {
        self.write_reg(BIT_FRAMING_REG, 0x07)?;
        let response = self.transceive(&[PICC_REQIDL])?;
        Ok(response.is_some())
    }

    fn anticollision(&mut self) -> Result<Option<[u8; 4]>, Box<dyn Error + Send + Sync>> {
        self.write_reg(BIT_FRAMING_REG, 0x00)?;
        let Some(back_data) = self.transceive(&[PICC_ANTICOLL, 0x20])? else {
            return Ok(None);
        };

        if back_data.len() < 5 {
            return Ok(None);
        }

        let checksum = back_data[..4].iter().fold(0u8, |acc, b| acc ^ b);
        if checksum != back_data[4] {
            return Err("UID checksum mismatch".into());
        }

        Ok(Some([
            back_data[0],
            back_data[1],
            back_data[2],
            back_data[3],
        ]))
    }

    fn transceive(&mut self, data: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn Error + Send + Sync>> {
        self.write_reg(COM_IRQ_REG, 0x7F)?;
        self.write_reg(FIFO_LEVEL_REG, 0x80)?;
        for byte in data {
            self.write_reg(FIFO_DATA_REG, *byte)?;
        }
        self.write_reg(COMMAND_REG, PCD_TRANSCEIVE)?;
        self.set_bit_mask(BIT_FRAMING_REG, 0x80)?;

        let mut countdown = 2_000;
        loop {
            let irq = self.read_reg(COM_IRQ_REG)?;
            if irq & 0x01 != 0 {
                // No response within internal timer; treat as a missed read without spamming logs.
                return Ok(None);
            }
            if irq & 0x30 != 0 {
                break;
            }
            countdown -= 1;
            if countdown == 0 {
                return Ok(None);
            }
        }

        self.clear_bit_mask(BIT_FRAMING_REG, 0x80)?;

        let error = self.read_reg(ERROR_REG)?;
        if error & 0x1B != 0 {
            debug!("RFID reported error bits: 0x{error:02X}");
            return Ok(None);
        }

        let fifo_level = self.read_reg(FIFO_LEVEL_REG)?;
        if fifo_level == 0 {
            return Ok(None);
        }

        let last_bits = self.read_reg(CONTROL_REG)? & 0x07;
        let _valid_bits = if last_bits != 0 {
            (fifo_level - 1) * 8 + last_bits
        } else {
            fifo_level * 8
        };

        let mut back_data = Vec::with_capacity(fifo_level as usize);
        for _ in 0..fifo_level {
            back_data.push(self.read_reg(FIFO_DATA_REG)?);
        }

        Ok(Some(back_data))
    }

    fn antenna_on(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let current = self.read_reg(TX_CONTROL_REG)?;
        if current & 0x03 != 0x03 {
            self.set_bit_mask(TX_CONTROL_REG, 0x03)?;
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.write_reg(COMMAND_REG, PCD_RESETPHASE)
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, Box<dyn Error + Send + Sync>> {
        let address = 0x80 | ((reg << 1) & 0x7E);
        let mut read_buffer = [0u8; 2];
        self.spi
            .lock()
            .map_err(|e| format!("SPI mutex poisoned: {e}"))?
            .transfer(&mut read_buffer, &[address, 0])?;
        Ok(read_buffer[1])
    }

    fn write_reg(&mut self, reg: u8, value: u8) -> Result<(), Box<dyn Error + Send + Sync>> {
        let address = (reg << 1) & 0x7E;
        self.spi
            .lock()
            .map_err(|e| format!("SPI mutex poisoned: {e}"))?
            .write(&[address, value])?;
        Ok(())
    }

    fn set_bit_mask(&mut self, reg: u8, mask: u8) -> Result<(), Box<dyn Error + Send + Sync>> {
        let current = self.read_reg(reg)?;
        self.write_reg(reg, current | mask)
    }

    fn clear_bit_mask(&mut self, reg: u8, mask: u8) -> Result<(), Box<dyn Error + Send + Sync>> {
        let current = self.read_reg(reg)?;
        self.write_reg(reg, current & !mask)
    }
}

fn resolve_spi(config: &RfidConfig) -> Result<(Bus, SlaveSelect), Box<dyn Error + Send + Sync>> {
    let bus = match config.bus {
        0 => Bus::Spi0,
        1 => Bus::Spi1,
        2 => Bus::Spi2,
        3 => Bus::Spi3,
        4 => Bus::Spi4,
        5 => Bus::Spi5,
        6 => Bus::Spi6,
        other => {
            return Err(format!(
                "Unsupported SPI bus {}. Supported buses are 0 through 6.",
                other
            )
            .into());
        }
    };

    Ok((bus, SlaveSelect::Ss0))
}
