pub mod podman;

use async_trait::async_trait;
use config::Config;
pub use podman::PodmanExecutor;

use crate::{node::Node, pipeline::Pipeline, run::{JobRun, PipelineRun}};

#[async_trait]
pub trait Executor {
  async fn execute_node(&self, node: &Node, config: &Config) -> JobRun;
  async fn execute_pipeline(&self, pipeline: &Pipeline, config: &Config) -> PipelineRun;
}
