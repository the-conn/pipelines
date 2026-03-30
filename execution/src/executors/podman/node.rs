use std::{path::Path, process::Stdio, time::SystemTime};

use config::Config;
use tokio::{io::AsyncWriteExt, process::Command};
use tracing::{error, info, instrument};

use super::{PodmanExecutor, get_log_file};
use crate::{
  node::Node,
  run::{JobRun, Status},
};

impl PodmanExecutor {
  #[instrument(skip(self, node, config, workspace, existing_run), fields(node_name = %node.name, container_name = tracing::field::Empty, run_id = tracing::field::Empty))]
  pub(super) async fn run_node(
    &self,
    node: &Node,
    config: &Config,
    workspace: Option<&Path>,
    existing_run: Option<JobRun>,
  ) -> JobRun {
    let mut run = existing_run
      .map(|r| JobRun {
        node: node.clone(),
        ..r
      })
      .unwrap_or_else(|| JobRun::new(node.clone()));
    let container_name = format!("ci-run-{}", &run.id);
    tracing::Span::current().record("container_name", &container_name);
    tracing::Span::current().record("run_id", &run.id);

    let script = crate::executors::generate_entrypoint_script(node.steps.clone());
    let stdout_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);
    let stderr_handle = get_log_file(config, &container_name)
      .map(Stdio::from)
      .unwrap_or_else(Stdio::null);

    info!("Starting container execution");

    let mut cmd = Command::new("podman");
    cmd
      .args(["run", "--rm", "-i", "--name", &container_name])
      .args(self.build_env_args(&node.environment));

    if let Some(ws) = workspace {
      match ws.canonicalize() {
        Ok(abs_ws) => {
          cmd.args(["-v", &format!("{}:/workspace", abs_ws.display())]);
        }
        Err(e) => {
          error!(error = %e, workspace = %ws.display(), "Failed to resolve workspace path");
          return fail_run_early(run);
        }
      }
    }

    cmd
      .arg(&node.image)
      .args(["sh", "-s"])
      .stdin(Stdio::piped())
      .stdout(stdout_handle)
      .stderr(stderr_handle);

    let mut child = match cmd.spawn() {
      Ok(c) => c,
      Err(e) => {
        error!(error = %e, "Failed to spawn podman process");
        return fail_run_early(run);
      }
    };

    run.started_at = Some(SystemTime::now());

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

fn fail_run_early(mut run: JobRun) -> JobRun {
  run.status = Status::Failure;
  run.started_at = Some(SystemTime::now());
  run.ended_at = Some(SystemTime::now());
  run
}
