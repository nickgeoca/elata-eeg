use crate::{
    api::{self, AppState},
    websocket_broker::WebSocketBroker,
};
use axum::{
    extract::{ConnectInfo, State, WebSocketUpgrade},
    response::{IntoResponse, Response},
    routing::get,
};
use std::{net::SocketAddr, sync::Arc};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

#[axum::debug_handler]
async fn websocket_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Response {
    tracing::info!("Client attempting to connect: {}", addr);
    let broker = state.broker;
    ws.on_upgrade(move |socket| broker.handle_connection(socket, addr))
}

pub async fn run(
    state: AppState,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let app = api::create_router()
        .route("/ws", get(websocket_handler))
        .with_state(state)
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any))
        .layer(TraceLayer::new_for_http());

    // run it
    let addr = SocketAddr::from(([0, 0, 0, 0], 9000));
    tracing::info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
        })
        .await?;

    Ok(())
}