use std::{
    fmt::Write as _,
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
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing::warn;

use crate::{AnyResult, commands::Command, crabbox::Crabbox};

mod upload;

use upload::{upload_files, upload_form};

pub async fn serve_web(addr: SocketAddr, crabbox: Arc<Mutex<Crabbox>>) -> AnyResult<()> {
    let state = AppState {
        crabbox,
        last_uploaded: Arc::new(Mutex::new(Vec::new())),
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

    let (current, queue, queue_position, library, last_tag) = match snapshot {
        Some(snapshot) => (
            snapshot.current.map_or_else(
                || "Nothing playing".to_string(),
                |p| p.display().to_string(),
            ),
            snapshot.queue,
            snapshot.queue_position,
            snapshot.library,
            snapshot.last_tag,
        ),
        None => (
            "Unavailable".to_string(),
            Vec::new(),
            None,
            Vec::new(),
            None,
        ),
    };

    let queue_html = if queue.is_empty() {
        "<p>Queue is empty</p>".to_string()
    } else {
        let mut html = String::from("<ol class=\"queue\">");
        for (idx, track) in queue.iter().enumerate() {
            let display = escape_html(&track.display().to_string());
            if queue_position == Some(idx) {
                let _ = write!(html, "<li><strong>{display}</strong></li>");
            } else {
                let _ = write!(html, "<li>{display}</li>");
            }
        }
        html.push_str("</ol>");
        html
    };

    let library_html = if library.is_empty() {
        "<p>No tracks found.</p>".to_string()
    } else {
        let mut html = String::from("<ul class=\"library\">");
        for track in library {
            let display = escape_html(&track.display().to_string());
            let _ = write!(html, "<li>{display}</li>");
        }
        html.push_str("</ul>");
        html
    };

    let current_display = escape_html(&current);
    let last_tag_display =
        last_tag.map_or_else(|| "None".to_string(), |tag| escape_html(&tag.to_string()));

    let page = format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Crabbox</title>
    <style>
      body {{ font-family: Arial, sans-serif; padding: 24px; background: #f4f4f7; color: #222; }}
      h1 {{ margin-top: 0; }}
      .controls {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(120px, 1fr)); gap: 8px; max-width: 720px; margin: 16px 0; }}
      form {{ margin: 0; }}
      button {{ width: 100%; padding: 10px; border: none; background: #0f62fe; color: #fff; border-radius: 6px; cursor: pointer; font-size: 14px; }}
      button:hover {{ background: #0b4cc0; }}
      .secondary button {{ background: #6f6f6f; }}
      .secondary button:hover {{ background: #525252; }}
      .danger button {{ background: #da1e28; }}
      .danger button:hover {{ background: #a2191f; }}
      .queue, .library {{ padding-left: 20px; }}
      .command {{ margin: 16px 0; max-width: 480px; display: flex; gap: 8px; }}
      .command input {{ flex: 1; padding: 10px; border: 1px solid #ccc; border-radius: 6px; }}
      .section {{ background: #fff; padding: 16px; border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,0.08); margin-bottom: 16px; }}
      .muted {{ color: #666; }}
      .link-button {{ display: inline-block; padding: 10px 14px; background: #0f62fe; color: #fff; border-radius: 6px; text-decoration: none; }}
      .link-button:hover {{ background: #0b4cc0; }}
    </style>
  </head>
  <body>
    <h1>Crabbox</h1>
    <div class="section">
      <p>Current track: <span class="muted">{current_display}</span></p>
      <p>Last tag: <span class="muted">{last_tag_display}</span></p>
      <div class="controls">
        <form method="post" action="/play">
          <button type="submit">Play</button>
        </form>
        <form method="post" action="/playpause">
          <button type="submit">Play / Pause</button>
        </form>
        <form method="post" action="/stop" class="secondary">
          <button type="submit">Stop</button>
        </form>
        <form method="post" action="/prev">
          <button type="submit">Previous</button>
        </form>
        <form method="post" action="/next">
          <button type="submit">Next</button>
        </form>
        <form method="post" action="/volume-down" class="secondary">
          <button type="submit">Volume Down</button>
        </form>
        <form method="post" action="/volume-up" class="secondary">
          <button type="submit">Volume Up</button>
        </form>
        <form method="post" action="/shutdown" class="danger">
          <button type="submit">Shutdown</button>
        </form>
      </div>
    </div>

    <div class="section">
      <form method="post" action="/command" class="command">
        <input type="text" name="command" placeholder="Enter command e.g. PLAY chill/*" />
        <button type="submit">Run</button>
      </form>
    </div>

    <div class="section">
      <a class="link-button" href="/upload">Upload files or folders</a>
    </div>

    <div class="section">
      <h2>Current queue</h2>
      {queue_html}
    </div>

    <div class="section">
      <h2>Library</h2>
      {library_html}
    </div>
  </body>
</html>"#
    );

    Html(page)
}

#[derive(Clone)]
pub(super) struct AppState {
    pub(super) crabbox: Arc<Mutex<Crabbox>>,
    pub(super) last_uploaded: Arc<Mutex<Vec<PathBuf>>>,
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

async fn send_command(state: &AppState, command: Command) {
    let sender = state.crabbox.lock().ok().map(|c| c.sender());

    if let Some(sender) = sender {
        let _ = sender.send(command).await;
    }
}

pub(super) fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
