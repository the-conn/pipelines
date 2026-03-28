use std::time::SystemTime;

use async_trait::async_trait;
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

use execution::run::{JobRun, PipelineRun};

use crate::{Storage, StorageError};

pub struct SqliteStorage {
  pool: SqlitePool,
}

impl SqliteStorage {
  pub async fn new(database_url: &str) -> Result<Self, StorageError> {
    let pool = SqlitePoolOptions::new()
      .connect(database_url)
      .await?;
    let storage = Self { pool };
    storage.migrate().await?;
    Ok(storage)
  }

  async fn migrate(&self) -> Result<(), StorageError> {
    sqlx::query(
      "CREATE TABLE IF NOT EXISTS pipeline_runs (
        id TEXT PRIMARY KEY,
        pipeline_name TEXT NOT NULL,
        status TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        started_at INTEGER,
        ended_at INTEGER
      )",
    )
    .execute(&self.pool)
    .await?;

    sqlx::query(
      "CREATE TABLE IF NOT EXISTS node_runs (
        id TEXT PRIMARY KEY,
        pipeline_run_id TEXT NOT NULL,
        node_name TEXT NOT NULL,
        node_image TEXT NOT NULL,
        status TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        started_at INTEGER,
        ended_at INTEGER,
        FOREIGN KEY (pipeline_run_id) REFERENCES pipeline_runs(id)
      )",
    )
    .execute(&self.pool)
    .await?;

    Ok(())
  }
}

fn to_unix_secs(t: SystemTime) -> i64 {
  t.duration_since(SystemTime::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs() as i64
}

#[async_trait]
impl Storage for SqliteStorage {
  async fn save_pipeline_run(
    &self,
    pipeline_name: &str,
    run: &PipelineRun,
  ) -> Result<(), StorageError> {
    let status = run.status.to_string();
    let created_at = to_unix_secs(run.created_at);
    let started_at = run.started_at.map(to_unix_secs);
    let ended_at = run.ended_at.map(to_unix_secs);

    sqlx::query(
      "INSERT OR REPLACE INTO pipeline_runs
        (id, pipeline_name, status, created_at, started_at, ended_at)
       VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&run.id)
    .bind(pipeline_name)
    .bind(status)
    .bind(created_at)
    .bind(started_at)
    .bind(ended_at)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  async fn save_node_run(
    &self,
    pipeline_run_id: &str,
    run: &JobRun,
  ) -> Result<(), StorageError> {
    let status = run.status.to_string();
    let created_at = to_unix_secs(run.created_at);
    let started_at = run.started_at.map(to_unix_secs);
    let ended_at = run.ended_at.map(to_unix_secs);

    sqlx::query(
      "INSERT OR REPLACE INTO node_runs
        (id, pipeline_run_id, node_name, node_image, status, created_at, started_at, ended_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&run.id)
    .bind(pipeline_run_id)
    .bind(&run.node.name)
    .bind(&run.node.image)
    .bind(status)
    .bind(created_at)
    .bind(started_at)
    .bind(ended_at)
    .execute(&self.pool)
    .await?;

    Ok(())
  }
}
