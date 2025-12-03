#![warn(clippy::pedantic)]

use std::{path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand};

mod config;

use config::Config;

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

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Server(args) => run_server(&args),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_server(args: &ServerArgs) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(&args.config)?;

    for entry in &config.music {
        println!("Music directory: {}", entry.dir.display());
    }

    todo!()
}
