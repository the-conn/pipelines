mod history;
mod registry;

use std::time::{Duration, SystemTime};

use execution::{
  Node, Pipeline,
  run::{JobRun, Status},
};
use sqlx::{
  Row, SqlitePool,
  sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use crate::StorageError;

pub struct SqliteStorage {
  pub(super) pool: SqlitePool,
}

impl SqliteStorage {
  pub async fn new(database_url: &str) -> Result<Self, StorageError> {
    let options = database_url
      .parse::<SqliteConnectOptions>()?
      .foreign_keys(true);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    let storage = Self { pool };
    storage.migrate().await?;
    Ok(storage)
  }

  async fn migrate(&self) -> Result<(), StorageError> {
    sqlx::query(
      "CREATE TABLE IF NOT EXISTS pipelines (
        name        TEXT PRIMARY KEY,
        yaml_source TEXT NOT NULL,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
      )",
    )
    .execute(&self.pool)
    .await?;

    sqlx::query(
      "CREATE TABLE IF NOT EXISTS pipeline_runs (
        id                TEXT PRIMARY KEY,
        pipeline_name     TEXT NOT NULL,
        pipeline_snapshot TEXT NOT NULL,
        status            TEXT NOT NULL,
        created_at        INTEGER NOT NULL,
        started_at        INTEGER,
        ended_at          INTEGER
      )",
    )
    .execute(&self.pool)
    .await?;

    sqlx::query(
      "CREATE TABLE IF NOT EXISTS node_runs (
        id               TEXT PRIMARY KEY,
        pipeline_run_id  TEXT NOT NULL,
        node_snapshot    TEXT NOT NULL,
        node_name        TEXT NOT NULL,
        node_image       TEXT NOT NULL,
        node_steps       TEXT NOT NULL DEFAULT '[]',
        node_environment TEXT NOT NULL DEFAULT '{}',
        status           TEXT NOT NULL,
        created_at       INTEGER NOT NULL,
        started_at       INTEGER,
        ended_at         INTEGER,
        FOREIGN KEY (pipeline_run_id) REFERENCES pipeline_runs(id) ON DELETE CASCADE
      )",
    )
    .execute(&self.pool)
    .await?;

    Ok(())
  }
}

pub(super) fn to_unix_secs(t: SystemTime) -> i64 {
  t.duration_since(SystemTime::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs() as i64
}

pub(super) fn from_unix_secs(secs: i64) -> SystemTime {
  SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64)
}

pub(super) fn parse_status(s: &str) -> Result<Status, StorageError> {
  use std::str::FromStr;
  Status::from_str(s).map_err(StorageError::Parse)
}

pub(super) fn parse_pipeline_snapshot(snapshot: &str) -> Result<Pipeline, StorageError> {
  serde_json::from_str(snapshot).map_err(|e| StorageError::Parse(e.to_string()))
}

pub(super) fn build_job_run_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<JobRun, StorageError> {
  use std::collections::HashMap;

  let steps: Vec<String> =
    serde_json::from_str(r.get("node_steps")).map_err(|e| StorageError::Parse(e.to_string()))?;
  let environment: HashMap<String, String> = serde_json::from_str(r.get("node_environment"))
    .map_err(|e| StorageError::Parse(e.to_string()))?;

  let node = Node {
    name: r.get("node_name"),
    image: r.get("node_image"),
    steps,
    environment,
  };

  Ok(JobRun {
    id: r.get("id"),
    node,
    status: parse_status(r.get("status"))?,
    created_at: from_unix_secs(r.get("created_at")),
    started_at: r.get::<Option<i64>, _>("started_at").map(from_unix_secs),
    ended_at: r.get::<Option<i64>, _>("ended_at").map(from_unix_secs),
  })
}
