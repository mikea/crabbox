use std::{
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    extract::{Form, Path, Query, State},
    response::{Html, Json, Redirect},
    routing::{get, post},
};
use minijinja::{Environment, value::Value};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, time::sleep};
use tracing::{info, warn};

use crate::{AnyResult, BUILD_INFO, BuildInfo, commands::Command, crabbox::Crabbox, tag::TagId};

mod edit_tag;
mod index;
mod library;
mod upload;

use edit_tag::{assign_tag, edit_tag};
use index::index;
use library::library_page;
use upload::{upload_files, upload_form};

pub async fn serve_web(addr: SocketAddr, crabbox: Arc<Mutex<Crabbox>>) -> AnyResult<()> {
    let templates = build_templates(BUILD_INFO)?;

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
        .route("/clear-queue", post(clear_queue))
        .route("/activate_tag/{id}", post(activate_tag))
        .route("/shutdown", post(shutdown))
        .route("/command", post(run_command))
        .route("/list_files", get(list_files))
        .route("/edit_tag/{id}", get(edit_tag))
        .route("/assign_tag", post(assign_tag))
        .route("/library", get(library_page))
        .route("/upload", get(upload_form))
        .route("/do_upload", post(upload_files))
        .with_state(state);
    let listener = bind_with_retry(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn bind_with_retry(addr: SocketAddr) -> AnyResult<TcpListener> {
    let mut attempts = 0usize;
    loop {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                if attempts > 0 {
                    info!(attempts, "Web server successfully bound after retrying");
                }
                return Ok(listener);
            }
            Err(err) => {
                attempts += 1;
                warn!(
                    attempts,
                    %addr,
                    "Failed to bind web address: {err}. Retrying in 2s."
                );
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
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

async fn clear_queue(State(state): State<AppState>) -> Redirect {
    send_command(&state, Command::ClearQueue).await;
    Redirect::to("/")
}

async fn activate_tag(Path(id): Path<String>, State(state): State<AppState>) -> Redirect {
    match TagId::from_str(&id) {
        Ok(tag_id) => send_command(&state, Command::Tag { id: tag_id }).await,
        Err(err) => warn!(%id, "Invalid tag id: {err}"),
    }

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

#[derive(Deserialize)]
struct ListFilesQuery {
    filter: Option<String>,
}

async fn list_files(
    Query(query): Query<ListFilesQuery>,
    State(state): State<AppState>,
) -> Json<Vec<String>> {
    let files = state
        .crabbox
        .lock()
        .map(|c| c.library.list_tracks(query.filter))
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.display().to_string())
        .collect();

    Json(files)
}

fn build_templates(build_info: BuildInfo) -> AnyResult<Environment<'static>> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(minijinja::default_auto_escape_callback);
    env.add_global("build_info", Value::from_serialize(build_info));
    env.add_template(
        "index.html",
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/index.html")),
    )?;
    env.add_template(
        "footer.html",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/templates/footer.html"
        )),
    )?;
    env.add_template(
        "library.html",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/templates/library.html"
        )),
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
