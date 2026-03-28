use serde::Deserialize;
use thiserror::Error;

use crate::node::Node;

#[derive(Debug, Error)]
pub enum PipelineError {
  #[error("Failed to parse YAML: {0}")]
  YamlParseError(String),
}

#[derive(Debug, Deserialize, Clone)]
pub struct Pipeline {
  pub name: String,
  pub nodes: Vec<Node>,
}

impl Pipeline {
  pub fn from_yaml(yaml: &str) -> Result<Self, PipelineError> {
    match serde_saphyr::from_str(yaml) {
      Ok(pipeline) => Ok(pipeline),
      Err(e) => Err(PipelineError::YamlParseError(e.to_string())),
    }
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
}
