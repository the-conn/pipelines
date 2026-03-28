mod node;
mod pipeline;

use std::{
  collections::HashMap,
  fs::{File, OpenOptions},
  process::Stdio,
};

use async_trait::async_trait;
use config::Config;
use tokio::process::Command;
use tracing::{error, instrument};

use crate::{Executor, Node, Pipeline, PipelineRun, run::JobRun};

pub struct PodmanExecutor {}

pub(super) fn get_log_file(config: &Config, container_name: &str) -> Option<File> {
  let runs_dir = config.podman_config.as_ref()?.runs_dir.as_ref()?;
  let file_path = runs_dir.join(format!("{}.log", container_name));

  if let Err(e) = std::fs::create_dir_all(runs_dir) {
    error!(runs_dir = %runs_dir.display(), error = %e, "Failed to create runs directory");
    return None;
  }

  match OpenOptions::new()
    .create(true)
    .append(true)
    .open(&file_path)
  {
    Ok(f) => Some(f),
    Err(e) => {
      error!(error = %e, log_path = %file_path.display(), container_name = %container_name, "Failed to open log file");
      None
    }
  }
}

impl PodmanExecutor {
  pub async fn is_available(&self) -> bool {
    Command::new("podman")
      .arg("--version")
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .await
      .map(|s| s.success())
      .unwrap_or(false)
  }

  pub(super) fn build_env_args(&self, env: &HashMap<String, String>) -> Vec<String> {
    env
      .iter()
      .flat_map(|(k, v)| vec!["-e".to_string(), format!("{}={}", k, v)])
      .collect()
  }

  pub(super) fn generate_entrypoint_script(&self, steps: Vec<String>) -> String {
    let mut script = String::from("#!/bin/sh\nset -e\n");

    for step in steps {
      script.push_str(&format!(
        "echo \"--- Executing: {} ---\"\n",
        step.replace("\"", "\\\"")
      ));
      script.push_str(&step);
      script.push('\n');
    }

    script
  }
}

#[async_trait]
impl Executor for PodmanExecutor {
  #[instrument(skip(self, node, config), fields(node_name = %node.name))]
  async fn execute_node(&self, node: &Node, config: &Config) -> JobRun {
    self.run_node(node, config, None).await
  }

  #[instrument(skip(self, pipeline, config), fields(pipeline_name = %pipeline.name))]
  async fn execute_pipeline(&self, pipeline: &Pipeline, config: &Config) -> PipelineRun {
    self.run_pipeline(pipeline, config).await
  }
}
