mod sqlite;

use async_trait::async_trait;
pub use sqlite::SqliteStorage;

use execution::run::{JobRun, PipelineRun};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
  #[error("Database error: {0}")]
  Database(#[from] sqlx::Error),
}

#[async_trait]
pub trait Storage: Send + Sync {
  async fn save_pipeline_run(
    &self,
    pipeline_name: &str,
    run: &PipelineRun,
  ) -> Result<(), StorageError>;

  async fn save_node_run(
    &self,
    pipeline_run_id: &str,
    run: &JobRun,
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
    SqliteStorage::new("sqlite::memory:").await.expect("Failed to create in-memory SQLite storage")
  }

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
}
