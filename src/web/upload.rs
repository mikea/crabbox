use std::path::{Component, Path, PathBuf};

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{Html, Redirect},
};
use serde::Serialize;
use tokio::{fs, io::AsyncWriteExt};

use super::AppState;

pub async fn upload_form(State(state): State<AppState>) -> Html<String> {
    let destinations = state
        .crabbox
        .lock()
        .map(|crabbox| crabbox.music_directories())
        .unwrap_or_default()
        .into_iter()
        .map(|dir| dir.display().to_string())
        .collect();

    let last_uploaded = state
        .last_uploaded
        .lock()
        .map(|paths| {
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    state.render(
        "upload.html",
        UploadTemplateContext {
            destinations,
            last_uploaded,
        },
    )
}

pub async fn upload_files(
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
    let mut uploaded_paths: Vec<PathBuf> = Vec::new();

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

        let destination = target_root.join(&relative_path);

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
        uploaded_paths.push(relative_path);
    }

    if saved_files == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "No files were uploaded".to_string(),
        ));
    }

    if let Ok(mut last_uploaded) = state.last_uploaded.lock() {
        *last_uploaded = uploaded_paths;
    }

    Ok(Redirect::to("/upload"))
}

#[derive(Serialize)]
struct UploadTemplateContext {
    destinations: Vec<String>,
    last_uploaded: Vec<String>,
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
