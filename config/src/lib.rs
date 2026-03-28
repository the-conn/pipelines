use std::path::PathBuf;

use tracing::error;

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
        runs_dir: ensure_runs_dir("runs".into()),
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
        runs_dir: ensure_runs_dir(runs_dir),
      }),
      kubernetes_config: None,
      storage_config: StorageConfig {
        db_url: "sqlite::memory:".to_string(),
      },
    }
  }
}

fn ensure_runs_dir(path: PathBuf) -> Option<PathBuf> {
  if let Err(e) = std::fs::create_dir_all(&path) {
    error!(path = %path.display(), error = %e, "Failed to create runs directory");
    return None;
  }
  Some(path)
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
