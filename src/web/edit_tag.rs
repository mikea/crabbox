use std::str::FromStr;

use axum::{
    extract::{Form, Path, State},
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
    filter: Option<String>,
    action: String,
}

pub(super) async fn edit_tag(
    Path(tag_id): Path<String>,
    State(state): State<AppState>,
) -> Html<String> {
    let snapshot = state.crabbox.lock().ok().map(|c| c.snapshot());
    let requested_tag_id = tag_id.clone();

    let context = snapshot.map_or(
        EditTagTemplateContext {
            available: false,
            tag: None,
            error: None,
            tag_id: requested_tag_id.clone(),
        },
        |snapshot| match TagId::from_str(&requested_tag_id) {
            Ok(id) => {
                let command = snapshot
                    .tags
                    .iter()
                    .find(|(tag, _)| *tag == id)
                    .map(|(_, command)| command.clone());
                let (selected_command, filter) = command.as_ref().map_or_else(
                    || ("PLAY".to_string(), None),
                    |command| (command.name().to_string(), command_filter(command)),
                );

                EditTagTemplateContext {
                    available: true,
                    tag: Some(TagTemplateContext {
                        id: id.to_string(),
                        selected_command: selected_command.clone(),
                        filter,
                        command_options: command_options(&selected_command),
                    }),
                    error: None,
                    tag_id: requested_tag_id.clone(),
                }
            }
            Err(err) => EditTagTemplateContext {
                available: true,
                tag: None,
                error: Some(err),
                tag_id: requested_tag_id,
            },
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
                    .map(|command| {
                        let upper = command.to_ascii_uppercase();
                        let mut composed = upper.clone();

                        if command_requires_filter(&upper)
                            && let Some(filter) = form
                                .filter
                                .as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                        {
                            composed = format!("{upper} {filter}");
                        }

                        composed
                    }),
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
    selected_command: String,
    filter: Option<String>,
    command_options: Vec<CommandOptionContext>,
}

#[derive(Serialize)]
struct EditTagTemplateContext {
    available: bool,
    tag: Option<TagTemplateContext>,
    error: Option<String>,
    tag_id: String,
}

#[derive(Serialize)]
struct CommandOptionContext {
    value: String,
    label: String,
    requires_filter: bool,
    selected: bool,
}

fn command_filter(command: &Command) -> Option<String> {
    match command {
        Command::Play { filter } | Command::PlayPause { filter } | Command::Shuffle { filter } => {
            filter.clone()
        }
        _ => None,
    }
}

fn command_requires_filter(name: &str) -> bool {
    matches!(name, "PLAY" | "PLAYPAUSE" | "SHUFFLE")
}

fn command_options(selected_command: &str) -> Vec<CommandOptionContext> {
    let commands = [
        Command::Play { filter: None },
        Command::PlayPause { filter: None },
        Command::Shuffle { filter: None },
        Command::Stop,
        Command::Next,
        Command::Prev,
        Command::VolumeUp,
        Command::VolumeDown,
        Command::Shutdown,
    ];

    commands
        .iter()
        .map(|command| {
            let name = command.name().to_string();
            CommandOptionContext {
                requires_filter: command.has_filter(),
                selected: name == selected_command,
                label: name.clone(),
                value: name,
            }
        })
        .collect()
}
