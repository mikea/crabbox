use std::{
    fmt::Write as _,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{Form, Multipart, State},
    http::StatusCode,
    response::{Html, Redirect},
    routing::{get, post},
};
use serde::Deserialize;
use tokio::{fs, io::AsyncWriteExt, net::TcpListener};
use tracing::warn;

use crate::{AnyResult, commands::Command, crabbox::Crabbox};

pub async fn serve_web(addr: SocketAddr, crabbox: Arc<Mutex<Crabbox>>) -> AnyResult<()> {
    let state = AppState { crabbox };

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
        .route("/upload", post(upload_files))
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

async fn upload_form(State(state): State<AppState>) -> Html<String> {
    let directories = state
        .crabbox
        .lock()
        .map(|crabbox| crabbox.music_directories())
        .unwrap_or_default();

    let destination_html = match directories.len() {
        0 => "<p class=\"muted\">No music directories configured.</p>".to_string(),
        1 => {
            let dir = directories.first().expect("directory exists");
            let escaped = escape_html(&dir.display().to_string());
            format!(
                "<p>Uploading to <strong>{escaped}</strong></p><input type=\"hidden\" name=\"target_dir\" value=\"{escaped}\" />"
            )
        }
        _ => {
            let mut options = String::new();
            for dir in &directories {
                let escaped = escape_html(&dir.display().to_string());
                let _ = write!(options, "<option value=\"{escaped}\">{escaped}</option>");
            }

            format!(
                "<label for=\"target_dir\">Select destination</label><br /><select id=\"target_dir\" name=\"target_dir\">{options}</select>"
            )
        }
    };

    let page = format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Upload | Crabbox</title>
    <style>
      body {{ font-family: Arial, sans-serif; padding: 24px; background: #f4f4f7; color: #222; }}
      h1 {{ margin-top: 0; }}
      form {{ margin: 0; }}
      .section {{ background: #fff; padding: 16px; border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,0.08); margin-bottom: 16px; max-width: 720px; }}
      button {{ padding: 10px 14px; border: none; background: #0f62fe; color: #fff; border-radius: 6px; cursor: pointer; font-size: 14px; }}
      button:hover {{ background: #0b4cc0; }}
      .muted {{ color: #666; }}
      .back {{ text-decoration: none; color: #0f62fe; }}
      .field {{ margin: 12px 0; }}
    </style>
  </head>
  <body>
    <h1>Upload music</h1>
    <div class="section">
      <form method="post" action="/upload" enctype="multipart/form-data">
        <div class="field">{destination_html}</div>
        <div class="field">
          <label for="files">Choose files or entire folders</label><br />
          <input type="file" id="files" name="files" multiple webkitdirectory directory />
        </div>
        <p class="muted">Supports uploading individual tracks or selecting a whole directory of music.</p>
        <button type="submit">Upload</button>
      </form>
    </div>

    <p><a class="back" href="/">&larr; Back to player</a></p>
  </body>
</html>"#
    );

    Html(page)
}

#[derive(Clone)]
struct AppState {
    crabbox: Arc<Mutex<Crabbox>>,
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

async fn upload_files(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Redirect, (StatusCode, String)> {
    let available_directories = state
        .crabbox
        .lock()
        .map(|crabbox| crabbox.music_directories())
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to access music directories".to_string(),
            )
        })?;

    let mut target_dir_value: Option<String> = None;
    let mut saved_files = 0usize;

    while let Some(field) = multipart.next_field().await.map_err(internal_error)? {
        let Some(name) = field.name().map(str::to_owned) else {
            continue;
        };

        if name == "target_dir" {
            target_dir_value = Some(field.text().await.map_err(internal_error)?);
            continue;
        }

        if name != "files" {
            continue;
        }

        let target_root = resolve_target_dir(&available_directories, target_dir_value.as_deref())
            .ok_or((
            StatusCode::BAD_REQUEST,
            "Invalid target directory".to_string(),
        ))?;

        let Some(filename) = field.file_name().map(ToString::to_string) else {
            continue;
        };

        let Some(relative_path) = sanitize_relative_path(&filename) else {
            continue;
        };

        let destination = target_root.join(relative_path);

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await.map_err(internal_error)?;
        }

        let mut file = fs::File::create(&destination)
            .await
            .map_err(internal_error)?;

        let mut field = field;
        while let Some(chunk) = field.chunk().await.map_err(internal_error)? {
            file.write_all(&chunk).await.map_err(internal_error)?;
        }

        saved_files += 1;
    }

    if saved_files == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "No files were uploaded".to_string(),
        ));
    }

    Ok(Redirect::to("/upload"))
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

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn sanitize_relative_path(filename: &str) -> Option<PathBuf> {
    let mut clean = PathBuf::new();

    for component in Path::new(filename).components() {
        if let Component::Normal(part) = component {
            clean.push(part);
        }
    }

    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn resolve_target_dir(directories: &[PathBuf], selected: Option<&str>) -> Option<PathBuf> {
    let selected = selected?;
    directories
        .iter()
        .find(|dir| dir.to_string_lossy() == selected)
        .cloned()
}

fn internal_error<E>(err: E) -> (StatusCode, String)
where
    E: std::error::Error + Send + Sync + 'static,
{
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
