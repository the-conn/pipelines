use std::time::{SystemTime, SystemTimeError};

use uuid::Uuid;

use crate::node::Node;

#[derive(Debug, thiserror::Error)]
pub enum DurationError {
  #[error("Job has not finished yet")]
  NotFinished,
  #[error("Job never started")]
  NotStarted,
  #[error("System clock drifted: end time is before start time")]
  ClockDrift(#[from] SystemTimeError),
}

pub struct JobRun {
  pub node: Node,
  pub id: String,
  pub status: Status,
  pub created_at: SystemTime,
  pub started_at: Option<SystemTime>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
