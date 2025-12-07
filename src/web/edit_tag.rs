use std::str::FromStr;

use axum::{
    extract::{Form, State},
    response::{Html, Redirect},
};
use serde::Deserialize;
use tracing::warn;

use crate::{commands::Command, tag::TagId};

use super::{AppState, escape_html, send_command};

#[derive(Deserialize)]
pub(super) struct AssignTagForm {
    tag_id: String,
    command: Option<String>,
    action: String,
}

pub(super) async fn edit_tag(State(state): State<AppState>) -> Html<String> {
    let snapshot = state.crabbox.lock().ok().map(|c| c.snapshot());

    let Some(snapshot) = snapshot else {
        return Html("<p>Crabbox unavailable</p>".to_string());
    };

    let content = match snapshot.last_tag {
        Some(tag) => {
            let tag_text = escape_html(&tag.to_string());
            let command_value = snapshot
                .last_tag_command
                .map(|command| escape_html(&command.to_string()))
                .unwrap_or_default();

            format!(
                r#"<div class=\"section\">
      <p>Tag ID: <strong>{tag_text}</strong></p>
      <form method=\"post\" action=\"/assign_tag\" class=\"command\">
        <input type=\"hidden\" name=\"tag_id\" value=\"{tag_text}\" />
        <input type=\"text\" name=\"command\" value=\"{command_value}\" placeholder=\"Command e.g. PLAY chill/*\" />
        <button type=\"submit\" name=\"action\" value=\"save\">Save</button>
        <button type=\"submit\" name=\"action\" value=\"delete\" class=\"danger\">Delete</button>
      </form>
    </div>"#
            )
        }
        None => "<div class=\"section\"><p>No tag has been scanned yet.</p></div>".to_string(),
    };

    let page = format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset=\"utf-8\" />
    <title>Edit tag · Crabbox</title>
    <style>
      body {{ font-family: Arial, sans-serif; padding: 24px; background: #f4f4f7; color: #222; }}
      h1 {{ margin-top: 0; }}
      form {{ margin: 0; }}
      button {{ width: 100%; padding: 10px; border: none; background: #0f62fe; color: #fff; border-radius: 6px; cursor: pointer; font-size: 14px; }}
      button:hover {{ background: #0b4cc0; }}
      button.danger {{ background: #da1e28; }}
      button.danger:hover {{ background: #a2191f; }}
      .command {{ margin: 16px 0; max-width: 520px; display: grid; grid-template-columns: 1fr 120px 120px; gap: 8px; }}
      .command input {{ padding: 10px; border: 1px solid #ccc; border-radius: 6px; }}
      .section {{ background: #fff; padding: 16px; border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,0.08); margin-bottom: 16px; max-width: 720px; }}
      .muted {{ color: #666; }}
      .link-button {{ display: inline-block; padding: 10px 14px; background: #6f6f6f; color: #fff; border-radius: 6px; text-decoration: none; }}
      .link-button:hover {{ background: #525252; }}
    </style>
  </head>
  <body>
    <h1>Edit last tag</h1>
    {content}
    <div class=\"section\">
      <a class=\"link-button\" href=\"/\">Back to controls</a>
    </div>
  </body>
</html>"#
    );

    Html(page)
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
