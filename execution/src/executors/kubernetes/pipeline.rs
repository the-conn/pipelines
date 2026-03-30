use std::{collections::BTreeMap, sync::Arc, time::SystemTime};

use k8s_openapi::{
  api::core::v1::{PersistentVolumeClaim, PersistentVolumeClaimSpec, VolumeResourceRequirements},
  apimachinery::pkg::{api::resource::Quantity, apis::meta::v1::ObjectMeta},
};
use kube::{
  Api,
  api::{DeleteParams, PostParams},
};
use tracing::{error, info, instrument, warn};

use super::{KubernetesExecutor, RESOURCE_PREFIX};
use crate::{
  pipeline::Pipeline,
  run::{JobRun, PipelineRun, RunRecorder, Status},
};

const WORKSPACE_STORAGE_SIZE: &str = "1Gi";

impl KubernetesExecutor {
  #[instrument(skip(self, pipeline, config, recorder), fields(pipeline_name = %pipeline.name, pipeline_run_id = tracing::field::Empty))]
  pub(super) async fn run_pipeline(
    &self,
    pipeline: &Pipeline,
    config: &config::Config,
    recorder: Option<Arc<dyn RunRecorder>>,
  ) -> PipelineRun {
    let mut pipeline_run = PipelineRun::new(pipeline.clone());
    tracing::Span::current().record("pipeline_run_id", &pipeline_run.id);

    let namespace = config
      .kubernetes_config
      .as_ref()
      .map(|kc| kc.namespace.as_str())
      .unwrap_or("default");

    let pvc_name = format!("{}workspace-{}", RESOURCE_PREFIX, pipeline_run.id);
    let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(self.client.clone(), namespace);

    if let Err(e) = create_workspace_pvc(&pvcs, &pvc_name).await {
      error!(error = %e, pvc = %pvc_name, "Failed to create workspace PVC");
      pipeline_run.status = Status::Failure;
      pipeline_run.ended_at = Some(SystemTime::now());
      record_pipeline_run(&recorder, &pipeline_run).await;
      return pipeline_run;
    }

    info!(pvc = %pvc_name, "Workspace PVC created");

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

    let mut pre_runs_iter = pre_job_runs.into_iter();

    for node in &pipeline.nodes {
      let pre_run = pre_runs_iter.next();
      let run = self.run_node(node, config, Some(&pvc_name), pre_run).await;
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

        cleanup_pvc(&pvcs, &pvc_name).await;
        return pipeline_run;
      }
    }

    pipeline_run.status = Status::Success;
    pipeline_run.ended_at = Some(SystemTime::now());
    record_pipeline_run(&recorder, &pipeline_run).await;

    cleanup_pvc(&pvcs, &pvc_name).await;
    pipeline_run
  }
}

async fn create_workspace_pvc(
  pvcs: &Api<PersistentVolumeClaim>,
  pvc_name: &str,
) -> Result<PersistentVolumeClaim, kube::Error> {
  let mut requests = BTreeMap::new();
  requests.insert(
    "storage".to_string(),
    Quantity(WORKSPACE_STORAGE_SIZE.to_string()),
  );

  let pvc = PersistentVolumeClaim {
    metadata: ObjectMeta {
      name: Some(pvc_name.to_string()),
      ..Default::default()
    },
    spec: Some(PersistentVolumeClaimSpec {
      access_modes: Some(vec!["ReadWriteOnce".to_string()]),
      resources: Some(VolumeResourceRequirements {
        requests: Some(requests),
        ..Default::default()
      }),
      ..Default::default()
    }),
    ..Default::default()
  };

  pvcs.create(&PostParams::default(), &pvc).await
}

async fn cleanup_pvc(pvcs: &Api<PersistentVolumeClaim>, pvc_name: &str) {
  match pvcs.delete(pvc_name, &DeleteParams::default()).await {
    Ok(_) => info!(pvc = %pvc_name, "Workspace PVC deleted"),
    Err(e) => warn!(error = %e, pvc = %pvc_name, "Failed to delete workspace PVC"),
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
