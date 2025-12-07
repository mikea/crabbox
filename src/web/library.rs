use axum::{extract::State, response::Html};
use serde::Serialize;

use super::AppState;

pub(super) async fn library_page(State(state): State<AppState>) -> Html<String> {
    let library = state
        .crabbox
        .lock()
        .map(|c| c.library.list_tracks(None))
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.display().to_string())
        .collect();

    state.render("library.html", LibraryContext { library })
}

#[derive(Serialize)]
struct LibraryContext {
    library: Vec<String>,
}
