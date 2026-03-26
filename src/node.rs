use std::collections::HashMap;
use thiserror::Error;

use crate::yaml_parser;

#[derive(Error, Debug)]
pub enum NodeError {
  #[error("YAML parsing error: {0}")]
  YamlError(#[from] yaml_parser::YamlError),
  #[error("Expected exactly one top-level node, found {0}")]
  TooManyTopLevelNodes(usize),
  #[error("Missing required field: {0}")]
  MissingField(String),
  #[error("Unexpected field in YAML: {0}")]
  UnexpectedField(String),
  #[error("Invalid type for field '{0}': expected {1}")]
  InvalidType(String, String),
}

#[derive(Debug, PartialEq)]
enum Input {
  String(String),
}

enum Output {
  String(String),
  Stdout(String),
  Stderr(String),
  ExitCode(i32),
}

enum Status {
  Success,
  Failure,
  Skipped,
  Aborted,
  InProgress,
  NotStarted,
}

#[derive(Debug)]
enum Step {
  Command(String),
}

#[derive(Debug)]
pub struct Node {
  name: String,
  image: String,
  inputs: HashMap<String, Input>,
  steps: Vec<Step>,
}

impl Node {
  pub fn from_yaml(yaml: &str) -> Result<Self, NodeError> {
    let mut nodes = yaml_parser::parse_yaml(yaml)?;
    if nodes.len() != 1 {
      return Err(NodeError::TooManyTopLevelNodes(nodes.len()));
    }

    let root = nodes.remove(0);
    let mut map = match root {
      yaml_parser::YamlNode::Mapping(m) => m,
      _ => {
        return Err(NodeError::InvalidType(
          "root".to_string(),
          "Mapping".to_string(),
        ));
      }
    };

    let name = match map.remove("name") {
      Some(yaml_parser::YamlNode::Scalar(yaml_parser::YamlScalar::String(s))) => s,
      Some(_) => {
        return Err(NodeError::InvalidType(
          "name".to_string(),
          "String".to_string(),
        ));
      }
      None => return Err(NodeError::MissingField("name".to_string())),
    };

    let image = match map.remove("image") {
      Some(yaml_parser::YamlNode::Scalar(yaml_parser::YamlScalar::String(s))) => s,
      Some(_) => {
        return Err(NodeError::InvalidType(
          "image".to_string(),
          "String".to_string(),
        ));
      }
      None => return Err(NodeError::MissingField("image".to_string())),
    };

    let mut inputs = HashMap::new();
    if let Some(node) = map.remove("inputs") {
      if let yaml_parser::YamlNode::Mapping(input_map) = node {
        for (k, v) in input_map {
          if let yaml_parser::YamlNode::Scalar(yaml_parser::YamlScalar::String(s)) = v {
            inputs.insert(k, Input::String(s));
          } else {
            return Err(NodeError::InvalidType(
              format!("inputs.{}", k),
              "String".to_string(),
            ));
          }
        }
      } else {
        return Err(NodeError::InvalidType(
          "inputs".to_string(),
          "Mapping".to_string(),
        ));
      }
    }

    let mut steps = Vec::new();
    let steps_node = map
      .remove("steps")
      .ok_or_else(|| NodeError::MissingField("steps".to_string()))?;

    if let yaml_parser::YamlNode::Sequence(seq) = steps_node {
      for step_node in seq {
        if let yaml_parser::YamlNode::Scalar(yaml_parser::YamlScalar::String(cmd)) = step_node {
          steps.push(Step::Command(cmd));
        } else {
          return Err(NodeError::InvalidType(
            "step item".to_string(),
            "String".to_string(),
          ));
        }
      }
    } else {
      return Err(NodeError::InvalidType(
        "steps".to_string(),
        "Sequence".to_string(),
      ));
    }

    if let Some(extra_key) = map.keys().next() {
      return Err(NodeError::UnexpectedField(extra_key.clone()));
    }

    Ok(Node {
      name,
      image,
      inputs,
      steps,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_node_parsing_with_inputs() {
    let yaml = "
name: 'Test Node'
image: 'alpine:latest'
inputs:
  target_dir: '/tmp/output'
steps:
  - 'mkdir -p ${{ inputs.target_dir }}'
  - 'ls ${{ inputs.target_dir }}'
";

    let node = Node::from_yaml(yaml).expect("Should parse successfully");

    assert_eq!(node.name, "Test Node");
    assert!(node.inputs.contains_key("target_dir"));

    // Verify the command contains the input placeholder
    let Step::Command(cmd) = &node.steps[0];
    assert!(cmd.contains("${{ inputs.target_dir }}"));
  }

  #[test]
  fn test_node_unexpected_field() {
    let yaml = "
name: 'Fail'
image: 'alpine'
steps: []
wrong_field: 'this should cause an error'
";
    let result = Node::from_yaml(yaml);

    // Debug print to see what the parser actually produced
    match &result {
      Ok(node) => println!("Parsed successfully (oops!): {:?}", node),
      Err(e) => println!("Got error: {:?}", e),
    }

    assert!(matches!(result, Err(NodeError::UnexpectedField(ref f)) if f == "wrong_field"));
  }
}
