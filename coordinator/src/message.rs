#[derive(Debug)]
pub enum CoordinatorMessage {
  NodeCompleted { node_name: String, success: bool },
  Cancel,
}
