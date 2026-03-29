use std::{collections::HashMap, sync::Arc, time::SystemTime};

use config::Config;
use tracing::{error, info, instrument, warn};

use super::PodmanExecutor;
use crate::{
  node::Node,
  pipeline::Pipeline,
  run::{JobRun, PipelineRun, RunRecorder, Status},
};

impl PodmanExecutor {
  #[instrument(skip(self, pipeline, config, recorder), fields(pipeline_name = %pipeline.name, pipeline_run_id = tracing::field::Empty))]
  pub(super) async fn run_pipeline(
    &self,
    pipeline: &Pipeline,
    config: &Config,
    recorder: Option<Arc<dyn RunRecorder>>,
  ) -> PipelineRun {
    let mut pipeline_run = PipelineRun::new(pipeline.clone());
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
      pipeline_run.ended_at = Some(SystemTime::now());
      record_pipeline_run(&recorder, &pipeline_run).await;
      return pipeline_run;
    }

    info!(workspace = %workspace.display(), "Pipeline workspace created");

    pipeline_run.status = Status::InProgress;
    pipeline_run.started_at = Some(SystemTime::now());

    let pre_job_runs: Vec<JobRun> = pipeline
      .nodes
      .iter()
      .map(|node| JobRun::new(node.clone()))
      .collect();

    record_pipeline_run(&recorder, &pipeline_run).await;
    for pre_run in &pre_job_runs {
      record_job_run(&recorder, &pipeline_run.id, pre_run).await;
    }

    let mut accumulated_env: HashMap<String, String> = HashMap::new();
    let mut pre_runs_iter = pre_job_runs.into_iter();

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

      let pre_run = pre_runs_iter.next();
      let run = self
        .run_node(&node_with_env, config, Some(&workspace), pre_run)
        .await;
      let success = run.status == Status::Success;

      record_job_run(&recorder, &pipeline_run.id, &run).await;
      pipeline_run.node_runs.push(run);

      if !success {
        let skipped_runs: Vec<JobRun> = pre_runs_iter
          .map(|mut r| {
            r.status = Status::Skipped;
            r
          })
          .collect();

        for skipped_run in &skipped_runs {
          record_job_run(&recorder, &pipeline_run.id, skipped_run).await;
        }
        pipeline_run.node_runs.extend(skipped_runs);

        pipeline_run.status = Status::Failure;
        pipeline_run.ended_at = Some(SystemTime::now());
        record_pipeline_run(&recorder, &pipeline_run).await;
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
    pipeline_run.ended_at = Some(SystemTime::now());
    record_pipeline_run(&recorder, &pipeline_run).await;
    pipeline_run
  }
}

async fn record_pipeline_run(recorder: &Option<Arc<dyn RunRecorder>>, run: &PipelineRun) {
  if let Some(r) = recorder {
    if let Err(e) = r.record_pipeline_run(run).await {
      warn!(error = %e, run_id = %run.id, "Failed to record pipeline run");
    }
  }
}

async fn record_job_run(
  recorder: &Option<Arc<dyn RunRecorder>>,
  pipeline_run_id: &str,
  run: &JobRun,
) {
  if let Some(r) = recorder {
    if let Err(e) = r.record_job_run(pipeline_run_id, run).await {
      warn!(error = %e, job_run_id = %run.id, "Failed to record job run");
    }
  }
}
