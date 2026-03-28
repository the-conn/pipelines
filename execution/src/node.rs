use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NodeError {
  #[error("Failed to parse YAML: {0}")]
  YamlParseError(String),
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct Node {
  pub name: String,
  pub image: String,
  pub environment: HashMap<String, String>,
  pub steps: Vec<String>,
}

impl Node {
  pub fn from_yaml(yaml: &str) -> Result<Self, NodeError> {
    match serde_saphyr::from_str(yaml) {
      Ok(node) => Ok(node),
      Err(e) => Err(NodeError::YamlParseError(e.to_string())),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_node_from_valid_yaml() {
    let yaml = r#"
name: "rust-build"
image: "rust:1.80-alpine"
environment:
  CARGO_HOME: "/root/.cargo"
  RUST_BACKTRACE: "1"
steps:
  - "cargo --version"
  - "cargo build --release"
  - "cargo test"
"#;

    let node = Node::from_yaml(yaml).expect("Valid YAML should parse");

    assert_eq!(node.name, "rust-build");
    assert_eq!(node.image, "rust:1.80-alpine");

    assert_eq!(node.environment.get("RUST_BACKTRACE").unwrap(), "1");

    assert_eq!(node.steps.len(), 3);
    assert_eq!(node.steps[1], "cargo build --release");
  }

  #[test]
  fn test_node_missing_fields() {
    let yaml = r#"
name: "incomplete"
image: "alpine"
environment: {}
"#;
    let result = Node::from_yaml(yaml);
    assert!(
      result.is_err(),
      "Should fail when required 'steps' field is missing"
    );
  }

  #[test]
  fn test_node_invalid_types() {
    let yaml = r#"
name: "bad-types"
image: ["not", "a", "string"]
environment: {}
steps: []
"#;
    let result = Node::from_yaml(yaml);
    assert!(result.is_err());
  }
}
