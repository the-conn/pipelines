use std::{string, sync::Arc};

use app_config::AppConfig;
use axum::{
  body::Bytes,
  extract::State,
  http::{HeaderMap, StatusCode},
  response::IntoResponse,
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tracing::{info, warn};

use super::get_header;

pub struct GithubProvider;

impl GithubProvider {
  pub async fn handle_webhook(
    State(config): State<Arc<AppConfig>>,
    headers: HeaderMap,
    body: Bytes,
  ) -> impl IntoResponse {
    let string_body = String::from_utf8_lossy(&body);
    info!(body = ?string_body, "Received GitHub webhook");
    let signature = get_header(&headers, "X-Hub-Signature-256");

    if !signature_matches(&body, &signature, config.github_webhook_secret()) {
      warn!("Unauthorized webhook attempt: Signature mismatch");
      return StatusCode::UNAUTHORIZED;
    }

    let event_type = get_header(&headers, "X-GitHub-Event");
    info!("Verified GitHub event received: {}", event_type);

    let json_result: Result<serde_json::Value, _> = serde_json::from_slice(&body);

    match json_result {
      Ok(payload) => {
        process_event(&event_type, payload);
        StatusCode::ACCEPTED
      }
      Err(e) => {
        warn!("Failed to parse verified webhook JSON: {}", e);
        StatusCode::BAD_REQUEST
      }
    }
  }
}

fn signature_matches(payload: &[u8], signature_header: &str, secret: &str) -> bool {
  let Some(hex_hash) = signature_header.strip_prefix("sha256=") else {
    return false;
  };

  let Ok(expected_signature) = hex::decode(hex_hash) else {
    return false;
  };

  let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
    return false;
  };

  mac.update(payload);
  mac.verify_slice(&expected_signature).is_ok()
}

fn process_event(event_type: &str, _payload: serde_json::Value) {
  match event_type {
    "push" => {
      info!("Push event received. Time to find the .jefferies folder!");
    }
    _ => info!("Received unhandled event type: {}", event_type),
  }
}
