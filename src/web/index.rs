use axum::{extract::State, response::Html};
use serde::Serialize;

use super::AppState;

#[allow(clippy::too_many_lines)]
pub(super) async fn index(State(state): State<AppState>) -> Html<String> {
    let snapshot = state.crabbox.lock().ok().map(|c| c.snapshot());

    let (current, queue, queue_position, last_tag, last_tag_command) = match snapshot {
        Some(ref snapshot) => (
            snapshot.current.as_ref().map_or_else(
                || "Nothing playing".to_string(),
                |p| p.display().to_string(),
            ),
            snapshot.queue.clone(),
            snapshot.queue_position,
            snapshot.last_tag,
            snapshot.last_tag_command.clone(),
        ),
        None => ("Unavailable".to_string(), Vec::new(), None, None, None),
    };

    let queue_items = queue
        .into_iter()
        .enumerate()
        .map(|(idx, track)| QueueItem {
            name: track.display().to_string(),
            is_current: queue_position == Some(idx),
        })
        .collect();

    let last_tag = last_tag.map(|tag| TagAssignmentContext {
        id: tag.to_string(),
        command: last_tag_command.map(|command| command.to_string()),
    });

    let tags = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .tags
                .iter()
                .map(|(id, command)| TagAssignmentContext {
                    id: id.to_string(),
                    command: Some(command.to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    state.render(
        "index.html",
        IndexContext {
            current,
            queue: queue_items,
            last_tag,
            tags,
        },
    )
}

#[derive(Serialize)]
struct QueueItem {
    name: String,
    is_current: bool,
}

#[derive(Serialize)]
struct TagAssignmentContext {
    id: String,
    command: Option<String>,
}

#[derive(Serialize)]
struct IndexContext {
    current: String,
    queue: Vec<QueueItem>,
    last_tag: Option<TagAssignmentContext>,
    tags: Vec<TagAssignmentContext>,
}
