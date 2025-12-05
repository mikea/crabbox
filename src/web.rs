use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::State,
    response::{Html, Redirect},
    routing::{get, post},
};
use tokio::net::TcpListener;

use crate::{AnyResult, commands::Command, crabbox::Crabbox};

pub async fn serve_web(addr: SocketAddr, crabbox: Arc<Mutex<Crabbox>>) -> AnyResult<()> {
    let state = AppState { crabbox };

    let app = Router::new()
        .route("/", get(index))
        .route("/play", post(play))
        .route("/stop", post(stop))
        .with_state(state);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index(State(state): State<AppState>) -> Html<String> {
    let current = state
        .crabbox
        .lock()
        .ok()
        .and_then(|c| c.current_track())
        .map_or_else(
            || "Nothing playing".to_string(),
            |p| p.display().to_string(),
        );

    let page = format!(
        r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Crabbox</title>
  </head>
  <body>
    <h1>Hello from Crabbox</h1>
    <p>Current track: {current}</p>
    <form method="post" action="/play">
      <button type="submit">Play</button>
    </form>
    <form method="post" action="/stop" style="margin-top: 8px;">
      <button type="submit">Stop</button>
    </form>
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
    let sender = state.crabbox.lock().ok().map(|c| c.sender());

    if let Some(sender) = sender {
        let _ = sender.send(Command::Play { filter: None }).await;
    }
    Redirect::to("/")
}

async fn stop(State(state): State<AppState>) -> Redirect {
    let sender = state.crabbox.lock().ok().map(|c| c.sender());

    if let Some(sender) = sender {
        let _ = sender.send(Command::Stop).await;
    }
    Redirect::to("/")
}
