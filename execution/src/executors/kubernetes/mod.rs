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

pub struct KubernetesExecutor {
  client: kube::Client,
}

impl KubernetesExecutor {
  pub async fn new() -> Result<Self, kube::Error> {
    let client = kube::Client::try_default().await?;
    Ok(Self { client })
  }
}

pub(super) fn generate_entrypoint_script(steps: Vec<String>) -> String {
  let mut script = String::from("#!/bin/sh\nset -e\n");

  for step in steps {
    script.push_str(&format!(
      "echo \"--- Executing: {} ---\"\n",
      step.replace('"', "\\\"")
    ));
    script.push_str(&step);
    script.push('\n');
  }

  script
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_generate_entrypoint_script_single_step() {
    let script = generate_entrypoint_script(vec!["echo hello".to_string()]);
    assert!(script.starts_with("#!/bin/sh\nset -e\n"));
    assert!(script.contains("echo hello"));
    assert!(script.contains("--- Executing: echo hello ---"));
  }

  #[test]
  fn test_generate_entrypoint_script_multiple_steps() {
    let script =
      generate_entrypoint_script(vec!["echo first".to_string(), "echo second".to_string()]);
    assert!(script.contains("echo first"));
    assert!(script.contains("echo second"));
  }

  #[test]
  fn test_generate_entrypoint_script_escapes_quotes() {
    let script = generate_entrypoint_script(vec!["echo \"hello world\"".to_string()]);
    assert!(script.contains("echo \\\"hello world\\\""));
  }
}
