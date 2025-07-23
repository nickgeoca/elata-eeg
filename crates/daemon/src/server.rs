use crate::api::{self, AppState};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub async fn run() -> anyhow::Result<()> {
    let (sse_tx, _) = broadcast::channel(100);

    let state = AppState {
        pipelines: Arc::new(Mutex::new(HashMap::new())),
        sse_tx,
        pipeline_handle: Arc::new(Mutex::new(None)),
    };

    let app = api::create_router(state).layer(
        CorsLayer::new().allow_origin(Any).allow_methods(Any),
    ).layer(TraceLayer::new_for_http());

    // run it
    let addr = SocketAddr::from(([0, 0, 0, 0], 9000));
    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}