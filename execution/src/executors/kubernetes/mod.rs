mod node;
mod pipeline;

use std::sync::Arc;

use async_trait::async_trait;
use config::Config;
use tracing::instrument;

use crate::{
  Executor, Node, Pipeline, PipelineRun,
  run::{JobRun, RunRecorder},
};

pub(super) const RESOURCE_PREFIX: &str = "jefferies-";

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("failed to initialize Kubernetes client: {0}")]
  ClientInit(#[from] kube::Error),
}

pub struct KubernetesExecutor {
  client: kube::Client,
}

impl KubernetesExecutor {
  pub async fn new() -> Result<Self, Error> {
    let client = kube::Client::try_default().await?;
    Ok(Self { client })
  }
}

#[async_trait]
impl Executor for KubernetesExecutor {
  #[instrument(skip(self, node, config), fields(node_name = %node.name))]
  async fn execute_node(&self, node: &Node, config: &Config) -> JobRun {
    self.run_node(node, config, None, None).await
  }

  #[instrument(skip(self, pipeline, config, recorder), fields(pipeline_name = %pipeline.name))]
  async fn execute_pipeline(
    &self,
    pipeline: &Pipeline,
    config: &Config,
    recorder: Option<Arc<dyn RunRecorder>>,
  ) -> PipelineRun {
    self.run_pipeline(pipeline, config, recorder).await
  }
}
