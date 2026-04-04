mod github;

use axum::http::HeaderMap;
pub use github::GithubProvider;

pub(crate) fn get_header(headers: &HeaderMap, key: &str) -> String {
  headers
    .get(key)
    .and_then(|v| v.to_str().ok())
    .map(|s| s.to_string())
    .unwrap_or_default()
}
