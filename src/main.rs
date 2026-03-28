use std::collections::HashMap;

use config::Config;
use execution::{Executor, Node, PodmanExecutor};
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

fn setup_tracing() {
  let subscriber = FmtSubscriber::builder()
    .with_max_level(Level::INFO)
    .with_env_filter("server=info,execution=info,config=info")
    .finish();

  tracing::subscriber::set_global_default(subscriber).expect("Setting default subscriber failed");
}

async fn test_podman_executor(config: &Config) {
  let node = Node {
    name: "Demo Build Step".to_string(),
    image: "alpine:latest".to_string(),
    environment: HashMap::from([
      ("BUILD_ENV".to_string(), "local-dev".to_string()),
      ("USER_NAME".to_string(), "podman user".to_string()),
    ]),
    steps: vec![
      "echo 'Starting Pipeline...'",
      "uname -a",
      "df -h",
      "echo \"Environment Check: $BUILD_ENV for $USER_NAME\"",
      "echo 'Done!'",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect(),
  };

  let executor = PodmanExecutor {};

  info!(node_name = %node.name, "Dispatching job to Podman...");

  let run = executor.execute(&node, &config).await;

  info!(
      node_name = %node.name,
      status = ?run.status,
      duration = ?run.execution_duration(),
      "Job sequence completed"
  );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  setup_tracing();

  let config = Config::local();
  info!(config = ?config, "Loaded local configuration");

  test_podman_executor(&config).await;

  Ok(())
}
