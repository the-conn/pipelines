use async_trait::async_trait;
use execution::run::{JobRun, PipelineRun, RunRecorder, Status};
use sqlx::Row;

use super::{
  SqliteStorage, build_job_run_from_row, from_unix_secs, parse_pipeline_snapshot, parse_status,
  to_unix_secs,
};
use crate::{RunHistory, StorageError};

#[async_trait]
impl RunRecorder for SqliteStorage {
  async fn record_pipeline_run(
    &self,
    run: &PipelineRun,
  ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    RunHistory::save_pipeline_run(self, run)
      .await
      .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
  }

  async fn record_job_run(
    &self,
    pipeline_run_id: &str,
    run: &JobRun,
  ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    RunHistory::save_job_run(self, pipeline_run_id, run)
      .await
      .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
  }
}

#[async_trait]
impl RunHistory for SqliteStorage {
  async fn save_pipeline_run(&self, run: &PipelineRun) -> Result<(), StorageError> {
    let status = run.status.to_string();
    let created_at = to_unix_secs(run.created_at);
    let started_at = run.started_at.map(to_unix_secs);
    let ended_at = run.ended_at.map(to_unix_secs);
    let pipeline_snapshot =
      serde_json::to_string(&run.pipeline).map_err(|e| StorageError::Parse(e.to_string()))?;

    sqlx::query(
      "INSERT INTO pipeline_runs
        (id, pipeline_name, pipeline_snapshot, status, created_at, started_at, ended_at)
       VALUES (?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT(id) DO UPDATE SET
         pipeline_name     = excluded.pipeline_name,
         pipeline_snapshot = excluded.pipeline_snapshot,
         status            = excluded.status,
         created_at        = excluded.created_at,
         started_at        = excluded.started_at,
         ended_at          = excluded.ended_at",
    )
    .bind(&run.id)
    .bind(&run.pipeline.name)
    .bind(pipeline_snapshot)
    .bind(status)
    .bind(created_at)
    .bind(started_at)
    .bind(ended_at)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  async fn save_job_run(&self, pipeline_run_id: &str, run: &JobRun) -> Result<(), StorageError> {
    let status = run.status.to_string();
    let created_at = to_unix_secs(run.created_at);
    let started_at = run.started_at.map(to_unix_secs);
    let ended_at = run.ended_at.map(to_unix_secs);
    let node_snapshot =
      serde_json::to_string(&run.node).map_err(|e| StorageError::Parse(e.to_string()))?;
    let steps_json =
      serde_json::to_string(&run.node.steps).map_err(|e| StorageError::Parse(e.to_string()))?;
    let env_json = serde_json::to_string(&run.node.environment)
      .map_err(|e| StorageError::Parse(e.to_string()))?;

    sqlx::query(
      "INSERT OR REPLACE INTO node_runs
        (id, pipeline_run_id, node_snapshot, node_name, node_image, node_steps, node_environment,
         status, created_at, started_at, ended_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&run.id)
    .bind(pipeline_run_id)
    .bind(node_snapshot)
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

  async fn get_pipeline_run(&self, run_id: &str) -> Result<PipelineRun, StorageError> {
    let run_row = sqlx::query(
      "SELECT id, pipeline_name, pipeline_snapshot, status, created_at, started_at, ended_at
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

    let node_runs = node_rows
      .into_iter()
      .map(|r| build_job_run_from_row(&r))
      .collect::<Result<Vec<_>, _>>()?;

    let pipeline = parse_pipeline_snapshot(run_row.get("pipeline_snapshot"))?;

    Ok(PipelineRun {
      id: run_row.get("id"),
      pipeline,
      node_runs,
      status: parse_status(run_row.get("status"))?,
      created_at: from_unix_secs(run_row.get("created_at")),
      started_at: run_row
        .get::<Option<i64>, _>("started_at")
        .map(from_unix_secs),
      ended_at: run_row
        .get::<Option<i64>, _>("ended_at")
        .map(from_unix_secs),
    })
  }

  async fn get_job_run(&self, job_run_id: &str) -> Result<JobRun, StorageError> {
    let row = sqlx::query(
      "SELECT id, node_name, node_image, node_steps, node_environment,
              status, created_at, started_at, ended_at
       FROM node_runs WHERE id = ?",
    )
    .bind(job_run_id)
    .fetch_optional(&self.pool)
    .await?;

    match row {
      None => Err(StorageError::NotFound(format!("job run '{job_run_id}'"))),
      Some(r) => build_job_run_from_row(&r),
    }
  }

  async fn list_recent_pipeline_runs(&self, limit: i64) -> Result<Vec<PipelineRun>, StorageError> {
    let rows = sqlx::query(
      "SELECT id, pipeline_name, pipeline_snapshot, status, created_at, started_at, ended_at
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
        let pipeline = parse_pipeline_snapshot(r.get("pipeline_snapshot"))?;
        Ok(PipelineRun {
          id: r.get("id"),
          pipeline,
          node_runs: Vec::new(),
          status: parse_status(r.get("status"))?,
          created_at: from_unix_secs(r.get("created_at")),
          started_at: r.get::<Option<i64>, _>("started_at").map(from_unix_secs),
          ended_at: r.get::<Option<i64>, _>("ended_at").map(from_unix_secs),
        })
      })
      .collect()
  }

  async fn list_recent_job_runs(&self, limit: i64) -> Result<Vec<JobRun>, StorageError> {
    let rows = sqlx::query(
      "SELECT id, node_name, node_image, node_steps, node_environment,
              status, created_at, started_at, ended_at
       FROM node_runs
       ORDER BY created_at DESC
       LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&self.pool)
    .await?;

    rows.iter().map(build_job_run_from_row).collect()
  }

  async fn list_pipeline_runs_by_name(
    &self,
    pipeline_name: &str,
  ) -> Result<Vec<PipelineRun>, StorageError> {
    let rows = sqlx::query(
      "SELECT id, pipeline_name, pipeline_snapshot, status, created_at, started_at, ended_at
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
        let pipeline = parse_pipeline_snapshot(r.get("pipeline_snapshot"))?;
        Ok(PipelineRun {
          id: r.get("id"),
          pipeline,
          node_runs: Vec::new(),
          status: parse_status(r.get("status"))?,
          created_at: from_unix_secs(r.get("created_at")),
          started_at: r.get::<Option<i64>, _>("started_at").map(from_unix_secs),
          ended_at: r.get::<Option<i64>, _>("ended_at").map(from_unix_secs),
        })
      })
      .collect()
  }

  async fn update_run_status(&self, run_id: &str, status: Status) -> Result<(), StorageError> {
    sqlx::query("UPDATE pipeline_runs SET status = ? WHERE id = ?")
      .bind(status.to_string())
      .bind(run_id)
      .execute(&self.pool)
      .await?;

    Ok(())
  }
}
