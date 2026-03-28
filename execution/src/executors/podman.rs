use std::{
  collections::HashMap,
  fs::{File, OpenOptions},
  process::Stdio,
  time::SystemTime,
};

use async_trait::async_trait;
use config::Config;
use tokio::{io::AsyncWriteExt, process::Command};
use tracing::{error, info, instrument};

use crate::{
  executors::Executor,
  node::Node,
  run::{JobRun, Status},
};

pub struct PodmanExecutor {}

fn get_log_file(config: &Config, container_name: &str) -> Option<File> {
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

  fn build_env_args(&self, env: &HashMap<String, String>) -> Vec<String> {
    env
      .iter()
      .flat_map(|(k, v)| vec!["-e".to_string(), format!("{}={}", k, v)])
      .collect()
  }

  fn generate_entrypoint_script(&self, steps: Vec<String>) -> String {
    let mut script = String::from("#!/bin/sh\nset -e\n");

    for step in steps {
      script.push_str(&format!(
        "echo \"--- Executing: {} ---\"\n",
        step.replace("\"", "\\\"")
      ));
      script.push_str(&step);
      script.push_str("\n");
    }

    script
  }
}

#[async_trait]
impl Executor for PodmanExecutor {
  #[instrument(skip(self, node, config), fields(node_name = %node.name, container_name = tracing::field::Empty, run_id = tracing::field::Empty))]
  async fn execute(&self, node: &Node, config: &Config) -> JobRun {
    let mut run = JobRun::new(node.clone());
    let container_name = format!("ci-run-{}", &run.id);
    tracing::Span::current().record("container_name", &container_name);
    tracing::Span::current().record("run_id", &run.id);

    let script = self.generate_entrypoint_script(node.steps.clone());
    let stdout_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);
    let stderr_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);

    info!("Starting container execution");

    let mut child = match Command::new("podman")
      .args(["run", "--rm", "-i", "--name", &container_name])
      .args(self.build_env_args(&node.environment))
      .arg(&node.image)
      .args(["sh", "-s"]) // Using -s to read from stdin
      .stdin(Stdio::piped())
      .stdout(stdout_handle)
      .stderr(stderr_handle)
      .spawn()
    {
      Ok(c) => c,
      Err(e) => {
        error!(error = %e, "Failed to spawn podman process");
        run.status = Status::Failure;
        run.ended_at = Some(SystemTime::now());
        return run;
      }
    };

    if let Some(mut stdin) = child.stdin.take() {
      let _ = stdin.write_all(script.as_bytes()).await;
      let _ = stdin.flush().await;
      drop(stdin);
    }

    match child.wait().await {
      Ok(status) if status.success() => {
        info!("Container execution completed successfully");
        run.status = Status::Success;
      }
      Ok(status) => {
        error!(
          exit_code = status.code().unwrap_or(-1),
          "Container execution failed"
        );
        run.status = Status::Failure;
      }
      Err(e) => {
        error!(error = %e, "Failed to wait for podman process");
        run.status = Status::Failure;
      }
    }

    run.ended_at = Some(SystemTime::now());
    run
  }
}
