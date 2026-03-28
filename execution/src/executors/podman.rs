use std::{
  collections::HashMap,
  fs::{File, OpenOptions},
  path::Path,
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
  pipeline::Pipeline,
  run::{JobRun, PipelineRun, Status},
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

  /// Execute a node, optionally mounting a shared workspace directory at `/workspace`.
  async fn execute_node(
    &self,
    node: &Node,
    config: &Config,
    workspace: Option<&Path>,
  ) -> JobRun {
    let mut run = JobRun::new(node.clone());
    let container_name = format!("ci-run-{}", &run.id);

    let script = self.generate_entrypoint_script(node.steps.clone());
    let stdout_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);
    let stderr_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);

    info!(node_name = %node.name, container_name = %container_name, "Starting container execution");

    let mut cmd = Command::new("podman");
    cmd
      .args(["run", "--rm", "-i", "--name", &container_name])
      .args(self.build_env_args(&node.environment));

    if let Some(ws) = workspace {
      cmd.args(["-v", &format!("{}:/workspace", ws.display())]);
    }

    cmd
      .arg(&node.image)
      .args(["sh", "-s"]) // Using -s to read from stdin
      .stdin(Stdio::piped())
      .stdout(stdout_handle)
      .stderr(stderr_handle);

    let mut child = match cmd.spawn() {
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

  /// Execute a pipeline: run each node sequentially, sharing a workspace volume and
  /// propagating variables exported via `/workspace/.pipeline-env` to subsequent nodes.
  #[instrument(skip(self, pipeline, config), fields(pipeline_name = %pipeline.name, pipeline_run_id = tracing::field::Empty))]
  pub async fn execute_pipeline(&self, pipeline: &Pipeline, config: &Config) -> PipelineRun {
    let mut pipeline_run = PipelineRun::new();
    tracing::Span::current().record("pipeline_run_id", &pipeline_run.id);

    // Determine workspace directory under the configured runs_dir (or system temp).
    let workspace = {
      let base = config
        .podman_config
        .as_ref()
        .and_then(|pc| pc.runs_dir.as_ref())
        .cloned()
        .unwrap_or_else(std::env::temp_dir);
      base.join(format!("pipeline-{}", pipeline_run.id))
    };

    if let Err(e) = std::fs::create_dir_all(&workspace) {
      error!(error = %e, workspace = %workspace.display(), "Failed to create pipeline workspace");
      pipeline_run.status = Status::Failure;
      return pipeline_run;
    }

    info!(workspace = %workspace.display(), "Pipeline workspace created");

    // Environment variables accumulated from previous nodes via .pipeline-env.
    let mut accumulated_env: HashMap<String, String> = HashMap::new();

    for node in &pipeline.nodes {
      // Merge accumulated env into the node's own environment.
      // The node's explicitly declared vars take priority over propagated ones.
      let mut merged_env = accumulated_env.clone();
      for (k, v) in &node.environment {
        merged_env.insert(k.clone(), v.clone());
      }

      let node_with_env = Node {
        name: node.name.clone(),
        image: node.image.clone(),
        environment: merged_env,
        steps: node.steps.clone(),
      };

      let run = self
        .execute_node(&node_with_env, config, Some(&workspace))
        .await;
      let success = run.status == Status::Success;
      pipeline_run.node_runs.push(run);

      if !success {
        pipeline_run.status = Status::Failure;
        return pipeline_run;
      }

      // Read variables exported by the node and accumulate them for subsequent nodes.
      let env_file = workspace.join(".pipeline-env");
      if env_file.exists() {
        match std::fs::read_to_string(&env_file) {
          Ok(content) => {
            for line in content.lines() {
              let line = line.trim();
              if line.is_empty() || line.starts_with('#') {
                continue;
              }
              if let Some((key, value)) = line.split_once('=') {
                accumulated_env.insert(key.trim().to_string(), value.trim().to_string());
              }
            }
          }
          Err(e) => {
            error!(error = %e, "Failed to read .pipeline-env file");
          }
        }
      }
    }

    pipeline_run.status = Status::Success;
    pipeline_run
  }
}

#[async_trait]
impl Executor for PodmanExecutor {
  #[instrument(skip(self, node, config), fields(node_name = %node.name, container_name = tracing::field::Empty, run_id = tracing::field::Empty))]
  async fn execute(&self, node: &Node, config: &Config) -> JobRun {
    let run = self.execute_node(node, config, None).await;
    tracing::Span::current().record("container_name", format!("ci-run-{}", &run.id));
    tracing::Span::current().record("run_id", &run.id);
    run
  }
}

