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
pub struct AppConfig {
  server: ServerConfig,
  log: LogConfig,
}

impl AppConfig {
  pub fn load() -> Result<Self, AppConfigError> {
    let environment = env::var("ENV").unwrap_or_else(|_| "dev".into());

    let s = Config::builder()
      .add_source(File::with_name("config/default"))
      .add_source(File::with_name(&format!("config/{}", environment)).required(false))
      .add_source(Environment::with_prefix("JEFFERIES").separator("_"))
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
}
