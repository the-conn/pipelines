use std::sync::Arc;

use app_config::AppConfig;
use axum::{
  Json, Router,
  extract::{Path, State},
  http::{HeaderMap, StatusCode},
  routing::{get, post},
};
use coordinator::{CoordinatorMessage, LogDispatcher, RunRegistry};
use providers::{GithubProvider, ProviderState};
use serde::Deserialize;
use thiserror::Error;
use tokio::signal;
use tower_http::{
  cors::CorsLayer,
  trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info, warn};

#[derive(Error, Debug)]
pub enum ServerError {
  #[error("Server IO error: {0}")]
  IOError(#[from] std::io::Error),
}

#[derive(Debug, Deserialize)]
pub struct NodeStatusUpdate {
  pub node_name: String,
  pub success: bool,
}

fn router(state: Arc<ProviderState>) -> Router {
  let cors = build_cors_layer();

  Router::new()
    .route("/health", get(health))
    .route("/webhooks/github", post(GithubProvider::handle_webhook))
    .route("/runs/{run_id}/status", post(report_node_status))
    .layer(cors)
    .layer(
      TraceLayer::new_for_http()
        .make_span_with(make_span)
        .on_request(DefaultOnRequest::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO)),
    )
    .with_state(state)
}

fn build_cors_layer() -> CorsLayer {
  CorsLayer::very_permissive()
}

fn make_span(request: &axum::http::Request<axum::body::Body>) -> tracing::Span {
  let headers: &HeaderMap = request.headers();
  let trace_id = headers
    .get("traceparent")
    .or_else(|| headers.get("x-trace-id"))
    .or_else(|| headers.get("x-request-id"))
    .and_then(|v| v.to_str().ok())
    .unwrap_or("");

  tracing::info_span!(
    "http_request",
    method = %request.method(),
    uri = %request.uri(),
    trace_id = %trace_id,
  )
}

async fn shutdown_signal() {
  let ctrl_c = async {
    signal::ctrl_c()
      .await
      .expect("failed to install Ctrl+C handler");
  };

  #[cfg(unix)]
  let terminate = async {
    signal::unix::signal(signal::unix::SignalKind::terminate())
      .expect("failed to install signal handler")
      .recv()
      .await;
  };

  #[cfg(not(unix))]
  let terminate = std::future::pending::<()>();

  tokio::select! {
      _ = ctrl_c => { info!("Received Ctrl-C, shutting down..."); },
      _ = terminate => { info!("Received SIGTERM, shutting down..."); },
  }
}

async fn report_node_status(
  State(state): State<Arc<ProviderState>>,
  Path(run_id): Path<String>,
  Json(update): Json<NodeStatusUpdate>,
) -> StatusCode {
  let message = CoordinatorMessage::NodeCompleted {
    node_name: update.node_name,
    success: update.success,
  };
  match state.registry.send(&run_id, message).await {
    Ok(()) => StatusCode::OK,
    Err(e) => {
      warn!(run_id, error = %e, "Failed to route node status update");
      StatusCode::NOT_FOUND
    }
  }
}

pub async fn serve(config: AppConfig) -> Result<(), ServerError> {
  let shared_config = Arc::new(config);
  let registry = RunRegistry::new();
  let dispatcher = Arc::new(LogDispatcher::new(registry.clone()));
  let state = Arc::new(ProviderState::new(
    shared_config.clone(),
    registry,
    dispatcher,
  ));

  let addr = format!("{}:{}", shared_config.host(), shared_config.port());
  let listener = tokio::net::TcpListener::bind(&addr).await?;
  info!(address = %addr, "Starting server...");
  axum::serve(listener, router(state))
    .with_graceful_shutdown(shutdown_signal())
    .await?;
  Ok(())
}

async fn health() -> StatusCode {
  StatusCode::OK
}
