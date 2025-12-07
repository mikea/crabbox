use std::{
    fmt::Write as _,
    path::{Component, Path, PathBuf},
};

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::Html,
};
use tokio::{fs, io::AsyncWriteExt};

use super::{AppState, escape_html};

pub async fn upload_form(State(state): State<AppState>) -> Html<String> {
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
                "<p>Uploading to <strong>{escaped}</strong></p><input type=\"hidden\" name=\"target_dir\" value=\"{escaped}\" />",
            )
        }
        _ => {
            let mut options = String::new();
            for dir in &directories {
                let escaped = escape_html(&dir.display().to_string());
                let _ = write!(options, "<option value=\"{escaped}\">{escaped}</option>");
            }

            format!(
                "<label for=\"target_dir\">Select destination</label><br /><select id=\"target_dir\" name=\"target_dir\">{options}</select>",
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

pub async fn upload_files(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Html<String>, (StatusCode, String)> {
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
    let mut target_root: Option<PathBuf> = None;
    let mut saved_files: Vec<PathBuf> = Vec::new();

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

        let root = if let Some(root) = &target_root {
            root.clone()
        } else {
            let resolved = resolve_target_dir(&available_directories, target_dir_value.as_deref())
                .ok_or((
                    StatusCode::BAD_REQUEST,
                    "Invalid target directory".to_string(),
                ))?;
            target_root = Some(resolved.clone());
            resolved
        };

        let Some(filename) = field.file_name().map(ToString::to_string) else {
            continue;
        };

        let Some(relative_path) = sanitize_relative_path(&filename) else {
            continue;
        };

        let destination = root.join(relative_path.clone());

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

        saved_files.push(destination);
    }

    if saved_files.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "No files were uploaded".to_string(),
        ));
    }

    let uploaded_to = target_root.as_ref().map_or_else(
        || "unknown location".to_string(),
        |path| path.display().to_string(),
    );

    let mut list_items = String::new();
    for file in &saved_files {
        let escaped = escape_html(&file.display().to_string());
        let _ = write!(list_items, "<li>{escaped}</li>");
    }

    let page = format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Upload complete | Crabbox</title>
    <style>
      body {{ font-family: Arial, sans-serif; padding: 24px; background: #f4f4f7; color: #222; }}
      h1 {{ margin-top: 0; }}
      .section {{ background: #fff; padding: 16px; border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,0.08); margin-bottom: 16px; max-width: 720px; }}
      .muted {{ color: #666; }}
      .back {{ text-decoration: none; color: #0f62fe; }}
    </style>
  </head>
  <body>
    <h1>Upload successful</h1>
    <div class="section">
      <p class="muted">Saved to {uploaded_to}.</p>
      <p>Uploaded files:</p>
      <ul>{list_items}</ul>
      <p><a class="back" href="/upload">Upload more</a></p>
      <p><a class="back" href="/">&larr; Back to player</a></p>
    </div>
  </body>
</html>"#
    );

    Ok(Html(page))
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
