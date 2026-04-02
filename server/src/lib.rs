use axum::{
  Router,
  http::{HeaderMap, StatusCode},
  routing::get,
};
use thiserror::Error;
use tower_http::{
  cors::CorsLayer,
  trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info};

#[derive(Error, Debug)]
pub enum ServerError {
  #[error("Server IO error: {0}")]
  IOError(#[from] std::io::Error),
}

fn router() -> Router {
  let cors = build_cors_layer();

  Router::new()
    .route("/health", get(health))
    .layer(cors)
    .layer(
      TraceLayer::new_for_http()
        .make_span_with(make_span)
        .on_request(DefaultOnRequest::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO)),
    )
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

pub async fn serve(host: &str, port: u16) -> Result<(), ServerError> {
  let addr = format!("{}:{}", host, port);
  let listener = tokio::net::TcpListener::bind(&addr).await?;
  info!(address = %addr, "Starting server...");
  axum::serve(listener, router()).await?;
  Ok(())
}

async fn health() -> StatusCode {
  StatusCode::OK
}
