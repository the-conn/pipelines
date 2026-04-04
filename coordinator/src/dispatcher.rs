use app_config::AppConfig;
use async_trait::async_trait;
use pipelines::{NodeInfo, Pipeline};
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum DispatchError {
  #[error("Dispatch failed: {0}")]
  Failed(String),
}

#[async_trait]
pub trait Dispatcher: Send + Sync {
  async fn dispatch(
    &self,
    run_id: &str,
    node: &NodeInfo,
    pipeline: &Pipeline,
    config: &AppConfig,
  ) -> Result<(), DispatchError>;

  async fn cancel_node(
    &self,
    run_id: &str,
    node_name: &str,
    config: &AppConfig,
  ) -> Result<(), DispatchError>;
}

pub struct LogDispatcher;

#[async_trait]
impl Dispatcher for LogDispatcher {
  async fn dispatch(
    &self,
    run_id: &str,
    node: &NodeInfo,
    _pipeline: &Pipeline,
    _config: &AppConfig,
  ) -> Result<(), DispatchError> {
    info!(run_id, node_name = %node.name, image = %node.image, "Dispatching node");
    Ok(())
  }

  async fn cancel_node(
    &self,
    run_id: &str,
    node_name: &str,
    _config: &AppConfig,
  ) -> Result<(), DispatchError> {
    info!(run_id, node_name, "Cancelling node");
    Ok(())
  }
}
