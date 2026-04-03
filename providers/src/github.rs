use std::{fmt::Debug, sync::Arc};

use app_config::AppConfig;
use axum::{
  body::Bytes,
  extract::State,
  http::{HeaderMap, StatusCode},
};
use hmac::{Hmac, KeyInit, Mac};
use jsonwebtoken::EncodingKey;
use octocrab::{
  Octocrab,
  models::webhook_events::{
    WebhookEvent, WebhookEventPayload, payload::PullRequestWebhookEventAction,
  },
};
use serde::{Deserialize, de::Error};
use sha2::Sha256;
use thiserror::Error;
use tracing::{info, warn};

use super::get_header;

const GITHUB_EVENT_PUSH: &str = "push";
const GITHUB_EVENT_PULL_REQUEST: &str = "pull_request";

#[derive(Debug, Error)]
pub enum GithubError {
  #[error("Octocrab error: {0}")]
  Octocrab(#[from] octocrab::Error),
  #[error("Invalid GitHub App ID: {0}")]
  InvalidAppId(#[from] std::num::ParseIntError),
  #[error("Invalid GitHub App private key: {0}")]
  InvalidPrivateKey(#[from] jsonwebtoken::errors::Error),
  #[error("Invalid webhook event payload: {0}")]
  InvalidPayload(#[from] serde_json::Error),
}

pub struct GithubProvider;

impl GithubProvider {
  pub async fn handle_webhook(
    State(config): State<Arc<AppConfig>>,
    headers: HeaderMap,
    body: Bytes,
  ) -> StatusCode {
    let signature = get_header(&headers, "X-Hub-Signature-256");
    if !signature_matches(&body, &signature, config.github_webhook_secret()) {
      warn!("Unauthorized webhook attempt: Signature mismatch");
      return StatusCode::UNAUTHORIZED;
    }

    let event_type = get_header(&headers, "X-GitHub-Event");
    match event_type.as_str() {
      GITHUB_EVENT_PUSH => match handle_push(&body, config).await {
        Ok(status) => status,
        Err(e) => {
          warn!(error = ?e, "Failed to handle push event");
          StatusCode::INTERNAL_SERVER_ERROR
        }
      },
      GITHUB_EVENT_PULL_REQUEST => match handle_pull_request(&body, config).await {
        Ok(status) => status,
        Err(e) => {
          warn!(error = ?e, "Failed to handle pull request event");
          StatusCode::INTERNAL_SERVER_ERROR
        }
      },
      _ => {
        info!(event_type, "Received unsupported GitHub event");
        StatusCode::NOT_IMPLEMENTED
      }
    }
  }
}

async fn handle_push(payload: &[u8], config: Arc<AppConfig>) -> Result<StatusCode, GithubError> {
  Ok(StatusCode::NOT_IMPLEMENTED)
}

async fn handle_pull_request(
  payload: &[u8],
  config: Arc<AppConfig>,
) -> Result<StatusCode, GithubError> {
  let event = WebhookEvent::try_from_header_and_body(GITHUB_EVENT_PULL_REQUEST, payload)?;
  let WebhookEventPayload::PullRequest(pr_event) = event.specific else {
    return Err(GithubError::InvalidPayload(serde_json::Error::custom(
      "Bad pull request event payload",
    )));
  };

  if pr_event.action != PullRequestWebhookEventAction::Opened
    && pr_event.action != PullRequestWebhookEventAction::Synchronize
  {
    info!(action = ?pr_event.action, "Ignoring pull request event with unsupported action");
    return Ok(StatusCode::NOT_IMPLEMENTED);
  }

  let pr = pr_event.pull_request;
  let owner = pr
    .repo
    .clone()
    .ok_or_else(|| {
      GithubError::InvalidPayload(serde_json::Error::custom(
        "Missing repository information in pull request event",
      ))
    })?
    .owner
    .ok_or_else(|| {
      GithubError::InvalidPayload(serde_json::Error::custom(
        "Missing owner information in pull request event",
      ))
    })?
    .login;
  let repo = pr
    .repo
    .ok_or_else(|| {
      GithubError::InvalidPayload(serde_json::Error::custom(
        "Missing repository information in pull request event",
      ))
    })?
    .name;
  let sha = pr.head.sha;
  let ref_field = pr.head.ref_field;
  start_pr_pipelines(&owner, &repo, &sha, &ref_field, config).await?;
  Ok(StatusCode::OK)
}

async fn start_pr_pipelines(
  owner: &str,
  repo: &str,
  sha: &str,
  ref_field: &str,
  config: Arc<AppConfig>,
) -> Result<(), GithubError> {
  let crab = Octocrab::builder()
    .app(
      config.github_app_id().parse::<u64>()?.into(),
      EncodingKey::from_rsa_pem(config.github_private_key().as_bytes())?,
    )
    .build()?;

  Ok(())
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
