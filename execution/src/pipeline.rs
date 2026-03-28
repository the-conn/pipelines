use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::node::Node;

#[derive(Debug, Error)]
pub enum PipelineError {
  #[error("Failed to parse YAML: {0}")]
  YamlParseError(String),
  #[error("HTTP error: {0}")]
  HttpError(String),
  #[error("Git error: {0}")]
  GitError(String),
  #[error("IO error: {0}")]
  IoError(String),
}

#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
#[serde(tag = "type")]
pub enum PipelineSource {
  #[default]
  Inline,
  Registry {
    name: String,
  },
  Url {
    url: String,
  },
  Git {
    repo: String,
    git_ref: String,
    path: String,
  },
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct Pipeline {
  pub name: String,
  pub nodes: Vec<Node>,
  #[serde(default)]
  pub source: PipelineSource,
}

impl Pipeline {
  pub fn from_yaml(yaml: &str) -> Result<Self, PipelineError> {
    match serde_saphyr::from_str(yaml) {
      Ok(pipeline) => Ok(pipeline),
      Err(e) => Err(PipelineError::YamlParseError(e.to_string())),
    }
  }

  pub async fn from_url(url: &str) -> Result<Self, PipelineError> {
    let response = reqwest::get(url)
      .await
      .map_err(|e| PipelineError::HttpError(e.to_string()))?;

    if !response.status().is_success() {
      return Err(PipelineError::HttpError(format!(
        "HTTP request failed with status: {}",
        response.status()
      )));
    }

    let yaml = response
      .text()
      .await
      .map_err(|e| PipelineError::HttpError(e.to_string()))?;

    let mut pipeline = Self::from_yaml(&yaml)?;
    pipeline.source = PipelineSource::Url {
      url: url.to_string(),
    };
    Ok(pipeline)
  }

  pub async fn from_git(repo: &str, git_ref: &str, path: &str) -> Result<Self, PipelineError> {
    let tmp_dir = std::env::temp_dir().join(format!("pipeline-git-{}", uuid::Uuid::new_v4()));

    let fetch_result = fetch_file_from_git(repo, git_ref, path, &tmp_dir).await;
    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    let yaml = fetch_result?;

    let mut pipeline = Self::from_yaml(&yaml)?;
    pipeline.source = PipelineSource::Git {
      repo: repo.to_string(),
      git_ref: git_ref.to_string(),
      path: path.to_string(),
    };
    Ok(pipeline)
  }
}

async fn fetch_file_from_git(
  repo: &str,
  git_ref: &str,
  path: &str,
  tmp_dir: &std::path::Path,
) -> Result<String, PipelineError> {
  run_git_command(
    &[
      "clone",
      "--depth=1",
      "--branch",
      git_ref,
      repo,
      &tmp_dir.to_string_lossy(),
    ],
    None,
    &format!("clone repo '{}' at ref '{}'", repo, git_ref),
  )
  .await?;

  let file_path = tmp_dir.join(path);
  tokio::fs::read_to_string(&file_path)
    .await
    .map_err(|e| PipelineError::IoError(format!("Failed to read '{}': {}", path, e)))
}

async fn run_git_command(
  args: &[&str],
  cwd: Option<&std::path::Path>,
  description: &str,
) -> Result<(), PipelineError> {
  let mut cmd = tokio::process::Command::new("git");
  cmd
    .args(args)
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped());

  if let Some(dir) = cwd {
    cmd.current_dir(dir);
  }

  let output = cmd
    .output()
    .await
    .map_err(|e| PipelineError::GitError(format!("Failed to run git {}: {}", description, e)))?;

  if output.status.success() {
    Ok(())
  } else {
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(PipelineError::GitError(format!(
      "git {} failed with exit code {}: {}",
      description,
      output.status,
      stderr.trim()
    )))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_pipeline_from_valid_yaml() {
    let yaml = r#"
name: "my-pipeline"
nodes:
  - name: "step-1"
    image: "alpine:latest"
    environment:
      VAR1: "value1"
    steps:
      - "echo $VAR1"
  - name: "step-2"
    image: "alpine:latest"
    environment: {}
    steps:
      - "echo done"
"#;
    let pipeline = Pipeline::from_yaml(yaml).expect("Valid YAML should parse");
    assert_eq!(pipeline.name, "my-pipeline");
    assert_eq!(pipeline.nodes.len(), 2);
    assert_eq!(pipeline.nodes[0].name, "step-1");
    assert_eq!(pipeline.nodes[1].name, "step-2");
    assert_eq!(pipeline.nodes[0].environment.get("VAR1").unwrap(), "value1");
  }

  #[test]
  fn test_pipeline_missing_nodes() {
    let yaml = r#"
name: "empty"
"#;
    let result = Pipeline::from_yaml(yaml);
    assert!(result.is_err(), "Should fail when 'nodes' field is missing");
  }

  #[test]
  fn test_pipeline_invalid_yaml() {
    let result = Pipeline::from_yaml("not: valid: yaml: [");
    assert!(result.is_err());
  }

  #[test]
  fn test_pipeline_from_yaml_source_is_inline() {
    let yaml = "name: my-pipe\nnodes: []";
    let pipeline = Pipeline::from_yaml(yaml).expect("valid YAML");
    assert_eq!(pipeline.source, PipelineSource::Inline);
  }

  #[tokio::test]
  async fn test_pipeline_from_url_not_found() {
    let result = Pipeline::from_url(
      "https://raw.githubusercontent.com/the-conn/pipelines/refs/heads/main/examples/nonexistent-pipeline.yaml",
    )
    .await;
    assert!(
      matches!(result, Err(PipelineError::HttpError(_))),
      "Expected HttpError for non-existent URL"
    );
  }

  #[tokio::test]
  async fn test_pipeline_from_url() {
    let url = "https://raw.githubusercontent.com/the-conn/pipelines/refs/heads/main/examples/multinode-pipeline.yaml";
    let pipeline = Pipeline::from_url(url)
      .await
      .expect("should load pipeline from URL");
    assert_eq!(pipeline.name, "test-pipeline");
    assert_eq!(pipeline.nodes.len(), 2);
    assert_eq!(
      pipeline.source,
      PipelineSource::Url {
        url: url.to_string()
      }
    );
  }

  #[tokio::test]
  async fn test_pipeline_from_git_missing_path() {
    let result = Pipeline::from_git(
      "https://github.com/the-conn/pipelines.git",
      "main",
      "examples/nonexistent-pipeline.yaml",
    )
    .await;
    assert!(
      matches!(result, Err(PipelineError::IoError(_))),
      "Expected IoError for missing path in git repo"
    );
  }

  #[tokio::test]
  async fn test_pipeline_from_git() {
    let pipeline = Pipeline::from_git(
      "https://github.com/the-conn/pipelines.git",
      "main",
      "examples/multinode-pipeline.yaml",
    )
    .await
    .expect("should load pipeline from git");
    assert_eq!(pipeline.name, "test-pipeline");
    assert_eq!(pipeline.nodes.len(), 2);
    assert_eq!(
      pipeline.source,
      PipelineSource::Git {
        repo: "https://github.com/the-conn/pipelines.git".to_string(),
        git_ref: "main".to_string(),
        path: "examples/multinode-pipeline.yaml".to_string(),
      }
    );
  }
}
