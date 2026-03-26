use std::collections::HashMap;
use std::fs::read_to_string;
use std::iter::{Enumerate, Peekable};
use std::path::PathBuf;
use std::str::Lines;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum YamlError {
  #[error("Tabs are not allowed for indentation at line {0}")]
  TabsNotAllowed(usize),
  #[error("Unterminated string quote at line {0}")]
  UnterminatedString(usize),
  #[error("Invalid syntax at line {0}: {1}")]
  InvalidSyntax(usize, String),
  #[error("File read error: {0}")]
  FileReadError(#[from] std::io::Error),
}

#[derive(Debug, PartialEq)]
pub enum YamlScalar {
  String(String),
  Integer(i64),
  Float(f64),
  Boolean(bool),
  Null,
}

#[derive(Debug, PartialEq)]
pub enum YamlNode {
  Mapping(HashMap<String, YamlNode>),
  Sequence(Vec<YamlNode>),
  Scalar(YamlScalar),
}

pub fn parse_yaml_file(path: PathBuf) -> Result<Vec<YamlNode>, YamlError> {
  let content = read_to_string(path)?;
  parse_yaml(&content)
}

/// Parses a YAML string into a vector of top-level nodes. Each node can be a mapping, sequence, or scalar.
pub fn parse_yaml(yaml: &str) -> Result<Vec<YamlNode>, YamlError> {
  let mut lines = yaml.lines().enumerate().peekable();
  let mut nodes = Vec::new();

  while let Some(&(idx, line)) = lines.peek() {
    let line_num = idx + 1;

    if line.trim() == "---" {
      lines.next();
      continue;
    }

    match get_line_meta(line, line_num)? {
      None => {
        lines.next();
      }
      Some(_) => {
        nodes.push(parse_node(&mut lines, 0)?);
      }
    }
  }

  Ok(nodes)
}

fn parse_node(
  lines: &mut Peekable<Enumerate<Lines>>,
  indent: usize,
) -> Result<YamlNode, YamlError> {
  while let Some(&(idx, line)) = lines.peek() {
    if line.trim() == "---" {
      break;
    }

    let line_num = idx + 1;
    match get_line_meta(line, line_num)? {
      None => {
        lines.next();
      }
      Some(line_indent) => {
        if line_indent < indent {
          break;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with("- ") {
          return parse_sequence(lines, indent);
        } else if trimmed.contains(": ") || trimmed.ends_with(':') {
          return parse_mapping(lines, indent);
        } else {
          let (_, content) = lines.next().unwrap();
          return Ok(YamlNode::Scalar(parse_scalar(content, line_num)?));
        }
      }
    }
  }
  Ok(YamlNode::Scalar(YamlScalar::Null))
}

fn parse_mapping(
  lines: &mut Peekable<Enumerate<Lines>>,
  indent: usize,
) -> Result<YamlNode, YamlError> {
  let mut map = HashMap::new();

  while let Some(&(idx, line)) = lines.peek() {
    if line.trim() == "---" {
      break;
    }

    let line_num = idx + 1;
    match get_line_meta(line, line_num)? {
      None => {
        lines.next();
      }
      Some(line_indent) => {
        if line_indent < indent {
          break;
        }

        let (_, current_line) = lines.next().unwrap();
        if let Some((key, value_part)) = current_line.split_once(':') {
          let key = key.trim().to_string();
          let val_trimmed = value_part.trim();

          let node = if val_trimmed == "|" {
            YamlNode::Scalar(YamlScalar::String(parse_multiline_block(
              lines,
              indent + 2,
            )?))
          } else if val_trimmed.is_empty() {
            if let Some(&(next_idx, next_line)) = lines.peek() {
              let next_meta = get_line_meta(next_line, next_idx + 1)?;
              if let Some(next_indent) = next_meta {
                if next_indent > indent {
                  parse_node(lines, indent + 2)?
                } else {
                  YamlNode::Scalar(YamlScalar::Null)
                }
              } else {
                YamlNode::Scalar(YamlScalar::Null)
              }
            } else {
              YamlNode::Scalar(YamlScalar::Null)
            }
          } else {
            YamlNode::Scalar(parse_scalar(val_trimmed, line_num)?)
          };

          map.insert(key, node);
        }
      }
    }
  }
  Ok(YamlNode::Mapping(map))
}

fn parse_sequence(
  lines: &mut Peekable<Enumerate<Lines>>,
  indent: usize,
) -> Result<YamlNode, YamlError> {
  let mut sequence = Vec::new();

  while let Some(&(idx, line)) = lines.peek() {
    if line.trim() == "---" {
      break;
    }

    let line_num = idx + 1;
    match get_line_meta(line, line_num)? {
      None => {
        lines.next();
      }
      Some(line_indent) => {
        if line_indent < indent {
          break;
        }

        let (_, current_line) = lines.next().unwrap();
        let trimmed = current_line.trim_start();

        if let Some(content) = trimmed.strip_prefix("- ") {
          let content_trimmed = content.trim();
          if content_trimmed.is_empty() {
            sequence.push(parse_node(lines, indent + 2)?);
          } else {
            sequence.push(YamlNode::Scalar(parse_scalar(content_trimmed, line_num)?));
          }
        } else {
          break;
        }
      }
    }
  }
  Ok(YamlNode::Sequence(sequence))
}

fn parse_multiline_block(
  lines: &mut Peekable<Enumerate<Lines>>,
  block_indent: usize,
) -> Result<String, YamlError> {
  let mut result = String::new();

  while let Some(&(idx, line)) = lines.peek() {
    if line.trim().is_empty() {
      result.push('\n');
      lines.next();
      continue;
    }

    match get_line_meta(line, idx + 1)? {
      None => {
        lines.next();
      }
      Some(line_indent) => {
        if line_indent < block_indent {
          break;
        }
        let (_, current_line) = lines.next().unwrap();
        result.push_str(&current_line[block_indent..]);
        result.push('\n');
      }
    }
  }
  Ok(result.trim_end_matches('\n').to_string())
}

fn parse_scalar(raw: &str, line_num: usize) -> Result<YamlScalar, YamlError> {
  let trimmed = raw.trim();

  if trimmed.is_empty() || trimmed.to_lowercase() == "null" || trimmed == "~" {
    return Ok(YamlScalar::Null);
  }

  let first = trimmed.chars().next().unwrap();
  if first == '"' || first == '\'' {
    if trimmed.len() < 2 || !trimmed.ends_with(first) {
      return Err(YamlError::UnterminatedString(line_num));
    }
    return Ok(YamlScalar::String(
      trimmed[1..trimmed.len() - 1].to_string(),
    ));
  }

  let lower = trimmed.to_lowercase();
  if ["true", "yes", "on"].contains(&lower.as_str()) {
    return Ok(YamlScalar::Boolean(true));
  }
  if ["false", "no", "off"].contains(&lower.as_str()) {
    return Ok(YamlScalar::Boolean(false));
  }

  if let Ok(i) = trimmed.parse::<i64>() {
    return Ok(YamlScalar::Integer(i));
  }
  if let Ok(f) = trimmed.parse::<f64>() {
    return Ok(YamlScalar::Float(f));
  }

  Ok(YamlScalar::String(trimmed.to_string()))
}

fn get_line_meta(line: &str, line_num: usize) -> Result<Option<usize>, YamlError> {
  let mut indent = 0;
  let mut chars = line.chars().peekable();

  while let Some(&c) = chars.peek() {
    if c == ' ' {
      indent += 1;
      chars.next();
    } else if c == '\t' {
      return Err(YamlError::TabsNotAllowed(line_num));
    } else if c.is_whitespace() {
      return Err(YamlError::InvalidSyntax(
        line_num,
        format!("Illegal whitespace character detected: {:?}", c),
      ));
    } else {
      break;
    }
  }

  match chars.peek() {
    None | Some('#') => Ok(None),
    Some(_) => Ok(Some(indent)),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_standard_nesting() {
    let input = "
foo:
  bar: val
baz:
  list:
    - item1
";
    let result = parse_yaml(input).unwrap();
    assert_eq!(result.len(), 1);
    if let YamlNode::Mapping(m) = &result[0] {
      assert!(m.contains_key("foo"));
      assert!(m.contains_key("baz"));
    }
  }

  #[test]
  fn test_strict_tabs() {
    let input = "key:\n\tvalue";
    let result = parse_yaml(input);
    assert!(matches!(result, Err(YamlError::TabsNotAllowed(2))));
  }

  #[test]
  fn test_unterminated_quote() {
    let input = "key: \"val";
    let result = parse_yaml(input);
    assert!(matches!(result, Err(YamlError::UnterminatedString(1))));
  }

  #[test]
  fn test_empty_is_null() {
    let input = "key: ";
    let nodes = parse_yaml(input).unwrap();
    if let YamlNode::Mapping(m) = &nodes[0] {
      assert_eq!(m.get("key").unwrap(), &YamlNode::Scalar(YamlScalar::Null));
    }
  }

  #[test]
  fn test_quoted_empty_is_string() {
    let input = "key: \"\"";
    let nodes = parse_yaml(input).unwrap();
    if let YamlNode::Mapping(m) = &nodes[0] {
      assert_eq!(
        m.get("key").unwrap(),
        &YamlNode::Scalar(YamlScalar::String("".to_string()))
      );
    }
  }

  #[test]
  fn test_missing_space_after_colon() {
    let input = "foo:bar";
    let nodes = parse_yaml(input).unwrap();
    // Should be a single scalar, not a map
    assert_eq!(
      nodes[0],
      YamlNode::Scalar(YamlScalar::String("foo:bar".to_string()))
    );
  }

  #[test]
  fn test_multiline_block_with_siblings() {
    let input = "
description: |
  hello
  world
next_key: true
";
    let nodes = parse_yaml(input).unwrap();
    assert_eq!(nodes.len(), 1);

    if let YamlNode::Mapping(m) = &nodes[0] {
      match m.get("description").unwrap() {
        YamlNode::Scalar(YamlScalar::String(s)) => assert_eq!(s, "hello\nworld"),
        _ => panic!("description should be a string scalar"),
      }

      match m.get("next_key").unwrap() {
        YamlNode::Scalar(YamlScalar::Boolean(b)) => assert_eq!(*b, true),
        _ => panic!("next_key should be a boolean true"),
      }

      assert_eq!(m.len(), 2);
    } else {
      panic!("Top level should be a Mapping");
    }
  }

  #[test]
  fn test_scalar_type_inference() {
    let input = "
int_val: 42
negative_int: -100
float_val: 3.14
scientific_notation: 1e10
boolean_true: true
boolean_false: false
null_literal: null
tilde_null: ~
empty_null:
quoted_string: \"123\"
quoted_bool: 'true'
plain_string: hello world
";
    let nodes = parse_yaml(input).unwrap();
    assert_eq!(nodes.len(), 1);

    if let YamlNode::Mapping(m) = &nodes[0] {
      assert_eq!(
        m.get("int_val").unwrap(),
        &YamlNode::Scalar(YamlScalar::Integer(42))
      );
      assert_eq!(
        m.get("negative_int").unwrap(),
        &YamlNode::Scalar(YamlScalar::Integer(-100))
      );

      assert_eq!(
        m.get("float_val").unwrap(),
        &YamlNode::Scalar(YamlScalar::Float(3.14))
      );
      if let YamlNode::Scalar(YamlScalar::Float(f)) = m.get("scientific_notation").unwrap() {
        assert!((f - 1e10).abs() < f64::EPSILON);
      }

      assert_eq!(
        m.get("boolean_true").unwrap(),
        &YamlNode::Scalar(YamlScalar::Boolean(true))
      );
      assert_eq!(
        m.get("boolean_false").unwrap(),
        &YamlNode::Scalar(YamlScalar::Boolean(false))
      );

      assert_eq!(
        m.get("null_literal").unwrap(),
        &YamlNode::Scalar(YamlScalar::Null)
      );
      assert_eq!(
        m.get("tilde_null").unwrap(),
        &YamlNode::Scalar(YamlScalar::Null)
      );
      assert_eq!(
        m.get("empty_null").unwrap(),
        &YamlNode::Scalar(YamlScalar::Null)
      );

      assert_eq!(
        m.get("quoted_string").unwrap(),
        &YamlNode::Scalar(YamlScalar::String("123".to_string()))
      );
      assert_eq!(
        m.get("quoted_bool").unwrap(),
        &YamlNode::Scalar(YamlScalar::String("true".to_string()))
      );

      assert_eq!(
        m.get("plain_string").unwrap(),
        &YamlNode::Scalar(YamlScalar::String("hello world".to_string()))
      );
    }
  }

  #[test]
  fn test_multi_document_parsing() {
    let input = "
---
foo: bar
---
abc:
  - 123
";
    let nodes = parse_yaml(input).unwrap();
    assert_eq!(nodes.len(), 2);

    if let YamlNode::Mapping(m1) = &nodes[0] {
      assert_eq!(
        m1.get("foo").unwrap(),
        &YamlNode::Scalar(YamlScalar::String("bar".to_string()))
      );
    } else {
      panic!("First doc should be a mapping");
    }

    if let YamlNode::Mapping(m2) = &nodes[1] {
      if let YamlNode::Sequence(s) = m2.get("abc").unwrap() {
        assert_eq!(s[0], YamlNode::Scalar(YamlScalar::Integer(123)));
      } else {
        panic!("abc should be a sequence");
      }
    } else {
      panic!("Second doc should be a mapping");
    }
  }
}
