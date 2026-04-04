mod github;

use std::sync::Arc;

use app_config::AppConfig;
use axum::http::HeaderMap;
use coordinator::{Dispatcher, RunRegistry};
pub use github::GithubProvider;

pub(crate) fn get_header(headers: &HeaderMap, key: &str) -> String {
  headers
    .get(key)
    .and_then(|v| v.to_str().ok())
    .map(|s| s.to_string())
    .unwrap_or_default()
}

#[derive(Clone)]
pub struct ProviderState {
  pub config: Arc<AppConfig>,
  pub registry: Arc<RunRegistry>,
  pub dispatcher: Arc<dyn Dispatcher>,
}

impl ProviderState {
  pub fn new(
    config: Arc<AppConfig>,
    registry: Arc<RunRegistry>,
    dispatcher: Arc<dyn Dispatcher>,
  ) -> Self {
    Self {
      config,
      registry,
      dispatcher,
    }
  }
}
