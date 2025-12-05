#![warn(clippy::pedantic)]

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    thread,
};

use clap::{Args, Parser, Subcommand};
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

mod config;
mod crabbox;
mod glob;
mod pipe;
mod player;
mod web;

#[cfg(feature = "rpi")]
mod gpio;
#[cfg(feature = "rpi")]
mod rfid;

use config::Config;
use crabbox::Crabbox;
#[cfg(feature = "rpi")]
use gpio::GpioController;
use pipe::serve_control_pipe;
use player::play_blocking;
#[cfg(feature = "rpi")]
use rfid::Reader;
use web::serve_web;

type AnyResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Parser)]
#[command(version, about = "Crabbox command line interface")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Server(ServerArgs),
}

#[derive(Args)]
struct ServerArgs {
    /// Path to the TOML configuration file
    config: PathBuf,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    init_tracing();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Server(args) => run_server(&args).await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run_server(args: &ServerArgs) -> AnyResult<()> {
    let config = Config::load(&args.config)?;

    if let Some(startup_sound) = config.server.startup_sound.as_ref() {
        play_startup_sound(startup_sound.as_path(), config.default_volume);
    }

    for entry in &config.music {
        info!("Music directory: {}", entry.dir.display());
    }

    let crabbox = Crabbox::new(&config);

    if let Some(pipe_path) = config
        .server
        .pipe
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
    {
        info!("Starting control pipe at {}", pipe_path.display());
        let path = pipe_path.to_owned();
        let crabbox_clone = Arc::clone(&crabbox);
        tokio::spawn(async move {
            if let Err(err) = serve_control_pipe(path, crabbox_clone).await {
                error!("Control pipe failed: {err}");
            }
        });
    }

    #[cfg(feature = "rpi")]
    let _gpio_controller = if let Some(gpio_cfg) = config.gpio.as_ref() {
        Some(GpioController::new(gpio_cfg, Arc::clone(&crabbox))?)
    } else {
        None
    };
    #[cfg(feature = "rpi")]
    let _rfid_reader = if let Some(rfid_cfg) = config.rfid.as_ref() {
        Some(Reader::new(rfid_cfg)?)
    } else {
        None
    };

    let web_addr: SocketAddr = config.server.web.parse()?;
    info!("Starting web control interface at http://{web_addr}");
    serve_web(web_addr, Arc::clone(&crabbox)).await
}

fn init_tracing() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .with_file(true)
        .with_line_number(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("failed to set global tracing subscriber");
}

fn play_startup_sound(startup_sound: &Path, default_volume: f32) {
    let startup_sound = startup_sound.to_path_buf();
    let handle = thread::spawn(
        move || match play_blocking(&startup_sound, default_volume) {
            Ok(()) => info!("Played startup sound from {}", startup_sound.display()),
            Err(err) => error!(
                "Failed to play startup sound {}: {err}",
                startup_sound.display()
            ),
        },
    );

    let _ = handle;
}
