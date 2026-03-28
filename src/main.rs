use config::Config;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

fn setup_tracing() {
  let subscriber = FmtSubscriber::builder()
    .with_max_level(Level::INFO)
    .with_env_filter("server=info,execution=info,config=info,pipelines=info")
    .finish();

  tracing::subscriber::set_global_default(subscriber).expect("Setting default subscriber failed");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  setup_tracing();

  let config = Config::local();
  info!(config = ?config, "Loaded local configuration");

  Ok(())
}

#[cfg(test)]
mod tests {
  use std::{collections::HashMap, fs};

  use execution::{Executor, Node, PodmanExecutor, run::Status};

  use super::*;

  #[tokio::test]
  async fn test_podman_executor_integration() {
    let tmp_dir = std::env::temp_dir().join("ci_executor_test");
    fs::create_dir_all(&tmp_dir).expect("Failed to create temp dir");

    let config = Config::local();
    let mut config = config.clone();
    config
      .podman_config
      .as_mut()
      .expect("expected podman config")
      .runs_dir = Some(tmp_dir.clone());

    let executor = PodmanExecutor {};

    let node = Node {
      name: "Integration Test".to_string(),
      image: "alpine:latest".to_string(),
      environment: HashMap::from([("SECRET_KEY".to_string(), "REDACTED-123".to_string())]),
      steps: vec!["echo \"VALUE: $SECRET_KEY\"".to_string()],
    };

    let run = executor.execute(&node, &config).await;

    assert_eq!(
      run.status,
      Status::Success,
      "Executor should report success"
    );

    let log_file = tmp_dir.join(format!("ci-run-{}.log", &run.id));
    let content = fs::read_to_string(&log_file).expect("Failed to read log file");

    assert!(
      content.contains("VALUE: REDACTED-123"),
      "Log should contain expanded environment variable. Found: {}",
      content
    );

    let _ = fs::remove_dir_all(&tmp_dir);
  }
}
