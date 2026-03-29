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

  use execution::{Executor, Node, Pipeline, PodmanExecutor, run::Status};

  use super::*;

  #[tokio::test]
  async fn test_podman_executor_integration() {
    let tmp_dir = std::env::temp_dir().join("ci_executor_test");

    let config = Config::test(tmp_dir.clone());

    let executor = PodmanExecutor {};

    let node = Node {
      name: "Integration Test".to_string(),
      image: "alpine:latest".to_string(),
      environment: HashMap::from([("SECRET_KEY".to_string(), "REDACTED-123".to_string())]),
      steps: vec!["echo \"VALUE: $SECRET_KEY\"".to_string()],
    };

    let run = executor.execute_node(&node, &config).await;

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

  #[tokio::test]
  async fn test_podman_pipeline_integration() {
    let tmp_dir = std::env::temp_dir().join("ci_pipeline_test");

    let config = Config::test(tmp_dir.clone());

    let executor = PodmanExecutor {};

    let yaml = r#"
name: "test-pipeline"
nodes:
  - name: "producer"
    image: "alpine:latest"
    environment: {}
    steps:
      - "echo PIPELINE_VAR=hello_from_node1 >> /workspace/.pipeline-env"
      - "mkdir -p /workspace/output_dir"
      - "echo 'created by node1' > /workspace/output_dir/result.txt"
  - name: "consumer"
    image: "alpine:latest"
    environment: {}
    steps:
      - "echo \"Received: $PIPELINE_VAR\""
      - "ls /workspace/output_dir"
      - "cat /workspace/output_dir/result.txt"
"#;

    let pipeline = Pipeline::from_yaml(yaml).expect("Valid pipeline YAML should parse");
    let pipeline_run = executor.execute_pipeline(&pipeline, &config, None).await;

    assert_eq!(
      pipeline_run.status,
      Status::Success,
      "Pipeline should succeed. Node statuses: {:?}",
      pipeline_run
        .node_runs
        .iter()
        .map(|r| r.status)
        .collect::<Vec<_>>()
    );
    assert_eq!(
      pipeline_run.node_runs.len(),
      2,
      "Pipeline should have 2 node runs"
    );
    assert_eq!(
      pipeline_run.node_runs[0].status,
      Status::Success,
      "Producer node should succeed"
    );
    assert_eq!(
      pipeline_run.node_runs[1].status,
      Status::Success,
      "Consumer node should succeed"
    );

    let consumer_run_id = &pipeline_run.node_runs[1].id;
    let log_path = tmp_dir.join(format!("ci-run-{}.log", consumer_run_id));
    let log_content = fs::read_to_string(&log_path).expect("Failed to read consumer log file");

    assert!(
      log_content.contains("Received: hello_from_node1"),
      "Consumer log should contain the variable exported by the producer. Log: {}",
      log_content
    );
    assert!(
      log_content.contains("result.txt"),
      "Consumer log should list the file created by the producer. Log: {}",
      log_content
    );

    let _ = fs::remove_dir_all(&tmp_dir);
  }
}
