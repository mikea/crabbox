#![warn(clippy::pedantic)]

use std::{net::SocketAddr, path::PathBuf, process::ExitCode, sync::Arc};

use clap::{Args, Parser, Subcommand};
use rand::{rng, seq::SliceRandom};

mod config;
mod crabbox;
mod pipe;
mod web;

use config::Config;
use crabbox::Crabbox;
use pipe::serve_control_pipe;
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
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Server(args) => run_server(&args).await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run_server(args: &ServerArgs) -> AnyResult<()> {
    let config = Config::load(&args.config)?;

    for entry in &config.music {
        println!("Music directory: {}", entry.dir.display());
    }

    let crabbox = Arc::new(Crabbox::new(&config));

    let mut tracks = crabbox.library.clone();
    tracks.shuffle(&mut rng());

    for track in &tracks {
        println!("{}", track.display());
    }

    if let Some(pipe_path) = config
        .server
        .pipe
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
    {
        println!("Starting control pipe at {}", pipe_path.display());
        let path = pipe_path.to_owned();
        let crabbox_clone = Arc::clone(&crabbox);
        tokio::spawn(async move {
            if let Err(err) = serve_control_pipe(path, crabbox_clone).await {
                eprintln!("Control pipe failed: {err}");
            }
        });
    }

    let web_addr: SocketAddr = config.server.web.parse()?;
    println!("Starting web control interface at http://{web_addr}");
    serve_web(web_addr, Arc::clone(&crabbox)).await
}
