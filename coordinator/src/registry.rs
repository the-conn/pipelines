use std::{collections::HashMap, sync::Arc};

use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::message::CoordinatorMessage;

#[derive(Debug, Error)]
pub enum RegistryError {
  #[error("Run not found: {0}")]
  NotFound(String),
  #[error("Send error: {0}")]
  SendError(String),
}

#[derive(Debug, Default)]
pub struct RunRegistry {
  senders: tokio::sync::Mutex<HashMap<String, mpsc::Sender<CoordinatorMessage>>>,
}

impl RunRegistry {
  pub fn new() -> Arc<Self> {
    Arc::new(Self::default())
  }

  pub async fn register(&self, run_id: String, sender: mpsc::Sender<CoordinatorMessage>) {
    self.senders.lock().await.insert(run_id.clone(), sender);
    info!(run_id, "Registered coordinator for run");
  }

  pub async fn send(&self, run_id: &str, message: CoordinatorMessage) -> Result<(), RegistryError> {
    let guard = self.senders.lock().await;
    let sender = guard
      .get(run_id)
      .ok_or_else(|| RegistryError::NotFound(run_id.to_string()))?;
    sender
      .send(message)
      .await
      .map_err(|e| RegistryError::SendError(e.to_string()))
  }

  pub async fn deregister(&self, run_id: &str) {
    if self.senders.lock().await.remove(run_id).is_some() {
      info!(run_id, "Deregistered coordinator for run");
    } else {
      warn!(run_id, "Attempted to deregister unknown run");
    }
  }
}
