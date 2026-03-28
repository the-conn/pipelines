mod sqlite;

use std::time::SystemTime;

use async_trait::async_trait;
pub use sqlite::SqliteStorage;

use execution::run::{JobRun, PipelineRun, Status};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
  #[error("Database error: {0}")]
  Database(#[from] sqlx::Error),
  #[error("Not found: {0}")]
  NotFound(String),
  #[error("Parse error: {0}")]
  Parse(String),
}

/// A registered pipeline definition stored in the registry.
pub struct PipelineDefinition {
  pub name: String,
  pub yaml_source: String,
  pub created_at: SystemTime,
  pub updated_at: SystemTime,
}

/// A lightweight summary of a pipeline run (without node details).
pub struct PipelineRunSummary {
  pub id: String,
  pub pipeline_name: String,
  pub status: Status,
  pub created_at: SystemTime,
  pub started_at: Option<SystemTime>,
  pub ended_at: Option<SystemTime>,
}

/// A full pipeline run record including all associated node runs.
pub struct PipelineRunDetails {
  pub id: String,
  pub pipeline_name: String,
  pub status: Status,
  pub created_at: SystemTime,
  pub started_at: Option<SystemTime>,
  pub ended_at: Option<SystemTime>,
  pub node_runs: Vec<JobRun>,
}

#[async_trait]
pub trait Storage: Send + Sync {
  // ── Write ──────────────────────────────────────────────────────────────────

  /// Persist the final state of a pipeline run.
  async fn save_pipeline_run(
    &self,
    pipeline_name: &str,
    run: &PipelineRun,
  ) -> Result<(), StorageError>;

  /// Persist the final state of an individual node run.
  async fn save_node_run(
    &self,
    pipeline_run_id: &str,
    run: &JobRun,
  ) -> Result<(), StorageError>;

  // ── Pipeline Registry ──────────────────────────────────────────────────────

  /// Create or overwrite a pipeline definition in the registry.
  async fn register_pipeline(
    &self,
    name: &str,
    yaml_source: &str,
  ) -> Result<(), StorageError>;

  /// Fetch a single pipeline definition by name.
  async fn get_pipeline(&self, name: &str) -> Result<PipelineDefinition, StorageError>;

  /// Return all registered pipeline definitions.
  async fn list_pipelines(&self) -> Result<Vec<PipelineDefinition>, StorageError>;

  /// Remove a pipeline definition and all of its associated run history.
  async fn delete_pipeline(&self, name: &str) -> Result<(), StorageError>;

  // ── Execution History ──────────────────────────────────────────────────────

  /// Return the most recent pipeline runs across all pipelines, newest first.
  /// `limit` caps the number of results returned.
  async fn list_runs(&self, limit: i64) -> Result<Vec<PipelineRunSummary>, StorageError>;

  /// Return all runs for a specific pipeline, newest first.
  async fn get_runs_by_pipeline(
    &self,
    pipeline_name: &str,
  ) -> Result<Vec<PipelineRunSummary>, StorageError>;

  /// Retrieve a full pipeline run including all of its node runs.
  async fn get_run_details(&self, run_id: &str) -> Result<PipelineRunDetails, StorageError>;

  /// Overwrite the status of an existing pipeline run (e.g., mark as Aborted).
  async fn update_run_status(
    &self,
    run_id: &str,
    status: Status,
  ) -> Result<(), StorageError>;
}

#[cfg(test)]
mod tests {
  use std::time::SystemTime;

  use execution::{
    Node,
    run::{JobRun, PipelineRun, Status},
  };

  use super::*;

  async fn in_memory_storage() -> SqliteStorage {
    SqliteStorage::new("sqlite::memory:")
      .await
      .expect("Failed to create in-memory SQLite storage")
  }

  // ── Write tests (existing) ─────────────────────────────────────────────────

  #[tokio::test]
  async fn test_save_pipeline_run() {
    let storage = in_memory_storage().await;
    let mut run = PipelineRun::new();
    run.status = Status::Success;
    run.started_at = Some(SystemTime::now());
    run.ended_at = Some(SystemTime::now());

    let result = storage.save_pipeline_run("my-pipeline", &run).await;
    assert!(result.is_ok(), "save_pipeline_run should succeed");
  }

  #[tokio::test]
  async fn test_save_node_run() {
    let storage = in_memory_storage().await;
    let mut pipeline_run = PipelineRun::new();
    pipeline_run.status = Status::Success;

    storage
      .save_pipeline_run("my-pipeline", &pipeline_run)
      .await
      .expect("Failed to save pipeline run");

    let node = Node {
      name: "build".to_string(),
      image: "rust:alpine".to_string(),
      environment: Default::default(),
      steps: vec!["cargo build".to_string()],
    };
    let mut node_run = JobRun::new(node);
    node_run.status = Status::Success;
    node_run.started_at = Some(SystemTime::now());
    node_run.ended_at = Some(SystemTime::now());

    let result = storage.save_node_run(&pipeline_run.id, &node_run).await;
    assert!(result.is_ok(), "save_node_run should succeed");
  }

  #[tokio::test]
  async fn test_save_pipeline_run_failure_status() {
    let storage = in_memory_storage().await;
    let mut run = PipelineRun::new();
    run.status = Status::Failure;

    let result = storage.save_pipeline_run("failing-pipeline", &run).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_save_node_run_without_timestamps() {
    let storage = in_memory_storage().await;
    let pipeline_run = PipelineRun::new();

    storage
      .save_pipeline_run("my-pipeline", &pipeline_run)
      .await
      .expect("Failed to save pipeline run");

    let node = Node {
      name: "lint".to_string(),
      image: "alpine:latest".to_string(),
      environment: Default::default(),
      steps: vec!["echo lint".to_string()],
    };
    let node_run = JobRun::new(node);

    let result = storage.save_node_run(&pipeline_run.id, &node_run).await;
    assert!(result.is_ok());
  }

  // ── Pipeline Registry tests ────────────────────────────────────────────────

  #[tokio::test]
  async fn test_register_and_get_pipeline() {
    let storage = in_memory_storage().await;
    let yaml = "name: my-pipeline\nnodes: []";

    storage
      .register_pipeline("my-pipeline", yaml)
      .await
      .expect("register_pipeline should succeed");

    let def = storage
      .get_pipeline("my-pipeline")
      .await
      .expect("get_pipeline should find it");

    assert_eq!(def.name, "my-pipeline");
    assert_eq!(def.yaml_source, yaml);
  }

  #[tokio::test]
  async fn test_register_pipeline_overwrites() {
    let storage = in_memory_storage().await;

    storage
      .register_pipeline("pipe", "yaml: v1")
      .await
      .expect("first register should succeed");

    storage
      .register_pipeline("pipe", "yaml: v2")
      .await
      .expect("second register (overwrite) should succeed");

    let def = storage
      .get_pipeline("pipe")
      .await
      .expect("get_pipeline should find it");

    assert_eq!(def.yaml_source, "yaml: v2");
  }

  #[tokio::test]
  async fn test_get_pipeline_not_found() {
    let storage = in_memory_storage().await;
    let result = storage.get_pipeline("nonexistent").await;
    assert!(
      matches!(result, Err(StorageError::NotFound(_))),
      "Expected NotFound, got: {:?}",
      result.err()
    );
  }

  #[tokio::test]
  async fn test_list_pipelines() {
    let storage = in_memory_storage().await;

    storage
      .register_pipeline("alpha", "a: 1")
      .await
      .expect("register alpha");
    storage
      .register_pipeline("beta", "b: 2")
      .await
      .expect("register beta");

    let list = storage.list_pipelines().await.expect("list_pipelines should succeed");

    let names: Vec<&str> = list.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
  }

  #[tokio::test]
  async fn test_delete_pipeline_removes_runs() {
    let storage = in_memory_storage().await;

    storage
      .register_pipeline("to-delete", "y: 1")
      .await
      .expect("register");

    let run = PipelineRun::new();
    storage
      .save_pipeline_run("to-delete", &run)
      .await
      .expect("save run");

    storage
      .delete_pipeline("to-delete")
      .await
      .expect("delete_pipeline should succeed");

    let result = storage.get_pipeline("to-delete").await;
    assert!(matches!(result, Err(StorageError::NotFound(_))));

    let runs = storage
      .get_runs_by_pipeline("to-delete")
      .await
      .expect("get_runs_by_pipeline after delete");
    assert!(runs.is_empty(), "Runs should be removed with the pipeline");
  }

  // ── Execution History tests ────────────────────────────────────────────────

  #[tokio::test]
  async fn test_list_runs() {
    let storage = in_memory_storage().await;

    for i in 0..3 {
      let mut run = PipelineRun::new();
      run.status = Status::Success;
      storage
        .save_pipeline_run(&format!("pipe-{i}"), &run)
        .await
        .expect("save run");
    }

    let runs = storage.list_runs(10).await.expect("list_runs should succeed");
    assert_eq!(runs.len(), 3);
  }

  #[tokio::test]
  async fn test_list_runs_limit() {
    let storage = in_memory_storage().await;

    for i in 0..5 {
      let run = PipelineRun::new();
      storage
        .save_pipeline_run(&format!("pipe-{i}"), &run)
        .await
        .expect("save run");
    }

    let runs = storage.list_runs(2).await.expect("list_runs with limit");
    assert_eq!(runs.len(), 2);
  }

  #[tokio::test]
  async fn test_get_runs_by_pipeline() {
    let storage = in_memory_storage().await;

    for _ in 0..3 {
      let run = PipelineRun::new();
      storage
        .save_pipeline_run("target-pipe", &run)
        .await
        .expect("save run");
    }

    let run = PipelineRun::new();
    storage
      .save_pipeline_run("other-pipe", &run)
      .await
      .expect("save other run");

    let runs = storage
      .get_runs_by_pipeline("target-pipe")
      .await
      .expect("get_runs_by_pipeline should succeed");

    assert_eq!(runs.len(), 3);
    assert!(runs.iter().all(|r| r.pipeline_name == "target-pipe"));
  }

  #[tokio::test]
  async fn test_get_run_details_with_node_runs() {
    let storage = in_memory_storage().await;

    let mut pipeline_run = PipelineRun::new();
    pipeline_run.status = Status::Success;
    pipeline_run.started_at = Some(SystemTime::now());
    pipeline_run.ended_at = Some(SystemTime::now());
    let run_id = pipeline_run.id.clone();

    storage
      .save_pipeline_run("detail-pipe", &pipeline_run)
      .await
      .expect("save pipeline run");

    let node = Node {
      name: "compile".to_string(),
      image: "rust:alpine".to_string(),
      environment: std::collections::HashMap::from([("FOO".to_string(), "bar".to_string())]),
      steps: vec!["cargo build".to_string(), "cargo test".to_string()],
    };
    let mut node_run = JobRun::new(node);
    node_run.status = Status::Success;
    node_run.started_at = Some(SystemTime::now());
    node_run.ended_at = Some(SystemTime::now());

    storage
      .save_node_run(&run_id, &node_run)
      .await
      .expect("save node run");

    let details = storage
      .get_run_details(&run_id)
      .await
      .expect("get_run_details should succeed");

    assert_eq!(details.id, run_id);
    assert_eq!(details.pipeline_name, "detail-pipe");
    assert_eq!(details.status, Status::Success);
    assert_eq!(details.node_runs.len(), 1);
    assert_eq!(details.node_runs[0].node.name, "compile");
    assert_eq!(details.node_runs[0].node.image, "rust:alpine");
    assert_eq!(details.node_runs[0].node.steps, vec!["cargo build", "cargo test"]);
    assert_eq!(
      details.node_runs[0].node.environment.get("FOO").map(|s| s.as_str()),
      Some("bar")
    );
  }

  #[tokio::test]
  async fn test_get_run_details_not_found() {
    let storage = in_memory_storage().await;
    let result = storage.get_run_details("nonexistent-id").await;
    assert!(
      matches!(result, Err(StorageError::NotFound(_))),
      "Expected NotFound"
    );
  }

  #[tokio::test]
  async fn test_update_run_status() {
    let storage = in_memory_storage().await;

    let run = PipelineRun::new();
    let run_id = run.id.clone();
    storage
      .save_pipeline_run("my-pipe", &run)
      .await
      .expect("save run");

    storage
      .update_run_status(&run_id, Status::Aborted)
      .await
      .expect("update_run_status should succeed");

    let details = storage
      .get_run_details(&run_id)
      .await
      .expect("get_run_details after update");

    assert_eq!(details.status, Status::Aborted);
  }
}
