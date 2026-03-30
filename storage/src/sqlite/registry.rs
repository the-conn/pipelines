use std::time::SystemTime;

use async_trait::async_trait;
use execution::{Pipeline, pipeline::PipelineSource};
use sqlx::Row;

use super::{SqliteStorage, to_unix_secs};
use crate::{PipelineRegistry, StorageError};

#[async_trait]
impl PipelineRegistry for SqliteStorage {
  async fn save_pipeline(&self, name: &str, yaml_source: &str) -> Result<(), StorageError> {
    Pipeline::from_yaml(yaml_source).map_err(|e| StorageError::Parse(e.to_string()))?;

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

  async fn load_pipeline(&self, name: &str) -> Result<Pipeline, StorageError> {
    let row = sqlx::query("SELECT yaml_source FROM pipelines WHERE name = ?")
      .bind(name)
      .fetch_optional(&self.pool)
      .await?;

    match row {
      None => Err(StorageError::NotFound(format!("pipeline '{name}'"))),
      Some(r) => {
        let yaml_source: String = r.get("yaml_source");
        let mut pipeline =
          Pipeline::from_yaml(&yaml_source).map_err(|e| StorageError::Parse(e.to_string()))?;
        pipeline.source = PipelineSource::Registry {
          name: name.to_string(),
        };
        Ok(pipeline)
      }
    }
  }

  async fn list_pipelines(&self) -> Result<Vec<String>, StorageError> {
    let rows = sqlx::query("SELECT name FROM pipelines ORDER BY updated_at DESC")
      .fetch_all(&self.pool)
      .await?;

    Ok(rows.into_iter().map(|r| r.get("name")).collect())
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
}
