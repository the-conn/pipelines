mod sqlite;

use std::time::SystemTime;

use async_trait::async_trait;
use execution::{
  Pipeline,
  pipeline::PipelineSource,
  run::{JobRun, PipelineRun, Status},
};
pub use sqlite::SqliteStorage;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
  #[error("Database error: {0}")]
  Database(#[from] sqlx::Error),
  #[error("Not found: {0}")]
  NotFound(String),
  #[error("Parse error: {0}")]
  Parse(String),
}

pub struct PipelineDefinition {
  pub name: String,
  pub yaml_source: String,
  pub created_at: SystemTime,
  pub updated_at: SystemTime,
}

pub struct PipelineRunSummary {
  pub id: String,
  pub pipeline_name: String,
  pub status: Status,
  pub created_at: SystemTime,
  pub started_at: Option<SystemTime>,
  pub ended_at: Option<SystemTime>,
}

pub struct PipelineRunDetails {
  pub id: String,
  pub pipeline_name: String,
  pub pipeline_snapshot: String,
  pub status: Status,
  pub created_at: SystemTime,
  pub started_at: Option<SystemTime>,
  pub ended_at: Option<SystemTime>,
  pub node_runs: Vec<JobRun>,
}

#[async_trait]
pub trait Storage: Send + Sync {
  async fn save_pipeline_run(&self, run: &PipelineRun) -> Result<(), StorageError>;

  async fn save_node_run(&self, pipeline_run_id: &str, run: &JobRun) -> Result<(), StorageError>;

  async fn register_pipeline(&self, name: &str, yaml_source: &str) -> Result<(), StorageError>;

  async fn get_pipeline(&self, name: &str) -> Result<PipelineDefinition, StorageError>;

  async fn list_pipelines(&self) -> Result<Vec<PipelineDefinition>, StorageError>;

  async fn delete_pipeline(&self, name: &str) -> Result<(), StorageError>;

  async fn list_runs(&self, limit: i64) -> Result<Vec<PipelineRunSummary>, StorageError>;

  async fn get_runs_by_pipeline(
    &self,
    pipeline_name: &str,
  ) -> Result<Vec<PipelineRunSummary>, StorageError>;

  async fn get_run_details(&self, run_id: &str) -> Result<PipelineRunDetails, StorageError>;

  async fn update_run_status(&self, run_id: &str, status: Status) -> Result<(), StorageError>;

  async fn load_pipeline(&self, name: &str) -> Result<Pipeline, StorageError> {
    let definition = self.get_pipeline(name).await?;
    let mut pipeline = Pipeline::from_yaml(&definition.yaml_source)
      .map_err(|e| StorageError::Parse(e.to_string()))?;
    pipeline.source = PipelineSource::Registry {
      name: name.to_string(),
    };
    Ok(pipeline)
  }
}

#[cfg(test)]
mod tests {
  use std::time::SystemTime;

  use execution::{
    Node, Pipeline,
    pipeline::PipelineSource,
    run::{JobRun, PipelineRun, Status},
  };

  use super::*;

  async fn in_memory_storage() -> SqliteStorage {
    SqliteStorage::new("sqlite::memory:")
      .await
      .expect("Failed to create in-memory SQLite storage")
  }

  fn make_pipeline(name: &str) -> Pipeline {
    Pipeline {
      name: name.to_string(),
      nodes: Vec::new(),
      source: PipelineSource::Inline,
    }
  }

  #[tokio::test]
  async fn test_save_pipeline_run() {
    let storage = in_memory_storage().await;
    let mut run = PipelineRun::new(make_pipeline("my-pipeline"));
    run.status = Status::Success;
    run.started_at = Some(SystemTime::now());
    run.ended_at = Some(SystemTime::now());

    let result = storage.save_pipeline_run(&run).await;
    assert!(result.is_ok(), "save_pipeline_run should succeed");
  }

  #[tokio::test]
  async fn test_save_node_run() {
    let storage = in_memory_storage().await;
    let mut pipeline_run = PipelineRun::new(make_pipeline("my-pipeline"));
    pipeline_run.status = Status::Success;

    storage
      .save_pipeline_run(&pipeline_run)
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
    let mut run = PipelineRun::new(make_pipeline("failing-pipeline"));
    run.status = Status::Failure;

    let result = storage.save_pipeline_run(&run).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_save_node_run_without_timestamps() {
    let storage = in_memory_storage().await;
    let pipeline_run = PipelineRun::new(make_pipeline("my-pipeline"));

    storage
      .save_pipeline_run(&pipeline_run)
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

    let list = storage
      .list_pipelines()
      .await
      .expect("list_pipelines should succeed");

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

    let run = PipelineRun::new(make_pipeline("to-delete"));
    storage.save_pipeline_run(&run).await.expect("save run");

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

  #[tokio::test]
  async fn test_list_runs() {
    let storage = in_memory_storage().await;

    for i in 0..3 {
      let mut run = PipelineRun::new(make_pipeline(&format!("pipe-{i}")));
      run.status = Status::Success;
      storage.save_pipeline_run(&run).await.expect("save run");
    }

    let runs = storage
      .list_runs(10)
      .await
      .expect("list_runs should succeed");
    assert_eq!(runs.len(), 3);
  }

  #[tokio::test]
  async fn test_list_runs_limit() {
    let storage = in_memory_storage().await;

    for i in 0..5 {
      let run = PipelineRun::new(make_pipeline(&format!("pipe-{i}")));
      storage.save_pipeline_run(&run).await.expect("save run");
    }

    let runs = storage.list_runs(2).await.expect("list_runs with limit");
    assert_eq!(runs.len(), 2);
  }

  #[tokio::test]
  async fn test_get_runs_by_pipeline() {
    let storage = in_memory_storage().await;

    for _ in 0..3 {
      let run = PipelineRun::new(make_pipeline("target-pipe"));
      storage.save_pipeline_run(&run).await.expect("save run");
    }

    let run = PipelineRun::new(make_pipeline("other-pipe"));
    storage
      .save_pipeline_run(&run)
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

    let node = Node {
      name: "compile".to_string(),
      image: "rust:alpine".to_string(),
      environment: std::collections::HashMap::from([("FOO".to_string(), "bar".to_string())]),
      steps: vec!["cargo build".to_string(), "cargo test".to_string()],
    };
    let pipeline = Pipeline {
      name: "detail-pipe".to_string(),
      nodes: vec![node.clone()],
      source: PipelineSource::Inline,
    };

    let mut pipeline_run = PipelineRun::new(pipeline);
    pipeline_run.status = Status::Success;
    pipeline_run.started_at = Some(SystemTime::now());
    pipeline_run.ended_at = Some(SystemTime::now());
    let run_id = pipeline_run.id.clone();

    storage
      .save_pipeline_run(&pipeline_run)
      .await
      .expect("save pipeline run");

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
    assert!(
      !details.pipeline_snapshot.is_empty(),
      "pipeline_snapshot should be stored"
    );
    let snapshot: serde_json::Value =
      serde_json::from_str(&details.pipeline_snapshot).expect("snapshot should be valid JSON");
    assert_eq!(snapshot["name"], "detail-pipe");
    assert_eq!(details.node_runs.len(), 1);
    assert_eq!(details.node_runs[0].node.name, "compile");
    assert_eq!(details.node_runs[0].node.image, "rust:alpine");
    assert_eq!(
      details.node_runs[0].node.steps,
      vec!["cargo build", "cargo test"]
    );
    assert_eq!(
      details.node_runs[0]
        .node
        .environment
        .get("FOO")
        .map(|s| s.as_str()),
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

    let run = PipelineRun::new(make_pipeline("my-pipe"));
    let run_id = run.id.clone();
    storage.save_pipeline_run(&run).await.expect("save run");

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

  #[tokio::test]
  async fn test_pipeline_snapshot_is_independent_of_registry() {
    let storage = in_memory_storage().await;

    storage
      .register_pipeline("evolving-pipe", "name: evolving-pipe\nnodes: []")
      .await
      .expect("register v1");

    let node_v1 = Node {
      name: "step-v1".to_string(),
      image: "alpine:1".to_string(),
      environment: Default::default(),
      steps: vec!["echo v1".to_string()],
    };
    let pipeline_v1 = Pipeline {
      name: "evolving-pipe".to_string(),
      nodes: vec![node_v1],
      source: PipelineSource::Inline,
    };
    let run_v1 = PipelineRun::new(pipeline_v1);
    let run_v1_id = run_v1.id.clone();
    storage
      .save_pipeline_run(&run_v1)
      .await
      .expect("save v1 run");

    storage
      .register_pipeline("evolving-pipe", "name: evolving-pipe\nnodes: [{name: step-v2, image: alpine:2, environment: {}, steps: [echo v2]}]")
      .await
      .expect("register v2");

    let details = storage
      .get_run_details(&run_v1_id)
      .await
      .expect("get_run_details v1");

    let snapshot: serde_json::Value =
      serde_json::from_str(&details.pipeline_snapshot).expect("snapshot is valid JSON");

    assert_eq!(
      snapshot["nodes"][0]["name"], "step-v1",
      "run snapshot should preserve the pipeline definition at the time of the run"
    );
  }

  #[tokio::test]
  async fn test_load_pipeline_returns_pipeline_struct() {
    let storage = in_memory_storage().await;
    let yaml = "name: my-pipeline\nnodes: []";

    storage
      .register_pipeline("my-pipeline", yaml)
      .await
      .expect("register_pipeline should succeed");

    let pipeline = storage
      .load_pipeline("my-pipeline")
      .await
      .expect("load_pipeline should succeed");

    assert_eq!(pipeline.name, "my-pipeline");
    assert_eq!(
      pipeline.source,
      PipelineSource::Registry {
        name: "my-pipeline".to_string()
      }
    );
  }

  #[tokio::test]
  async fn test_load_pipeline_not_found() {
    let storage = in_memory_storage().await;
    let result = storage.load_pipeline("nonexistent").await;
    assert!(
      matches!(result, Err(StorageError::NotFound(_))),
      "Expected NotFound for unregistered pipeline"
    );
  }
}
