use std::collections::HashMap;

use config::Config;
use tracing::{error, info, instrument};

use crate::{
  node::Node,
  pipeline::Pipeline,
  run::{PipelineRun, Status},
};

use super::PodmanExecutor;

impl PodmanExecutor {
  #[instrument(skip(self, pipeline, config), fields(pipeline_name = %pipeline.name, pipeline_run_id = tracing::field::Empty))]
  pub(super) async fn run_pipeline(&self, pipeline: &Pipeline, config: &Config) -> PipelineRun {
    let mut pipeline_run = PipelineRun::new();
    tracing::Span::current().record("pipeline_run_id", &pipeline_run.id);

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

    let mut accumulated_env: HashMap<String, String> = HashMap::new();

    for node in &pipeline.nodes {
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
        .run_node(&node_with_env, config, Some(&workspace))
        .await;
      let success = run.status == Status::Success;
      pipeline_run.node_runs.push(run);

      if !success {
        pipeline_run.status = Status::Failure;
        return pipeline_run;
      }

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
