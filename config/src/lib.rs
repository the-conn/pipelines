use std::path::PathBuf;

use tracing::error;

#[derive(Debug, Clone)]
pub struct Config {
  pub executor: ExecutorKind,
  pub podman_config: Option<PodmanConfig>,
  pub kubernetes_config: Option<KubernetesConfig>,
  pub storage_config: StorageConfig,
  pub server_config: ServerConfig,
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
      server_config: ServerConfig {
        cors: CorsConfig {
          allow_any_origin: true,
        },
        ..ServerConfig::default()
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
      server_config: ServerConfig::default(),
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
pub struct KubernetesConfig {
  pub namespace: String,
}

impl Default for KubernetesConfig {
  fn default() -> Self {
    Self {
      namespace: "default".to_string(),
    }
  }
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
  pub db_url: String,
}

#[derive(Debug, Clone)]
pub struct CorsConfig {
  pub allow_any_origin: bool,
}

impl Default for CorsConfig {
  fn default() -> Self {
    Self {
      allow_any_origin: false,
    }
  }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
  pub host: String,
  pub port: u16,
  pub cors: CorsConfig,
}

impl Default for ServerConfig {
  fn default() -> Self {
    Self {
      host: "0.0.0.0".to_string(),
      port: 8080,
      cors: CorsConfig::default(),
    }
  }
}
