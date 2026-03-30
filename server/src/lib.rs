use std::sync::Arc;

use axum::{
  Json, Router,
  extract::{Path, State},
  http::{HeaderMap, StatusCode},
  response::{IntoResponse, Response},
  routing::{get, post},
};
use config::Config;
use execution::{Executor, Pipeline, PodmanExecutor, run::Status};
use serde::{Deserialize, Serialize};
use storage::{PipelineRegistry, SqliteStorage, StorageError};
use tower_http::{
  cors::CorsLayer,
  trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, error, info};

const DEFAULT_PIPELINE_YAML: &str = include_str!("../../examples/multinode-pipeline.yaml");

#[derive(Clone)]
pub struct AppState {
  pub storage: Arc<SqliteStorage>,
  pub config: Config,
}

#[derive(Deserialize)]
pub struct SavePipelineRequest {
  pub name: String,
  pub yaml: String,
}

#[derive(Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum RunRequest {
  Inline {
    yaml: String,
  },
  Registry {
    name: String,
  },
  Url {
    url: String,
  },
  Git {
    repo: String,
    git_ref: String,
    path: String,
  },
}

#[derive(Serialize)]
pub struct ErrorResponse {
  pub error: String,
}

pub fn router(state: AppState) -> Router {
  let cors = build_cors_layer(&state.config);

  Router::new()
    .route("/health", get(health))
    .route("/pipelines/default", get(get_default_pipeline))
    .route("/pipelines", post(save_pipeline))
    .route("/pipelines/{name}", get(get_pipeline))
    .route("/run", post(run_pipeline))
    .layer(cors)
    .layer(
      TraceLayer::new_for_http()
        .make_span_with(make_span)
        .on_request(DefaultOnRequest::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO)),
    )
    .with_state(state)
}

fn build_cors_layer(config: &Config) -> CorsLayer {
  if config.server_config.cors.allow_any_origin {
    CorsLayer::very_permissive()
  } else {
    CorsLayer::new()
  }
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

pub async fn serve(
  state: AppState,
  host: &str,
  port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
  let addr = format!("{}:{}", host, port);
  let listener = tokio::net::TcpListener::bind(&addr).await?;
  info!(address = %addr, "Server listening");
  axum::serve(listener, router(state)).await?;
  Ok(())
}

async fn health() -> StatusCode {
  StatusCode::OK
}

async fn get_default_pipeline() -> impl IntoResponse {
  match Pipeline::from_yaml(DEFAULT_PIPELINE_YAML) {
    Ok(pipeline) => Json(pipeline).into_response(),
    Err(e) => {
      error!(error = %e, "Failed to parse default pipeline");
      (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
          error: e.to_string(),
        }),
      )
        .into_response()
    }
  }
}

async fn save_pipeline(
  State(state): State<AppState>,
  Json(body): Json<SavePipelineRequest>,
) -> impl IntoResponse {
  match state.storage.save_pipeline(&body.name, &body.yaml).await {
    Ok(()) => {
      info!(name = %body.name, "Pipeline saved");
      StatusCode::OK.into_response()
    }
    Err(StorageError::Parse(msg)) => (
      StatusCode::UNPROCESSABLE_ENTITY,
      Json(ErrorResponse { error: msg }),
    )
      .into_response(),
    Err(e) => {
      error!(name = %body.name, error = %e, "Failed to save pipeline");
      (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
          error: e.to_string(),
        }),
      )
        .into_response()
    }
  }
}

fn pipeline_not_found(name: &str) -> Response {
  (
    StatusCode::NOT_FOUND,
    Json(ErrorResponse {
      error: format!("Pipeline '{}' not found", name),
    }),
  )
    .into_response()
}

fn storage_error_response(e: StorageError) -> Response {
  (
    StatusCode::INTERNAL_SERVER_ERROR,
    Json(ErrorResponse {
      error: e.to_string(),
    }),
  )
    .into_response()
}

fn pipeline_source_error_response(e: execution::pipeline::PipelineError) -> Response {
  (
    StatusCode::BAD_REQUEST,
    Json(ErrorResponse {
      error: e.to_string(),
    }),
  )
    .into_response()
}

async fn get_pipeline(
  State(state): State<AppState>,
  Path(name): Path<String>,
) -> impl IntoResponse {
  match state.storage.load_pipeline(&name).await {
    Ok(pipeline) => Json(pipeline).into_response(),
    Err(StorageError::NotFound(_)) => pipeline_not_found(&name),
    Err(e) => {
      error!(name = %name, error = %e, "Failed to load pipeline");
      storage_error_response(e)
    }
  }
}

async fn resolve_pipeline(request: RunRequest, state: &AppState) -> Result<Pipeline, Response> {
  match request {
    RunRequest::Inline { yaml } => {
      Pipeline::from_yaml(&yaml).map_err(pipeline_source_error_response)
    }
    RunRequest::Registry { name } => match state.storage.load_pipeline(&name).await {
      Ok(p) => Ok(p),
      Err(StorageError::NotFound(_)) => Err(pipeline_not_found(&name)),
      Err(e) => {
        error!(name = %name, error = %e, "Failed to load pipeline for run");
        Err(storage_error_response(e))
      }
    },
    RunRequest::Url { url } => Pipeline::from_url(&url)
      .await
      .map_err(pipeline_source_error_response),
    RunRequest::Git {
      repo,
      git_ref,
      path,
    } => Pipeline::from_git(&repo, &git_ref, &path)
      .await
      .map_err(pipeline_source_error_response),
  }
}

async fn run_pipeline(
  State(state): State<AppState>,
  Json(body): Json<RunRequest>,
) -> impl IntoResponse {
  let pipeline = match resolve_pipeline(body, &state).await {
    Ok(p) => p,
    Err(e) => return e,
  };

  let config = state.config.clone();
  tokio::spawn(async move {
    info!(pipeline = %pipeline.name, "Starting pipeline execution");
    let executor = PodmanExecutor {};
    let run = executor.execute_pipeline(&pipeline, &config, None).await;
    if run.status == Status::Failure {
      error!(pipeline = %pipeline.name, "Pipeline execution failed");
    } else {
      info!(pipeline = %pipeline.name, status = ?run.status, "Pipeline execution completed");
    }
  });

  StatusCode::ACCEPTED.into_response()
}

#[cfg(test)]
mod tests {
  use axum::{
    body::Body,
    http::{Request, StatusCode},
  };
  use tower::ServiceExt;

  use super::*;

  async fn test_state() -> AppState {
    let storage = SqliteStorage::new("sqlite::memory:")
      .await
      .expect("Failed to create test storage");
    AppState {
      storage: Arc::new(storage),
      config: Config::test(std::env::temp_dir().join("server_test")),
    }
  }

  #[tokio::test]
  async fn test_health_endpoint() {
    let state = test_state().await;
    let app = router(state);

    let response = app
      .oneshot(
        Request::builder()
          .uri("/health")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
  }

  #[tokio::test]
  async fn test_default_pipeline_endpoint() {
    let state = test_state().await;
    let app = router(state);

    let response = app
      .oneshot(
        Request::builder()
          .uri("/pipelines/default")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
      .await
      .unwrap();
    let pipeline: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(pipeline["name"], "test-pipeline");
    assert!(pipeline["nodes"].as_array().unwrap().len() > 0);
  }

  #[tokio::test]
  async fn test_save_and_get_pipeline() {
    let state = test_state().await;
    let app = router(state);

    let yaml = "name: test-pipe\nnodes: []";
    let body = serde_json::json!({ "name": "test-pipe", "yaml": yaml });

    let save_response = app
      .clone()
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/pipelines")
          .header("content-type", "application/json")
          .body(Body::from(serde_json::to_vec(&body).unwrap()))
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(save_response.status(), StatusCode::OK);

    let get_response = app
      .oneshot(
        Request::builder()
          .uri("/pipelines/test-pipe")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(get_response.status(), StatusCode::OK);
  }

  #[tokio::test]
  async fn test_save_pipeline_invalid_yaml() {
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({ "name": "bad", "yaml": "not: valid: yaml: [" });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/pipelines")
          .header("content-type", "application/json")
          .body(Body::from(serde_json::to_vec(&body).unwrap()))
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
  }

  #[tokio::test]
  async fn test_get_pipeline_not_found() {
    let state = test_state().await;
    let app = router(state);

    let response = app
      .oneshot(
        Request::builder()
          .uri("/pipelines/nonexistent")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
  }

  #[tokio::test]
  async fn test_run_pipeline_registry_not_found() {
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({ "source": "registry", "name": "missing-pipeline" });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/run")
          .header("content-type", "application/json")
          .body(Body::from(serde_json::to_vec(&body).unwrap()))
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
  }

  #[tokio::test]
  async fn test_run_pipeline_registry_accepted() {
    let state = test_state().await;
    let app = router(state.clone());

    let yaml = "name: ci\nnodes: []";
    state
      .storage
      .save_pipeline("ci", yaml)
      .await
      .expect("save should succeed");

    let body = serde_json::json!({ "source": "registry", "name": "ci" });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/run")
          .header("content-type", "application/json")
          .body(Body::from(serde_json::to_vec(&body).unwrap()))
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
  }

  #[tokio::test]
  async fn test_run_pipeline_inline_accepted() {
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({ "source": "inline", "yaml": "name: ci\nnodes: []" });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/run")
          .header("content-type", "application/json")
          .body(Body::from(serde_json::to_vec(&body).unwrap()))
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
  }

  #[tokio::test]
  async fn test_run_pipeline_inline_invalid_yaml() {
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({ "source": "inline", "yaml": "not: valid: yaml: [" });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/run")
          .header("content-type", "application/json")
          .body(Body::from(serde_json::to_vec(&body).unwrap()))
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_run_pipeline_url_bad_request() {
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({ "source": "url", "url": "http://0.0.0.0:1/pipeline.yaml" });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/run")
          .header("content-type", "application/json")
          .body(Body::from(serde_json::to_vec(&body).unwrap()))
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_run_pipeline_git_bad_request() {
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({
      "source": "git",
      "repo": "http://0.0.0.0:1/nonexistent.git",
      "git_ref": "main",
      "path": "pipeline.yaml"
    });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/run")
          .header("content-type", "application/json")
          .body(Body::from(serde_json::to_vec(&body).unwrap()))
          .unwrap(),
      )
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  }
}
