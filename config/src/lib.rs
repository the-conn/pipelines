use std::path::PathBuf;

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum ExecutorKind {
  Podman,
  Kubernetes,
}

#[derive(Debug, Clone)]
pub struct PodmanConfig {
  pub runs_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct KubernetesConfig {}
