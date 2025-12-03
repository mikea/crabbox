use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::State,
    response::{Html, Redirect},
    routing::{get, post},
};
use tokio::net::TcpListener;

use crate::{
    AnyResult,
    crabbox::{Command, Crabbox},
};

pub async fn serve_web(addr: SocketAddr, crabbox: Arc<Crabbox>) -> AnyResult<()> {
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

async fn index() -> Html<&'static str> {
    const PAGE: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Crabbox</title>
  </head>
  <body>
    <h1>Hello from Crabbox</h1>
    <form method="post" action="/play">
      <button type="submit">Play</button>
    </form>
    <form method="post" action="/stop" style="margin-top: 8px;">
      <button type="submit">Stop</button>
    </form>
  </body>
</html>"#;

    Html(PAGE)
}

#[derive(Clone)]
struct AppState {
    crabbox: Arc<Crabbox>,
}

async fn play(State(state): State<AppState>) -> Redirect {
    let _ = state.crabbox.sender().send(Command::Play).await;
    Redirect::to("/")
}

async fn stop(State(state): State<AppState>) -> Redirect {
    let _ = state.crabbox.sender().send(Command::Stop).await;
    Redirect::to("/")
}
