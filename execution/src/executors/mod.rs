pub mod podman;

use async_trait::async_trait;
use config::Config;
pub use podman::PodmanExecutor;

use crate::{node::Node, run::JobRun};
#[async_trait]
pub trait Executor {
  async fn execute(&self, node: &Node, config: &Config) -> JobRun;
}
