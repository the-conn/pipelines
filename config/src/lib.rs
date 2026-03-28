use std::path::PathBuf;

#[derive(Debug)]
pub struct Config {
  pub executor: ExecutorKind,
  pub podman_config: Option<PodmanConfig>,
  pub kubernetes_config: Option<KubernetesConfig>,
}

impl Config {
  pub fn local() -> Self {
    Self {
      executor: ExecutorKind::Podman,
      podman_config: Some(PodmanConfig {
        runs_dir: Some("runs".into()),
      }),
      kubernetes_config: None,
    }
  }
}

#[derive(Debug)]
pub enum ExecutorKind {
  Podman,
  Kubernetes,
}

#[derive(Debug)]
pub struct PodmanConfig {
  pub runs_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct KubernetesConfig {}
