use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
  #[error("Failed to parse YAML: {0}")]
  YamlParseError(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Pipeline {
  name: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  on: Option<PipelineTriggers>,
  nodes: Vec<PipelineNode>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PipelineTriggers {
  #[serde(skip_serializing_if = "Option::is_none")]
  push: Option<Refs>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pull_request: Option<Refs>,
  #[serde(skip_serializing_if = "Option::is_none")]
  schedule: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Refs {
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  branches: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PipelineNode {
  name: String,
  image: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  checkout: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  after: Vec<String>,
  steps: Vec<PipelineStep>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum PipelineStep {
  Inline(String),
  Named(NamedStep),
}

#[derive(Debug, Serialize, Deserialize)]
struct NamedStep {
  name: String,
  run: String,
}

impl Pipeline {
  pub fn from_yaml(yaml: &str) -> Result<Self, PipelineError> {
    match serde_saphyr::from_str(yaml) {
      Ok(pipeline) => Ok(pipeline),
      Err(e) => Err(PipelineError::YamlParseError(e.to_string())),
    }
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn triggered_by_push(&self, branch: &str) -> bool {
    let Some(triggers) = &self.on else {
      return false;
    };
    let Some(push_trigger) = &triggers.push else {
      return false;
    };
    refs_match_branch(push_trigger, branch)
  }

  pub fn triggered_by_pull_request(&self, branch: &str) -> bool {
    let Some(triggers) = &self.on else {
      return false;
    };
    let Some(pr_trigger) = &triggers.pull_request else {
      return false;
    };
    refs_match_branch(pr_trigger, branch)
  }
}

fn refs_match_branch(refs: &Refs, branch: &str) -> bool {
  let no_branch_filter = refs.branches.is_empty();
  no_branch_filter || refs.branches.iter().any(|b| b == branch)
}

#[cfg(test)]
mod tests {
  use std::{fs, path::PathBuf};

  use super::*;

  #[test]
  fn test_load_real_pipeline_file() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut workspace_root = crate_dir.to_path_buf();

    while !workspace_root.join(".jefferies").exists() {
      if !workspace_root.pop() {
        panic!(
          "Could not find .jefferies directory in any parent of {:?}",
          crate_dir
        );
      }
    }

    let path = workspace_root.join(".jefferies").join("pipeline.yaml");
    let yaml_content = fs::read_to_string(&path).expect("Should be able to read the pipeline file");

    let pipeline = Pipeline::from_yaml(&yaml_content)
      .expect("Should successfully parse .jefferies/pipeline.yaml");

    assert_eq!(pipeline.name, "Jefferies Pipeline");
    assert!(
      !pipeline.nodes.is_empty(),
      "Pipeline should have at least one node"
    );

    let first_node = &pipeline.nodes[0];
    assert!(
      first_node.image.contains("rust"),
      "First node should use a Rust image"
    );

    assert!(first_node.checkout.is_some());
  }

  #[test]
  fn test_triggered_by_push_matching_branch() {
    let yaml = r#"
name: Test Pipeline
on:
  push:
    branches:
      - main
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.triggered_by_push("main"));
    assert!(!pipeline.triggered_by_push("feature-branch"));
  }

  #[test]
  fn test_triggered_by_push_no_branch_filter() {
    let yaml = r#"
name: Test Pipeline
on:
  push: {}
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.triggered_by_push("main"));
    assert!(pipeline.triggered_by_push("any-branch"));
  }

  #[test]
  fn test_triggered_by_pull_request_matching_branch() {
    let yaml = r#"
name: Test Pipeline
on:
  pull_request:
    branches:
      - main
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(pipeline.triggered_by_pull_request("main"));
    assert!(!pipeline.triggered_by_pull_request("feature-branch"));
  }

  #[test]
  fn test_not_triggered_without_matching_event() {
    let yaml = r#"
name: Test Pipeline
on:
  push:
    branches:
      - main
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(!pipeline.triggered_by_pull_request("main"));
  }

  #[test]
  fn test_not_triggered_without_on_block() {
    let yaml = r#"
name: Test Pipeline
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert!(!pipeline.triggered_by_push("main"));
    assert!(!pipeline.triggered_by_pull_request("main"));
  }
}
