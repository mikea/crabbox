use std::str::FromStr;

use axum::{
    extract::{Form, State},
    response::{Html, Redirect},
};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{commands::Command, tag::TagId};

use super::{AppState, send_command};

#[derive(Deserialize)]
pub(super) struct AssignTagForm {
    tag_id: String,
    command: Option<String>,
    action: String,
}

pub(super) async fn edit_tag(State(state): State<AppState>) -> Html<String> {
    let snapshot = state.crabbox.lock().ok().map(|c| c.snapshot());

    let context = snapshot.map_or(
        EditTagTemplateContext {
            available: false,
            last_tag: None,
        },
        |snapshot| {
            let last_tag = snapshot.last_tag.map(|tag| TagTemplateContext {
                id: tag.to_string(),
                command: snapshot.last_tag_command.map(|command| command.to_string()),
            });

            EditTagTemplateContext {
                available: true,
                last_tag,
            }
        },
    );

    state.render("edit_tag.html", context)
}

pub(super) async fn assign_tag(
    State(state): State<AppState>,
    Form(form): Form<AssignTagForm>,
) -> Redirect {
    match TagId::from_str(&form.tag_id) {
        Ok(tag_id) => {
            let command_text = match form.action.as_str() {
                "delete" => None,
                _ => form
                    .command
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
            };
            send_command(
                &state,
                Command::AssignTag {
                    id: tag_id,
                    command: command_text,
                },
            )
            .await;
        }
        Err(err) => warn!(tag_id = form.tag_id, "Invalid tag id: {err}"),
    }

    Redirect::to("/edit_tag")
}

#[derive(Serialize)]
struct TagTemplateContext {
    id: String,
    command: Option<String>,
}

#[derive(Serialize)]
struct EditTagTemplateContext {
    available: bool,
    last_tag: Option<TagTemplateContext>,
}
