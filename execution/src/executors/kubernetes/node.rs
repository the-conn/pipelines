use std::time::{Duration, Instant, SystemTime};

use k8s_openapi::{
  api::core::v1::{
    Container, EnvVar, PersistentVolumeClaimVolumeSource, Pod, PodSpec, Volume, VolumeMount,
  },
  apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{
  Api,
  api::{DeleteParams, PostParams},
};
use tracing::{error, info, instrument, warn};

use super::{KubernetesExecutor, RESOURCE_PREFIX};
use crate::{
  executors::generate_entrypoint_script,
  node::Node,
  run::{JobRun, Status},
};

impl KubernetesExecutor {
  #[instrument(skip(self, node, config, pvc_name, existing_run), fields(node_name = %node.name, pod_name = tracing::field::Empty, run_id = tracing::field::Empty))]
  pub(super) async fn run_node(
    &self,
    node: &Node,
    config: &config::Config,
    pvc_name: Option<&str>,
    existing_run: Option<JobRun>,
  ) -> JobRun {
    let default_k8s_config = config::KubernetesConfig::default();
    let k8s_config = config
      .kubernetes_config
      .as_ref()
      .unwrap_or(&default_k8s_config);
    let namespace = k8s_config.namespace.as_str();
    let poll_interval = Duration::from_secs(k8s_config.pod_poll_interval_secs);
    let timeout = Duration::from_secs(k8s_config.pod_timeout_secs);

    let mut run = existing_run
      .map(|r| JobRun {
        node: node.clone(),
        ..r
      })
      .unwrap_or_else(|| JobRun::new(node.clone()));

    let pod_name = format!("{}{}", RESOURCE_PREFIX, run.id);
    tracing::Span::current().record("pod_name", &pod_name);
    tracing::Span::current().record("run_id", &run.id);

    let script = generate_entrypoint_script(node.steps.clone());
    let pod = build_pod(&pod_name, node, &script, pvc_name);

    let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);

    match pods.create(&PostParams::default(), &pod).await {
      Ok(_) => {
        info!(pod = %pod_name, "Pod created");
      }
      Err(e) => {
        error!(error = %e, pod = %pod_name, "Failed to create pod");
        return fail_run_early(run);
      }
    }

    run.started_at = Some(SystemTime::now());

    let status = wait_for_pod_completion(&pods, &pod_name, poll_interval, timeout).await;

    if let Err(e) = pods.delete(&pod_name, &DeleteParams::default()).await {
      warn!(error = %e, pod = %pod_name, "Failed to delete pod");
    } else {
      info!(pod = %pod_name, "Pod deleted");
    }

    run.status = status;
    run.ended_at = Some(SystemTime::now());
    run
  }
}

fn build_pod(pod_name: &str, node: &Node, script: &str, pvc_name: Option<&str>) -> Pod {
  let env_vars: Vec<EnvVar> = node
    .environment
    .iter()
    .map(|(k, v)| EnvVar {
      name: k.clone(),
      value: Some(v.clone()),
      ..Default::default()
    })
    .collect();

  let mut volume_mounts: Vec<VolumeMount> = Vec::new();
  let mut volumes: Vec<Volume> = Vec::new();

  if let Some(pvc) = pvc_name {
    volume_mounts.push(VolumeMount {
      name: "workspace".to_string(),
      mount_path: "/workspace".to_string(),
      ..Default::default()
    });
    volumes.push(Volume {
      name: "workspace".to_string(),
      persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
        claim_name: pvc.to_string(),
        ..Default::default()
      }),
      ..Default::default()
    });
  }

  let container = Container {
    name: "node".to_string(),
    image: Some(node.image.clone()),
    command: Some(vec!["sh".to_string()]),
    args: Some(vec!["-c".to_string(), script.to_string()]),
    env: Some(env_vars),
    volume_mounts: Some(volume_mounts),
    ..Default::default()
  };

  Pod {
    metadata: ObjectMeta {
      name: Some(pod_name.to_string()),
      ..Default::default()
    },
    spec: Some(PodSpec {
      containers: vec![container],
      volumes: Some(volumes),
      restart_policy: Some("Never".to_string()),
      ..Default::default()
    }),
    ..Default::default()
  }
}

async fn wait_for_pod_completion(
  pods: &Api<Pod>,
  pod_name: &str,
  poll_interval: Duration,
  timeout: Duration,
) -> Status {
  let start = Instant::now();

  loop {
    if start.elapsed() > timeout {
      error!(pod = %pod_name, "Timed out waiting for pod completion");
      return Status::Failure;
    }

    match pods.get(pod_name).await {
      Ok(pod) => {
        let phase = pod.status.and_then(|s| s.phase);
        match phase.as_deref() {
          Some("Succeeded") => {
            info!(pod = %pod_name, "Pod completed successfully");
            return Status::Success;
          }
          Some("Failed") => {
            error!(pod = %pod_name, "Pod failed");
            return Status::Failure;
          }
          Some(phase) => {
            info!(pod = %pod_name, phase = %phase, "Pod still running");
          }
          None => {
            info!(pod = %pod_name, "Pod phase unknown, waiting");
          }
        }
      }
      Err(e) => {
        error!(error = %e, pod = %pod_name, "Failed to get pod status");
        return Status::Failure;
      }
    }

    tokio::time::sleep(poll_interval).await;
  }
}

fn fail_run_early(mut run: JobRun) -> JobRun {
  run.status = Status::Failure;
  run.started_at = Some(SystemTime::now());
  run.ended_at = Some(SystemTime::now());
  run
}

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use super::*;
  use crate::Node;

  fn make_node(image: &str, env: HashMap<String, String>) -> Node {
    Node {
      name: "test".to_string(),
      image: image.to_string(),
      environment: env,
      steps: vec![],
    }
  }

  #[test]
  fn test_build_pod_no_workspace() {
    let node = make_node(
      "alpine:latest",
      HashMap::from([("KEY".to_string(), "val".to_string())]),
    );
    let pod = build_pod("jefferies-abc123", &node, "#!/bin/sh\necho hi\n", None);

    let spec = pod.spec.unwrap();
    assert_eq!(spec.restart_policy.unwrap(), "Never");
    assert!(spec.volumes.unwrap_or_default().is_empty());

    let container = &spec.containers[0];
    assert_eq!(container.image.as_deref().unwrap(), "alpine:latest");
    assert!(
      container
        .volume_mounts
        .as_ref()
        .unwrap_or(&vec![])
        .is_empty()
    );

    let env = container.env.as_ref().unwrap();
    assert!(
      env
        .iter()
        .any(|e| e.name == "KEY" && e.value.as_deref() == Some("val"))
    );
  }

  #[test]
  fn test_build_pod_with_workspace() {
    let node = make_node("alpine:latest", HashMap::new());
    let pod = build_pod(
      "jefferies-abc123",
      &node,
      "#!/bin/sh\necho hi\n",
      Some("jefferies-workspace-abc"),
    );

    let spec = pod.spec.unwrap();
    let volumes = spec.volumes.unwrap();
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].name, "workspace");
    let pvc_source = volumes[0].persistent_volume_claim.as_ref().unwrap();
    assert_eq!(pvc_source.claim_name, "jefferies-workspace-abc");

    let container = &spec.containers[0];
    let mounts = container.volume_mounts.as_ref().unwrap();
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].mount_path, "/workspace");
  }

  #[test]
  fn test_build_pod_metadata() {
    let node = make_node("alpine:latest", HashMap::new());
    let pod = build_pod("jefferies-test-id", &node, "#!/bin/sh\n", None);
    assert_eq!(pod.metadata.name.unwrap(), "jefferies-test-id");
  }

  #[test]
  fn test_build_pod_command_and_args() {
    let node = make_node("alpine:latest", HashMap::new());
    let script = "#!/bin/sh\nset -e\necho hello\n";
    let pod = build_pod("jefferies-test", &node, script, None);

    let container = &pod.spec.unwrap().containers[0];
    assert_eq!(container.command.as_ref().unwrap(), &["sh"]);
    assert_eq!(container.args.as_ref().unwrap(), &["-c", script]);
  }
}
