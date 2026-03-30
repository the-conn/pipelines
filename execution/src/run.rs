use std::time::{SystemTime, SystemTimeError};

use async_trait::async_trait;
use serde::Serialize;
use uuid::Uuid;

use crate::{node::Node, pipeline::Pipeline};

mod serde_system_time {
  use std::time::SystemTime;

  use serde::{Serializer, ser::Error};

  pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
    let secs = t
      .duration_since(SystemTime::UNIX_EPOCH)
      .map_err(|e| S::Error::custom(e.to_string()))?
      .as_secs();
    s.serialize_u64(secs)
  }
}

mod serde_opt_system_time {
  use std::time::SystemTime;

  use serde::{Serializer, ser::Error};

  pub fn serialize<S: Serializer>(opt: &Option<SystemTime>, s: S) -> Result<S::Ok, S::Error> {
    match opt {
      None => s.serialize_none(),
      Some(t) => {
        let secs = t
          .duration_since(SystemTime::UNIX_EPOCH)
          .map_err(|e| S::Error::custom(e.to_string()))?
          .as_secs();
        s.serialize_some(&secs)
      }
    }
  }
}

#[derive(Debug, thiserror::Error)]
pub enum DurationError {
  #[error("Job has not finished yet")]
  NotFinished,
  #[error("Job never started")]
  NotStarted,
  #[error("System clock drifted: end time is before start time")]
  ClockDrift(#[from] SystemTimeError),
}

#[derive(Serialize)]
pub struct JobRun {
  pub node: Node,
  pub id: String,
  pub status: Status,
  #[serde(with = "serde_system_time")]
  pub created_at: SystemTime,
  #[serde(with = "serde_opt_system_time")]
  pub started_at: Option<SystemTime>,
  #[serde(with = "serde_opt_system_time")]
  pub ended_at: Option<SystemTime>,
}

impl JobRun {
  pub fn new(node: Node) -> Self {
    Self {
      node,
      id: Uuid::new_v4().to_string(),
      status: Status::NotStarted,
      created_at: SystemTime::now(),
      started_at: None,
      ended_at: None,
    }
  }

  pub fn execution_duration(&self) -> Result<std::time::Duration, DurationError> {
    let start = self.started_at.ok_or(DurationError::NotStarted)?;
    let end = self.ended_at.ok_or(DurationError::NotFinished)?;
    Ok(end.duration_since(start)?)
  }
}

impl Default for JobRun {
  fn default() -> Self {
    Self::new(Node::default())
  }
}

#[derive(Serialize)]
pub struct PipelineRun {
  pub pipeline: Pipeline,
  pub id: String,
  pub node_runs: Vec<JobRun>,
  pub status: Status,
  #[serde(with = "serde_system_time")]
  pub created_at: SystemTime,
  #[serde(with = "serde_opt_system_time")]
  pub started_at: Option<SystemTime>,
  #[serde(with = "serde_opt_system_time")]
  pub ended_at: Option<SystemTime>,
}

impl PipelineRun {
  pub fn new(pipeline: Pipeline) -> Self {
    Self {
      pipeline,
      id: Uuid::new_v4().to_string(),
      node_runs: Vec::new(),
      status: Status::NotStarted,
      created_at: SystemTime::now(),
      started_at: None,
      ended_at: None,
    }
  }
}

impl Default for PipelineRun {
  fn default() -> Self {
    Self::new(Pipeline::default())
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Status {
  NotStarted,
  InProgress,
  Success,
  Failure,
  Aborted,
  Skipped,
}

impl std::fmt::Display for Status {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let s = match self {
      Status::NotStarted => "Not Started",
      Status::InProgress => "In Progress",
      Status::Success => "Success",
      Status::Failure => "Failure",
      Status::Aborted => "Aborted",
      Status::Skipped => "Skipped",
    };
    write!(f, "{}", s)
  }
}

impl std::str::FromStr for Status {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "Not Started" => Ok(Status::NotStarted),
      "In Progress" => Ok(Status::InProgress),
      "Success" => Ok(Status::Success),
      "Failure" => Ok(Status::Failure),
      "Aborted" => Ok(Status::Aborted),
      "Skipped" => Ok(Status::Skipped),
      other => Err(format!(
        "Unknown status: '{other}'. Valid values are: Not Started, In Progress, Success, Failure, Aborted, Skipped"
      )),
    }
  }
}

#[async_trait]
pub trait RunRecorder: Send + Sync {
  async fn record_pipeline_run(
    &self,
    run: &PipelineRun,
  ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

  async fn record_job_run(
    &self,
    pipeline_run_id: &str,
    run: &JobRun,
  ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
