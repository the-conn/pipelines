use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
  pub executor: ExecutorKind,
  pub podman_config: Option<PodmanConfig>,
  pub kubernetes_config: Option<KubernetesConfig>,
  pub storage_config: StorageConfig,
}

impl Config {
  pub fn local() -> Self {
    Self {
      executor: ExecutorKind::Podman,
      podman_config: Some(PodmanConfig {
        runs_dir: Some("runs".into()),
      }),
      kubernetes_config: None,
      storage_config: StorageConfig {
        db_url: "sqlite:pipelines.db".to_string(),
      },
    }
  }

  pub fn test(runs_dir: PathBuf) -> Self {
    Self {
      executor: ExecutorKind::Podman,
      podman_config: Some(PodmanConfig {
        runs_dir: Some(runs_dir),
      }),
      kubernetes_config: None,
      storage_config: StorageConfig {
        db_url: "sqlite::memory:".to_string(),
      },
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

#[derive(Debug, Clone)]
pub struct StorageConfig {
  pub db_url: String,
}
