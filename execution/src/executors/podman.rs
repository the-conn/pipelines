use std::{
  collections::HashMap,
  fs::{File, OpenOptions},
  process::{Command, Stdio},
  time::SystemTime,
};

use async_trait::async_trait;
use config::Config;
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
  #[instrument(skip(self, node, config), fields(container_name = tracing::field::Empty, run_id = tracing::field::Empty))]
  fn start_run(&self, node: &Node, config: &Config) -> (JobRun, String) {
    let mut run = JobRun::new(node.clone());
    let container_name = format!("ci-run-{}", &run.id[..8]);

    tracing::Span::current().record("container_name", &container_name);
    tracing::Span::current().record("run_id", &run.id);

    run.started_at = Some(SystemTime::now());
    run.status = Status::InProgress;

    let stdout_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);

    let stderr_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);

    match Command::new("podman")
      .args(["run", "-d", "--name", &container_name])
      .args(self.build_env_args(&node.environment))
      .arg(&node.image)
      .args(["tail", "-f", "/dev/null"])
      .stdout(stdout_handle)
      .stderr(stderr_handle)
      .status()
    {
      Ok(status) if status.success() => {
        info!("Successfully started container");
      }
      Ok(status) => {
        return (
          self.fail_run(
            run,
            format!("Podman failed with exit code: {:?}", status.code()),
            &container_name,
          ),
          container_name,
        );
      }
      Err(e) => {
        return (
          self.fail_run(
            run,
            format!(
              "Failed to execute podman command to start container {}: {}",
              container_name, e
            ),
            &container_name,
          ),
          container_name,
        );
      }
    }
    (run, container_name)
  }

  #[instrument(skip(self, run), fields(error = %reason, container_name = %container_name))]
  fn fail_run(&self, mut run: JobRun, reason: String, container_name: &str) -> JobRun {
    error!("Job execution failed");
    run.status = Status::Failure;
    run.ended_at = Some(SystemTime::now());
    self.cleanup(container_name);
    run
  }

  #[instrument(skip(self), fields(container_name = %container_name))]
  fn cleanup(&self, container_name: &str) {
    match Command::new("podman")
      .args(["rm", "-f", container_name])
      .status()
    {
      Ok(status) if status.success() => {
        info!("Successfully removed container");
      }
      Ok(status) => {
        error!(exit_code = ?status.code(), "Failed to remove container");
      }
      Err(e) => {
        error!(error = %e, "Failed to execute podman command to remove container");
      }
    }
  }

  fn build_env_args(&self, env: &HashMap<String, String>) -> Vec<String> {
    env
      .iter()
      .flat_map(|(k, v)| vec!["-e".to_string(), format!("{}={}", k, v)])
      .collect()
  }

  #[instrument(skip(self, config, run), fields(cmd = %cmd, container_name = %container_name))]
  fn run_step(&self, cmd: &str, container_name: &str, run: JobRun, config: &Config) -> JobRun {
    let stdout_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);

    let stderr_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);

    match Command::new("podman")
      .args(["exec", &container_name, "sh", "-c", cmd])
      .stdout(stdout_handle)
      .stderr(stderr_handle)
      .status()
    {
      Ok(status) if status.success() => {
        info!("Command executed successfully");
        run
      }
      Ok(status) => {
        return self.fail_run(
          run,
          format!(
            "Command '{}' failed with exit code: {:?}",
            cmd,
            status.code()
          ),
          container_name,
        );
      }
      Err(e) => {
        return self.fail_run(
          run,
          format!(
            "Failed to execute command '{}' in container {}: {}",
            cmd, container_name, e
          ),
          container_name,
        );
      }
    }
  }
}

#[async_trait]
impl Executor for PodmanExecutor {
  #[instrument(skip(self, node, config), fields(node_name = %node.name))]
  async fn execute(&self, node: &Node, config: &Config) -> JobRun {
    let (mut run, container_name) = self.start_run(node, config);
    if run.status == Status::Failure {
      return run;
    }

    for cmd in &node.steps {
      run = self.run_step(cmd, &container_name, run, config);
      if run.status == Status::Failure {
        return run;
      }
    }

    // 3. Finalize
    run.status = Status::Success;
    run.ended_at = Some(SystemTime::now());
    self.cleanup(&container_name);
    run
  }
}
