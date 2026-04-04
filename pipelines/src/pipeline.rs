use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
  #[error("Failed to parse YAML: {0}")]
  YamlParseError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
  name: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  timeout_secs: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  fail_fast: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  on: Option<PipelineTriggers>,
  nodes: Vec<PipelineNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PipelineTriggers {
  #[serde(
    default,
    skip_serializing_if = "Option::is_none",
    deserialize_with = "deserialize_trigger"
  )]
  push: Option<Refs>,
  #[serde(
    default,
    skip_serializing_if = "Option::is_none",
    deserialize_with = "deserialize_trigger"
  )]
  pull_request: Option<Refs>,
  #[serde(skip_serializing_if = "Option::is_none")]
  schedule: Option<String>,
}

fn deserialize_trigger<'de, D>(deserializer: D) -> Result<Option<Refs>, D::Error>
where
  D: Deserializer<'de>,
{
  let opt = Option::<Refs>::deserialize(deserializer)?;
  Ok(Some(opt.unwrap_or_default()))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Refs {
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  branches: Vec<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PipelineNode {
  name: String,
  image: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  timeout_secs: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  checkout: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  after: Vec<String>,
  steps: Vec<PipelineStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum PipelineStep {
  Inline(String),
  Named(NamedStep),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NamedStep {
  name: String,
  run: String,
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
  pub name: String,
  pub image: String,
  pub steps: Vec<String>,
  pub dependencies: Vec<String>,
  pub timeout_secs: Option<u64>,
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

  pub fn pipeline_timeout_secs(&self) -> Option<u64> {
    self.timeout_secs
  }

  pub fn fail_fast_override(&self) -> Option<bool> {
    self.fail_fast
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

  pub fn node_info(&self) -> Vec<NodeInfo> {
    self
      .nodes
      .iter()
      .map(|n| NodeInfo {
        name: n.name.clone(),
        image: n.image.clone(),
        steps: n.steps.iter().map(step_command).collect(),
        dependencies: n.after.clone(),
        timeout_secs: n.timeout_secs,
      })
      .collect()
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

fn step_command(step: &PipelineStep) -> String {
  match step {
    PipelineStep::Inline(cmd) => cmd.clone(),
    PipelineStep::Named(ns) => ns.run.clone(),
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
  push:
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

  #[test]
  fn test_node_info_includes_image_and_steps() {
    let yaml = r#"
name: Test Pipeline
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
      - cargo test
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let infos = pipeline.node_info();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].name, "Build");
    assert_eq!(infos[0].image, "rust:latest");
    assert_eq!(infos[0].steps, vec!["cargo build", "cargo test"]);
    assert!(infos[0].timeout_secs.is_none());
  }

  #[test]
  fn test_node_info_per_node_timeout() {
    let yaml = r#"
name: Test Pipeline
nodes:
  - name: Build
    image: rust:latest
    timeout_secs: 120
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    let infos = pipeline.node_info();
    assert_eq!(infos[0].timeout_secs, Some(120));
  }

  #[test]
  fn test_pipeline_timeout() {
    let yaml = r#"
name: Test Pipeline
timeout_secs: 7200
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert_eq!(pipeline.pipeline_timeout_secs(), Some(7200));
  }

  #[test]
  fn test_fail_fast_override_explicit_false() {
    let yaml = r#"
name: Test Pipeline
fail_fast: false
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert_eq!(pipeline.fail_fast_override(), Some(false));
  }

  #[test]
  fn test_fail_fast_override_explicit_true() {
    let yaml = r#"
name: Test Pipeline
fail_fast: true
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert_eq!(pipeline.fail_fast_override(), Some(true));
  }

  #[test]
  fn test_fail_fast_override_absent() {
    let yaml = r#"
name: Test Pipeline
nodes:
  - name: Build
    image: rust:latest
    steps:
      - cargo build
"#;
    let pipeline = Pipeline::from_yaml(yaml).unwrap();
    assert_eq!(pipeline.fail_fast_override(), None);
  }
}
