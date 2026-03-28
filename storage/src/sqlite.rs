use std::{
  collections::HashMap,
  str::FromStr,
  time::{Duration, SystemTime},
};

use async_trait::async_trait;
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions};

use execution::{
  Node,
  run::{JobRun, PipelineRun, Status},
};

use crate::{PipelineDefinition, PipelineRunDetails, PipelineRunSummary, Storage, StorageError};

pub struct SqliteStorage {
  pool: SqlitePool,
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
        id            TEXT PRIMARY KEY,
        pipeline_name TEXT NOT NULL,
        status        TEXT NOT NULL,
        created_at    INTEGER NOT NULL,
        started_at    INTEGER,
        ended_at      INTEGER
      )",
    )
    .execute(&self.pool)
    .await?;

    sqlx::query(
      "CREATE TABLE IF NOT EXISTS node_runs (
        id               TEXT PRIMARY KEY,
        pipeline_run_id  TEXT NOT NULL,
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

// ── Helpers ───────────────────────────────────────────────────────────────────

fn to_unix_secs(t: SystemTime) -> i64 {
  t.duration_since(SystemTime::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs() as i64
}

fn from_unix_secs(secs: i64) -> SystemTime {
  SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64)
}

fn parse_status(s: &str) -> Result<Status, StorageError> {
  Status::from_str(s).map_err(StorageError::Parse)
}

// ── Storage impl ──────────────────────────────────────────────────────────────

#[async_trait]
impl Storage for SqliteStorage {
  // ── Write ──────────────────────────────────────────────────────────────────

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
    let steps_json = serde_json::to_string(&run.node.steps)
      .map_err(|e| StorageError::Parse(e.to_string()))?;
    let env_json = serde_json::to_string(&run.node.environment)
      .map_err(|e| StorageError::Parse(e.to_string()))?;

    sqlx::query(
      "INSERT OR REPLACE INTO node_runs
        (id, pipeline_run_id, node_name, node_image, node_steps, node_environment,
         status, created_at, started_at, ended_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&run.id)
    .bind(pipeline_run_id)
    .bind(&run.node.name)
    .bind(&run.node.image)
    .bind(steps_json)
    .bind(env_json)
    .bind(status)
    .bind(created_at)
    .bind(started_at)
    .bind(ended_at)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  // ── Pipeline Registry ──────────────────────────────────────────────────────

  async fn register_pipeline(
    &self,
    name: &str,
    yaml_source: &str,
  ) -> Result<(), StorageError> {
    let now = to_unix_secs(SystemTime::now());

    sqlx::query(
      "INSERT INTO pipelines (name, yaml_source, created_at, updated_at)
       VALUES (?, ?, ?, ?)
       ON CONFLICT(name) DO UPDATE SET yaml_source = excluded.yaml_source,
                                       updated_at  = excluded.updated_at",
    )
    .bind(name)
    .bind(yaml_source)
    .bind(now)
    .bind(now)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  async fn get_pipeline(&self, name: &str) -> Result<PipelineDefinition, StorageError> {
    let row = sqlx::query(
      "SELECT name, yaml_source, created_at, updated_at FROM pipelines WHERE name = ?",
    )
    .bind(name)
    .fetch_optional(&self.pool)
    .await?;

    match row {
      None => Err(StorageError::NotFound(format!("pipeline '{name}'"))),
      Some(r) => Ok(PipelineDefinition {
        name: r.get("name"),
        yaml_source: r.get("yaml_source"),
        created_at: from_unix_secs(r.get("created_at")),
        updated_at: from_unix_secs(r.get("updated_at")),
      }),
    }
  }

  async fn list_pipelines(&self) -> Result<Vec<PipelineDefinition>, StorageError> {
    let rows = sqlx::query(
      "SELECT name, yaml_source, created_at, updated_at FROM pipelines ORDER BY name",
    )
    .fetch_all(&self.pool)
    .await?;

    rows
      .into_iter()
      .map(|r| {
        Ok(PipelineDefinition {
          name: r.get("name"),
          yaml_source: r.get("yaml_source"),
          created_at: from_unix_secs(r.get("created_at")),
          updated_at: from_unix_secs(r.get("updated_at")),
        })
      })
      .collect()
  }

  async fn delete_pipeline(&self, name: &str) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM pipeline_runs WHERE pipeline_name = ?")
      .bind(name)
      .execute(&self.pool)
      .await?;

    sqlx::query("DELETE FROM pipelines WHERE name = ?")
      .bind(name)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  // ── Execution History ──────────────────────────────────────────────────────

  async fn list_runs(&self, limit: i64) -> Result<Vec<PipelineRunSummary>, StorageError> {
    let rows = sqlx::query(
      "SELECT id, pipeline_name, status, created_at, started_at, ended_at
       FROM pipeline_runs
       ORDER BY created_at DESC
       LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&self.pool)
    .await?;

    rows
      .into_iter()
      .map(|r| {
        Ok(PipelineRunSummary {
          id: r.get("id"),
          pipeline_name: r.get("pipeline_name"),
          status: parse_status(r.get("status"))?,
          created_at: from_unix_secs(r.get("created_at")),
          started_at: r.get::<Option<i64>, _>("started_at").map(from_unix_secs),
          ended_at: r.get::<Option<i64>, _>("ended_at").map(from_unix_secs),
        })
      })
      .collect()
  }

  async fn get_runs_by_pipeline(
    &self,
    pipeline_name: &str,
  ) -> Result<Vec<PipelineRunSummary>, StorageError> {
    let rows = sqlx::query(
      "SELECT id, pipeline_name, status, created_at, started_at, ended_at
       FROM pipeline_runs
       WHERE pipeline_name = ?
       ORDER BY created_at DESC",
    )
    .bind(pipeline_name)
    .fetch_all(&self.pool)
    .await?;

    rows
      .into_iter()
      .map(|r| {
        Ok(PipelineRunSummary {
          id: r.get("id"),
          pipeline_name: r.get("pipeline_name"),
          status: parse_status(r.get("status"))?,
          created_at: from_unix_secs(r.get("created_at")),
          started_at: r.get::<Option<i64>, _>("started_at").map(from_unix_secs),
          ended_at: r.get::<Option<i64>, _>("ended_at").map(from_unix_secs),
        })
      })
      .collect()
  }

  async fn get_run_details(&self, run_id: &str) -> Result<PipelineRunDetails, StorageError> {
    let run_row = sqlx::query(
      "SELECT id, pipeline_name, status, created_at, started_at, ended_at
       FROM pipeline_runs WHERE id = ?",
    )
    .bind(run_id)
    .fetch_optional(&self.pool)
    .await?;

    let run_row = match run_row {
      None => return Err(StorageError::NotFound(format!("pipeline run '{run_id}'"))),
      Some(r) => r,
    };

    let node_rows = sqlx::query(
      "SELECT id, node_name, node_image, node_steps, node_environment,
              status, created_at, started_at, ended_at
       FROM node_runs WHERE pipeline_run_id = ?
       ORDER BY created_at ASC",
    )
    .bind(run_id)
    .fetch_all(&self.pool)
    .await?;

    let mut node_runs = Vec::with_capacity(node_rows.len());
    for r in node_rows {
      let steps: Vec<String> = serde_json::from_str(r.get("node_steps"))
        .map_err(|e| StorageError::Parse(e.to_string()))?;
      let environment: HashMap<String, String> =
        serde_json::from_str(r.get("node_environment"))
          .map_err(|e| StorageError::Parse(e.to_string()))?;

      let node = Node {
        name: r.get("node_name"),
        image: r.get("node_image"),
        steps,
        environment,
      };

      node_runs.push(JobRun {
        id: r.get("id"),
        node,
        status: parse_status(r.get("status"))?,
        created_at: from_unix_secs(r.get("created_at")),
        started_at: r.get::<Option<i64>, _>("started_at").map(from_unix_secs),
        ended_at: r.get::<Option<i64>, _>("ended_at").map(from_unix_secs),
      });
    }

    Ok(PipelineRunDetails {
      id: run_row.get("id"),
      pipeline_name: run_row.get("pipeline_name"),
      status: parse_status(run_row.get("status"))?,
      created_at: from_unix_secs(run_row.get("created_at")),
      started_at: run_row
        .get::<Option<i64>, _>("started_at")
        .map(from_unix_secs),
      ended_at: run_row
        .get::<Option<i64>, _>("ended_at")
        .map(from_unix_secs),
      node_runs,
    })
  }

  async fn update_run_status(
    &self,
    run_id: &str,
    status: Status,
  ) -> Result<(), StorageError> {
    sqlx::query("UPDATE pipeline_runs SET status = ? WHERE id = ?")
      .bind(status.to_string())
      .bind(run_id)
      .execute(&self.pool)
      .await?;

    Ok(())
  }
}
