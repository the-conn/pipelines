pub mod kubernetes;
pub mod podman;

use std::sync::Arc;

use async_trait::async_trait;
use config::Config;
pub use kubernetes::KubernetesExecutor;
pub use podman::PodmanExecutor;

use crate::{
  node::Node,
  pipeline::Pipeline,
  run::{JobRun, PipelineRun, RunRecorder},
};

#[async_trait]
pub trait Executor {
  async fn execute_node(&self, node: &Node, config: &Config) -> JobRun;
  async fn execute_pipeline(
    &self,
    pipeline: &Pipeline,
    config: &Config,
    recorder: Option<Arc<dyn RunRecorder>>,
  ) -> PipelineRun;
}

pub(crate) fn generate_entrypoint_script(steps: Vec<String>) -> String {
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
