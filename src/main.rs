#![warn(clippy::pedantic)]

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    thread,
};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

mod commands;
mod config;
mod crabbox;
mod glob;
mod pipe;
mod player;
mod state;
mod tag;
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

#[derive(Clone, Copy, Serialize)]
pub(crate) struct BuildInfo {
    version: &'static str,
    profile: &'static str,
    target: &'static str,
    commit: &'static str,
    dirty: &'static str,
    rustc: &'static str,
    built_at: &'static str,
}

pub(crate) const BUILD_INFO: BuildInfo = BuildInfo {
    version: env!("CARGO_PKG_VERSION"),
    profile: match (option_env!("BUILD_PROFILE"), option_env!("PROFILE")) {
        (Some(value), _) | (None, Some(value)) => value,
        (None, None) => "unknown",
    },
    target: match (option_env!("BUILD_TARGET"), option_env!("TARGET")) {
        (Some(value), _) | (None, Some(value)) => value,
        (None, None) => "unknown",
    },
    commit: match option_env!("GIT_COMMIT") {
        Some(value) => value,
        None => "unknown",
    },
    dirty: match option_env!("GIT_DIRTY") {
        Some(value) => value,
        None => "unknown",
    },
    rustc: match option_env!("RUSTC_VERSION") {
        Some(value) => value,
        None => "unknown",
    },
    built_at: match option_env!("BUILD_TIMESTAMP") {
        Some(value) => value,
        None => "unknown",
    },
};

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
    log_build_info();

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
    let command_sender = crabbox.lock().expect("crabbox lock poisoned").sender();

    if let Some(pipe_path) = config
        .server
        .pipe
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
    {
        info!("Starting control pipe at {}", pipe_path.display());
        let path = pipe_path.to_owned();
        let sender = command_sender.clone();
        tokio::spawn(async move {
            if let Err(err) = serve_control_pipe(path, sender).await {
                error!("Control pipe failed: {err}");
            }
        });
    }

    #[cfg(feature = "rpi")]
    let _gpio_controller = if let Some(gpio_cfg) = config.gpio.as_ref() {
        Some(GpioController::new(gpio_cfg, &command_sender)?)
    } else {
        None
    };
    #[cfg(feature = "rpi")]
    let _rfid_reader = if let Some(rfid_cfg) = config.rfid.as_ref() {
        Some(Reader::new(rfid_cfg, command_sender)?)
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

fn log_build_info() {
    info!(
        version = BUILD_INFO.version,
        profile = BUILD_INFO.profile,
        target = BUILD_INFO.target,
        commit = BUILD_INFO.commit,
        dirty = BUILD_INFO.dirty,
        rustc = BUILD_INFO.rustc,
        built_at = BUILD_INFO.built_at,
        "Crabbox build metadata",
    );
}

fn play_startup_sound(startup_sound: &Path, default_volume: f32) {
    let startup_sound = startup_sound.to_path_buf();
    let handle = thread::spawn(move || {
        info!("Playing startup sound from {}", startup_sound.display());
        match play_blocking(&startup_sound, default_volume) {
            Ok(()) => {}
            Err(err) => error!(
                "Failed to play startup sound {}: {err}",
                startup_sound.display()
            ),
        }
    });

    let _ = handle;
}
