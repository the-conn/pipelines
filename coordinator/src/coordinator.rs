use std::{collections::HashMap, sync::Arc};

use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{error, info, warn};

use crate::{
  dispatcher::Dispatcher,
  message::CoordinatorMessage,
  run::{NodeStatus, PipelineRun},
};

pub struct RunSummary {
  pub run_id: String,
  pub success: bool,
  pub cancelled: bool,
  pub node_statuses: HashMap<String, NodeStatus>,
}

struct Coordinator {
  run_id: String,
  run: PipelineRun,
  receiver: mpsc::Receiver<CoordinatorMessage>,
  dispatcher: Arc<dyn Dispatcher>,
}

pub fn start_coordinator(
  run_id: String,
  nodes: Vec<(String, Vec<String>)>,
  dispatcher: Arc<dyn Dispatcher>,
) -> (mpsc::Sender<CoordinatorMessage>, JoinHandle<RunSummary>) {
  let (sender, receiver) = mpsc::channel(128);
  let run = PipelineRun::new(&nodes);
  let coordinator = Coordinator {
    run_id,
    run,
    receiver,
    dispatcher,
  };
  let handle = tokio::spawn(coordinator.run());
  (sender, handle)
}

impl Coordinator {
  async fn run(mut self) -> RunSummary {
    info!(run_id = %self.run_id, "Coordinator starting");

    self.dispatch_ready_nodes().await;

    if self.run.is_complete() {
      return self.build_summary(false);
    }

    loop {
      match self.receiver.recv().await {
        Some(CoordinatorMessage::NodeCompleted { node_name, success }) => {
          self.handle_node_completed(&node_name, success).await;
          if self.run.is_complete() {
            break;
          }
        }
        Some(CoordinatorMessage::Cancel) => {
          return self.handle_cancellation().await;
        }
        None => {
          warn!(run_id = %self.run_id, "Coordinator channel closed unexpectedly");
          return self.handle_cancellation().await;
        }
      }
    }

    self.build_summary(false)
  }

  async fn handle_node_completed(&mut self, node_name: &str, success: bool) {
    if success {
      self.run.mark_success(node_name);
      info!(run_id = %self.run_id, node_name, "Node completed successfully");
    } else {
      self.run.mark_failed(node_name);
      warn!(run_id = %self.run_id, node_name, "Node failed");
    }
    self.dispatch_ready_nodes().await;
  }

  async fn dispatch_ready_nodes(&mut self) {
    for node_name in self.run.ready_nodes() {
      match self.dispatcher.dispatch(&self.run_id, &node_name).await {
        Ok(()) => {
          self.run.mark_running(&node_name);
          info!(run_id = %self.run_id, node_name = %node_name, "Node dispatched");
        }
        Err(e) => {
          error!(run_id = %self.run_id, node_name = %node_name, error = %e, "Failed to dispatch node");
          self.run.mark_failed(&node_name);
        }
      }
    }
  }

  async fn handle_cancellation(self) -> RunSummary {
    info!(run_id = %self.run_id, "Coordinator handling cancellation");
    let Coordinator {
      run_id,
      run,
      dispatcher,
      ..
    } = self;

    for (node_name, status) in run.statuses() {
      if *status == NodeStatus::Running {
        if let Err(e) = dispatcher.cancel_node(&run_id, node_name).await {
          warn!(run_id = %run_id, node_name, error = %e, "Failed to cancel node");
        }
      }
    }

    let node_statuses = run.statuses().clone();
    RunSummary {
      run_id,
      success: false,
      cancelled: true,
      node_statuses,
    }
  }

  fn build_summary(&self, cancelled: bool) -> RunSummary {
    let success = !cancelled
      && self
        .run
        .statuses()
        .values()
        .all(|s| *s == NodeStatus::Success);
    info!(
      run_id = %self.run_id,
      success,
      cancelled,
      "Coordinator completed"
    );
    RunSummary {
      run_id: self.run_id.clone(),
      success,
      cancelled,
      node_statuses: self.run.statuses().clone(),
    }
  }
}
