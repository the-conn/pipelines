use std::env;

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use thiserror::Error;
use tracing::Level;

#[derive(Error, Debug)]
pub enum AppConfigError {
  #[error("Configuration loading failed: {0}")]
  Config(#[from] ConfigError),
}

#[derive(Debug, Deserialize)]
struct ServerConfig {
  host: String,
  port: u16,
}

#[derive(Debug, Deserialize)]
struct LogConfig {
  level: String,
}

#[derive(Debug, Deserialize)]
struct GithubConfig {
  app_id: String,
  webhook_secret: String,
  client_id: String,
  client_secret: String,
  private_key: String,
}

#[derive(Debug, Deserialize)]
struct PipelineConfig {
  default_pipeline_timeout_secs: u64,
  default_node_timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
  server: ServerConfig,
  log: LogConfig,
  github: GithubConfig,
  pipeline: PipelineConfig,
}

impl AppConfig {
  pub fn load() -> Result<Self, AppConfigError> {
    let environment = env::var("ENV").unwrap_or_else(|_| "dev".into());

    let s = Config::builder()
      .add_source(File::with_name("config/default"))
      .add_source(File::with_name(&format!("config/{}", environment)).required(false))
      .add_source(Environment::with_prefix("JEFFERIES").separator("__"))
      .build()?;

    s.try_deserialize().map_err(AppConfigError::from)
  }

  pub fn host(&self) -> &str {
    &self.server.host
  }

  pub fn port(&self) -> u16 {
    self.server.port
  }

  pub fn log_level(&self) -> Level {
    self.log.level.parse().unwrap_or(Level::INFO)
  }

  pub fn github_app_id(&self) -> &str {
    &self.github.app_id
  }

  pub fn github_webhook_secret(&self) -> &str {
    &self.github.webhook_secret
  }

  pub fn github_client_id(&self) -> &str {
    &self.github.client_id
  }

  pub fn github_client_secret(&self) -> &str {
    &self.github.client_secret
  }

  pub fn github_private_key(&self) -> &str {
    &self.github.private_key
  }

  pub fn default_pipeline_timeout_secs(&self) -> u64 {
    self.pipeline.default_pipeline_timeout_secs
  }

  pub fn default_node_timeout_secs(&self) -> u64 {
    self.pipeline.default_node_timeout_secs
  }
}
