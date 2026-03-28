mod sqlite;

use async_trait::async_trait;
use execution::{
  Pipeline,
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

#[async_trait]
pub trait PipelineRegistry: Send + Sync {
  async fn save_pipeline(&self, name: &str, yaml_source: &str) -> Result<(), StorageError>;

  async fn load_pipeline(&self, name: &str) -> Result<Pipeline, StorageError>;

  async fn list_pipelines(&self) -> Result<Vec<String>, StorageError>;

  async fn delete_pipeline(&self, name: &str) -> Result<(), StorageError>;
}

#[async_trait]
pub trait RunHistory: Send + Sync {
  async fn save_pipeline_run(&self, run: &PipelineRun) -> Result<(), StorageError>;

  async fn save_job_run(&self, pipeline_run_id: &str, run: &JobRun) -> Result<(), StorageError>;

  async fn get_pipeline_run(&self, run_id: &str) -> Result<PipelineRun, StorageError>;

  async fn get_job_run(&self, job_run_id: &str) -> Result<JobRun, StorageError>;

  async fn list_recent_pipeline_runs(&self, limit: i64) -> Result<Vec<PipelineRun>, StorageError>;

  async fn list_recent_job_runs(&self, limit: i64) -> Result<Vec<JobRun>, StorageError>;

  async fn update_run_status(&self, run_id: &str, status: Status) -> Result<(), StorageError>;
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
  async fn test_save_job_run() {
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
    let mut job_run = JobRun::new(node);
    job_run.status = Status::Success;
    job_run.started_at = Some(SystemTime::now());
    job_run.ended_at = Some(SystemTime::now());

    let result = storage.save_job_run(&pipeline_run.id, &job_run).await;
    assert!(result.is_ok(), "save_job_run should succeed");
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
  async fn test_save_job_run_without_timestamps() {
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
    let job_run = JobRun::new(node);

    let result = storage.save_job_run(&pipeline_run.id, &job_run).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_save_and_load_pipeline() {
    let storage = in_memory_storage().await;
    let yaml = "name: my-pipeline\nnodes: []";

    storage
      .save_pipeline("my-pipeline", yaml)
      .await
      .expect("save_pipeline should succeed");

    let pipeline = storage
      .load_pipeline("my-pipeline")
      .await
      .expect("load_pipeline should find it");

    assert_eq!(pipeline.name, "my-pipeline");
    assert_eq!(
      pipeline.source,
      PipelineSource::Registry {
        name: "my-pipeline".to_string()
      }
    );
  }

  #[tokio::test]
  async fn test_save_pipeline_overwrites() {
    let storage = in_memory_storage().await;

    storage
      .save_pipeline("pipe", "name: pipe\nnodes: []")
      .await
      .expect("first save should succeed");

    storage
      .save_pipeline("pipe", "name: pipe\nnodes: []\n# v2")
      .await
      .expect("second save (overwrite) should succeed");

    let pipeline = storage
      .load_pipeline("pipe")
      .await
      .expect("load_pipeline should find it");

    assert_eq!(pipeline.name, "pipe");
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

  #[tokio::test]
  async fn test_list_pipelines() {
    let storage = in_memory_storage().await;

    storage
      .save_pipeline("alpha", "name: alpha\nnodes: []")
      .await
      .expect("save alpha");
    storage
      .save_pipeline("beta", "name: beta\nnodes: []")
      .await
      .expect("save beta");

    let names = storage
      .list_pipelines()
      .await
      .expect("list_pipelines should succeed");

    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));
  }

  #[tokio::test]
  async fn test_delete_pipeline_removes_runs() {
    let storage = in_memory_storage().await;

    storage
      .save_pipeline("to-delete", "name: to-delete\nnodes: []")
      .await
      .expect("save pipeline");

    let run = PipelineRun::new(make_pipeline("to-delete"));
    let run_id = run.id.clone();
    storage.save_pipeline_run(&run).await.expect("save run");

    storage
      .delete_pipeline("to-delete")
      .await
      .expect("delete_pipeline should succeed");

    let result = storage.load_pipeline("to-delete").await;
    assert!(matches!(result, Err(StorageError::NotFound(_))));

    let run_result = storage.get_pipeline_run(&run_id).await;
    assert!(
      matches!(run_result, Err(StorageError::NotFound(_))),
      "Runs should be removed with the pipeline"
    );
  }

  #[tokio::test]
  async fn test_list_recent_pipeline_runs() {
    let storage = in_memory_storage().await;

    for i in 0..3 {
      let mut run = PipelineRun::new(make_pipeline(&format!("pipe-{i}")));
      run.status = Status::Success;
      storage.save_pipeline_run(&run).await.expect("save run");
    }

    let runs = storage
      .list_recent_pipeline_runs(10)
      .await
      .expect("list_recent_pipeline_runs should succeed");
    assert_eq!(runs.len(), 3);
  }

  #[tokio::test]
  async fn test_list_recent_pipeline_runs_limit() {
    let storage = in_memory_storage().await;

    for i in 0..5 {
      let run = PipelineRun::new(make_pipeline(&format!("pipe-{i}")));
      storage.save_pipeline_run(&run).await.expect("save run");
    }

    let runs = storage
      .list_recent_pipeline_runs(2)
      .await
      .expect("list_recent_pipeline_runs with limit");
    assert_eq!(runs.len(), 2);
  }

  #[tokio::test]
  async fn test_list_recent_job_runs() {
    let storage = in_memory_storage().await;

    let pipeline_run = PipelineRun::new(make_pipeline("pipe"));
    storage
      .save_pipeline_run(&pipeline_run)
      .await
      .expect("save pipeline run");

    for i in 0..3 {
      let node = Node {
        name: format!("node-{i}"),
        image: "alpine:latest".to_string(),
        environment: Default::default(),
        steps: vec![format!("echo {i}")],
      };
      let job_run = JobRun::new(node);
      storage
        .save_job_run(&pipeline_run.id, &job_run)
        .await
        .expect("save job run");
    }

    let runs = storage
      .list_recent_job_runs(10)
      .await
      .expect("list_recent_job_runs should succeed");
    assert_eq!(runs.len(), 3);
  }

  #[tokio::test]
  async fn test_list_recent_job_runs_limit() {
    let storage = in_memory_storage().await;

    let pipeline_run = PipelineRun::new(make_pipeline("pipe"));
    storage
      .save_pipeline_run(&pipeline_run)
      .await
      .expect("save pipeline run");

    for i in 0..5 {
      let node = Node {
        name: format!("node-{i}"),
        image: "alpine:latest".to_string(),
        environment: Default::default(),
        steps: vec![format!("echo {i}")],
      };
      let job_run = JobRun::new(node);
      storage
        .save_job_run(&pipeline_run.id, &job_run)
        .await
        .expect("save job run");
    }

    let runs = storage
      .list_recent_job_runs(2)
      .await
      .expect("list_recent_job_runs with limit");
    assert_eq!(runs.len(), 2);
  }

  #[tokio::test]
  async fn test_get_pipeline_run_with_job_runs() {
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

    let mut job_run = JobRun::new(node);
    job_run.status = Status::Success;
    job_run.started_at = Some(SystemTime::now());
    job_run.ended_at = Some(SystemTime::now());

    storage
      .save_job_run(&run_id, &job_run)
      .await
      .expect("save job run");

    let loaded = storage
      .get_pipeline_run(&run_id)
      .await
      .expect("get_pipeline_run should succeed");

    assert_eq!(loaded.id, run_id);
    assert_eq!(loaded.pipeline.name, "detail-pipe");
    assert_eq!(loaded.status, Status::Success);
    assert_eq!(loaded.node_runs.len(), 1);
    assert_eq!(loaded.node_runs[0].node.name, "compile");
    assert_eq!(loaded.node_runs[0].node.image, "rust:alpine");
    assert_eq!(
      loaded.node_runs[0].node.steps,
      vec!["cargo build", "cargo test"]
    );
    assert_eq!(
      loaded.node_runs[0]
        .node
        .environment
        .get("FOO")
        .map(|s| s.as_str()),
      Some("bar")
    );
  }

  #[tokio::test]
  async fn test_get_pipeline_run_not_found() {
    let storage = in_memory_storage().await;
    let result = storage.get_pipeline_run("nonexistent-id").await;
    assert!(
      matches!(result, Err(StorageError::NotFound(_))),
      "Expected NotFound"
    );
  }

  #[tokio::test]
  async fn test_get_job_run() {
    let storage = in_memory_storage().await;

    let pipeline_run = PipelineRun::new(make_pipeline("pipe"));
    storage
      .save_pipeline_run(&pipeline_run)
      .await
      .expect("save pipeline run");

    let node = Node {
      name: "build".to_string(),
      image: "rust:alpine".to_string(),
      environment: Default::default(),
      steps: vec!["cargo build".to_string()],
    };
    let mut job_run = JobRun::new(node);
    job_run.status = Status::Success;
    let job_run_id = job_run.id.clone();

    storage
      .save_job_run(&pipeline_run.id, &job_run)
      .await
      .expect("save job run");

    let loaded = storage
      .get_job_run(&job_run_id)
      .await
      .expect("get_job_run should succeed");

    assert_eq!(loaded.id, job_run_id);
    assert_eq!(loaded.node.name, "build");
    assert_eq!(loaded.status, Status::Success);
  }

  #[tokio::test]
  async fn test_get_job_run_not_found() {
    let storage = in_memory_storage().await;
    let result = storage.get_job_run("nonexistent-id").await;
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

    let loaded = storage
      .get_pipeline_run(&run_id)
      .await
      .expect("get_pipeline_run after update");

    assert_eq!(loaded.status, Status::Aborted);
  }

  #[tokio::test]
  async fn test_pipeline_snapshot_is_independent_of_registry() {
    let storage = in_memory_storage().await;

    storage
      .save_pipeline("evolving-pipe", "name: evolving-pipe\nnodes: []")
      .await
      .expect("save v1");

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
      .save_pipeline("evolving-pipe", "name: evolving-pipe\nnodes: [{name: step-v2, image: alpine:2, environment: {}, steps: [echo v2]}]")
      .await
      .expect("save v2");

    let loaded = storage
      .get_pipeline_run(&run_v1_id)
      .await
      .expect("get_pipeline_run v1");

    assert_eq!(
      loaded.pipeline.nodes[0].name, "step-v1",
      "run snapshot should preserve the pipeline definition at the time of the run"
    );
  }
}
