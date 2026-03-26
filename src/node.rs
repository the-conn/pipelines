use std::collections::HashMap;

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

enum Step {
  Command(String),
}

struct Node {
  name: String,
  image: String,
  inputs: HashMap<String, Input>,
  status: Status,
  steps: Vec<Step>,
}

impl Node {
  fn from_yaml(yaml: &str) -> Self {
    Node {
      name: String::new(),
      image: String::new(),
      inputs: HashMap::new(),
      status: Status::NotStarted,
      steps: Vec::new(),
    }
  }
}

const TEST_NODE_YAML: &str = "
test-node:
  image: alpine:latest
  inputs:
    message: Hello, World!
  steps:
    - echo ${message}
";

#[cfg(test)]
mod tests {
  use crate::yaml_parser;

  #[test]
  fn test_yaml_parse() {
    match yaml_parser::parse_yaml(super::TEST_NODE_YAML) {
      Ok(nodes) => {
        println!("Parsed nodes: {:#?}", nodes);
      }
      Err(e) => panic!("Failed to parse YAML: {}", e),
    }
  }
}
