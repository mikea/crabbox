#![warn(clippy::pedantic)]

use std::{fs, net::SocketAddr, path::PathBuf, process::ExitCode, sync::Arc};

use axum::{Router, response::Html, routing::get};
use clap::{Args, Parser, Subcommand};
use rand::{seq::SliceRandom, thread_rng};
use tokio::net::{TcpListener, UnixListener};

mod config;
mod crabbox;

use config::Config;
use crabbox::Crabbox;

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
    tracks.shuffle(&mut thread_rng());

    for track in &tracks {
        println!("{}", track.display());
    }

    if let Some(socket_path) = config
        .server
        .socket
        .as_deref()
        .filter(|path| !path.as_os_str().is_empty())
    {
        println!("Starting control socket at {}", socket_path.display());
        let path = socket_path.to_owned();
        let crabbox_clone = Arc::clone(&crabbox);
        tokio::spawn(async move {
            if let Err(err) = serve_control_socket(path, crabbox_clone).await {
                eprintln!("Control socket failed: {err}");
            }
        });
    }

    let web_addr: SocketAddr = config.server.web.parse()?;
    println!("Starting web control interface at http://{web_addr}");
    serve_web(web_addr, Arc::clone(&crabbox)).await
}

async fn serve_control_socket(socket_path: PathBuf, crabbox: Arc<Crabbox>) -> AnyResult<()> {
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let crabbox = Arc::clone(&crabbox);
                tokio::spawn(async move {
                    let _ = handle_control_connection(stream, crabbox).await;
                });
            }
            Err(err) => eprintln!("Control socket accept error: {err}"),
        }
    }
}

async fn handle_control_connection(
    _stream: tokio::net::UnixStream,
    _crabbox: Arc<Crabbox>,
) -> AnyResult<()> {
    // TODO: parse commands and interact with Crabbox
    Ok(())
}

async fn serve_web(addr: SocketAddr, _crabbox: Arc<Crabbox>) -> AnyResult<()> {
    let app = Router::new().route("/", get(index));
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html("Hello from Crabbox")
}
