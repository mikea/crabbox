use std::{
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{Form, State},
    response::{Html, Redirect},
    routing::{get, post},
};
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::warn;

use crate::{AnyResult, commands::Command, crabbox::Crabbox};

mod edit_tag;
mod upload;

use edit_tag::{assign_tag, edit_tag};
use upload::{upload_files, upload_form};

pub async fn serve_web(addr: SocketAddr, crabbox: Arc<Mutex<Crabbox>>) -> AnyResult<()> {
    let templates = build_templates()?;

    let state = AppState {
        crabbox,
        last_uploaded: Arc::new(Mutex::new(Vec::new())),
        templates,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/play", post(play))
        .route("/playpause", post(play_pause))
        .route("/stop", post(stop))
        .route("/next", post(next))
        .route("/prev", post(prev))
        .route("/volume-up", post(volume_up))
        .route("/volume-down", post(volume_down))
        .route("/shutdown", post(shutdown))
        .route("/command", post(run_command))
        .route("/edit_tag", get(edit_tag))
        .route("/assign_tag", post(assign_tag))
        .route("/upload", get(upload_form))
        .route("/do_upload", post(upload_files))
        .with_state(state);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn index(State(state): State<AppState>) -> Html<String> {
    let snapshot = state.crabbox.lock().ok().map(|c| c.snapshot());

    let (current, queue, queue_position, library, last_tag, last_tag_command) = match snapshot {
        Some(snapshot) => (
            snapshot.current.map_or_else(
                || "Nothing playing".to_string(),
                |p| p.display().to_string(),
            ),
            snapshot.queue,
            snapshot.queue_position,
            snapshot.library,
            snapshot.last_tag,
            snapshot.last_tag_command,
        ),
        None => (
            "Unavailable".to_string(),
            Vec::new(),
            None,
            Vec::new(),
            None,
            None,
        ),
    };

    let queue_items = queue
        .into_iter()
        .enumerate()
        .map(|(idx, track)| QueueItem {
            name: track.display().to_string(),
            is_current: queue_position == Some(idx),
        })
        .collect();

    let library_items = library
        .into_iter()
        .map(|track| track.display().to_string())
        .collect();

    let last_tag = last_tag.map(|tag| LastTagContext {
        id: tag.to_string(),
        command: last_tag_command.map(|command| command.to_string()),
    });

    state.render(
        "index.html",
        IndexContext {
            current,
            queue: queue_items,
            library: library_items,
            last_tag,
        },
    )
}

#[derive(Clone)]
pub(super) struct AppState {
    pub(super) crabbox: Arc<Mutex<Crabbox>>,
    pub(super) last_uploaded: Arc<Mutex<Vec<PathBuf>>>,
    templates: Environment<'static>,
}

impl AppState {
    pub(super) fn render<C: Serialize>(&self, name: &str, context: C) -> Html<String> {
        let rendered = self
            .templates
            .get_template(name)
            .and_then(|template| template.render(context))
            .unwrap_or_else(|err| format!("Template error: {err}"));

        Html(rendered)
    }
}

async fn play(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::Play { filter: None }).await;
    Redirect::to("/")
}

async fn stop(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::Stop).await;
    Redirect::to("/")
}

async fn play_pause(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::PlayPause { filter: None }).await;
    Redirect::to("/")
}

async fn next(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::Next).await;
    Redirect::to("/")
}

async fn prev(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::Prev).await;
    Redirect::to("/")
}

async fn volume_up(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::VolumeUp).await;
    Redirect::to("/")
}

async fn volume_down(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::VolumeDown).await;
    Redirect::to("/")
}

async fn shutdown(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::Shutdown).await;
    Redirect::to("/")
}

#[derive(Deserialize)]
struct CommandForm {
    command: String,
}

async fn run_command(State(state): State<AppState>, Form(form): Form<CommandForm>) -> Redirect {
    match Command::from_str(&form.command) {
        Ok(cmd) => send_command(&state, cmd).await,
        Err(err) => warn!(command = form.command, "Invalid command from web: {err}"),
    }
    Redirect::to("/")
}

pub(super) async fn send_command(state: &AppState, command: Command) {
    let sender = state.crabbox.lock().ok().map(|c| c.sender());

    if let Some(sender) = sender {
        let _ = sender.send(command).await;
    }
}

#[derive(Serialize)]
struct QueueItem {
    name: String,
    is_current: bool,
}

#[derive(Serialize)]
struct LastTagContext {
    id: String,
    command: Option<String>,
}

#[derive(Serialize)]
struct IndexContext {
    current: String,
    queue: Vec<QueueItem>,
    library: Vec<String>,
    last_tag: Option<LastTagContext>,
}

fn build_templates() -> AnyResult<Environment<'static>> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(minijinja::default_auto_escape_callback);
    env.add_template(
        "index.html",
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/index.html")),
    )?;
    env.add_template(
        "upload.html",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/templates/upload.html"
        )),
    )?;
    env.add_template(
        "edit_tag.html",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/templates/edit_tag.html"
        )),
    )?;
    Ok(env)
}
