use std::sync::Arc;

use axum::{
  Json, Router,
  extract::{Path, State},
  http::StatusCode,
  response::{IntoResponse, Response},
  routing::{get, post},
};
use config::Config;
use execution::{Executor, PodmanExecutor, run::Status};
use serde::{Deserialize, Serialize};
use storage::{PipelineRegistry, SqliteStorage, StorageError};
use tracing::{error, info};

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
pub struct RunRequest {
  pub name: String,
}

#[derive(Serialize)]
pub struct ErrorResponse {
  pub error: String,
}

pub fn router(state: AppState) -> Router {
  Router::new()
    .route("/health", get(health))
    .route("/pipelines", post(save_pipeline))
    .route("/pipelines/{name}", get(get_pipeline))
    .route("/run", post(run_pipeline))
    .with_state(state)
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

async fn save_pipeline(
  State(state): State<AppState>,
  Json(body): Json<SavePipelineRequest>,
) -> impl IntoResponse {
  match state.storage.save_pipeline(&body.name, &body.yaml).await {
    Ok(()) => {
      info!(name = %body.name, "Pipeline saved");
      StatusCode::OK.into_response()
    }
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

async fn run_pipeline(
  State(state): State<AppState>,
  Json(body): Json<RunRequest>,
) -> impl IntoResponse {
  let pipeline = match state.storage.load_pipeline(&body.name).await {
    Ok(p) => p,
    Err(StorageError::NotFound(_)) => return pipeline_not_found(&body.name),
    Err(e) => {
      error!(name = %body.name, error = %e, "Failed to load pipeline for run");
      return storage_error_response(e);
    }
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
  async fn test_run_pipeline_not_found() {
    let state = test_state().await;
    let app = router(state);

    let body = serde_json::json!({ "name": "missing-pipeline" });

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
  async fn test_run_pipeline_accepted() {
    let state = test_state().await;
    let app = router(state.clone());

    let yaml = "name: ci\nnodes: []";
    state
      .storage
      .save_pipeline("ci", yaml)
      .await
      .expect("save should succeed");

    let body = serde_json::json!({ "name": "ci" });

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
}
