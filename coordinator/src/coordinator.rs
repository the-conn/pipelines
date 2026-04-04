use std::{collections::HashMap, sync::Arc, time::Duration};

use app_config::AppConfig;
use pipelines::{NodeInfo, Pipeline};
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
  pipeline: Arc<Pipeline>,
  config: Arc<AppConfig>,
  node_info_cache: HashMap<String, NodeInfo>,
  node_timeout_handles: HashMap<String, JoinHandle<()>>,
  receiver: mpsc::Receiver<CoordinatorMessage>,
  outbox: mpsc::Sender<CoordinatorMessage>,
  dispatcher: Arc<dyn Dispatcher>,
}

pub fn start_coordinator(
  run_id: String,
  pipeline: Arc<Pipeline>,
  config: Arc<AppConfig>,
  dispatcher: Arc<dyn Dispatcher>,
) -> (mpsc::Sender<CoordinatorMessage>, JoinHandle<RunSummary>) {
  let (sender, receiver) = mpsc::channel(128);
  let node_infos = pipeline.node_info();
  let run = PipelineRun::new(&node_infos);
  let node_info_cache = node_infos
    .into_iter()
    .map(|n| (n.name.clone(), n))
    .collect();
  let coordinator = Coordinator {
    run_id,
    run,
    pipeline,
    config,
    node_info_cache,
    node_timeout_handles: HashMap::new(),
    receiver,
    outbox: sender.clone(),
    dispatcher,
  };
  let handle = tokio::spawn(coordinator.run());
  (sender, handle)
}

impl Coordinator {
  async fn run(mut self) -> RunSummary {
    info!(run_id = %self.run_id, "Coordinator starting");

    let pipeline_timeout_secs = self
      .pipeline
      .pipeline_timeout_secs()
      .unwrap_or_else(|| self.config.default_pipeline_timeout_secs());
    let pipeline_deadline = tokio::time::sleep(Duration::from_secs(pipeline_timeout_secs));
    tokio::pin!(pipeline_deadline);

    self.dispatch_ready_nodes().await;

    if self.run.is_complete() {
      return self.build_summary(false);
    }

    loop {
      tokio::select! {
        _ = &mut pipeline_deadline => {
          warn!(
            run_id = %self.run_id,
            timeout_secs = pipeline_timeout_secs,
            "Pipeline timeout exceeded"
          );
          return self.handle_cancellation().await;
        }
        msg = self.receiver.recv() => {
          match msg {
            Some(CoordinatorMessage::NodeCompleted { node_name, success }) => {
              self.cancel_node_timeout(&node_name);
              if self.handle_node_completed(&node_name, success).await {
                return self.handle_cancellation().await;
              }
              if self.run.is_complete() {
                break;
              }
            }
            Some(CoordinatorMessage::NodeTimedOut { node_name }) => {
              if self.handle_node_timed_out(&node_name).await {
                return self.handle_cancellation().await;
              }
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
      }
    }

    self.build_summary(false)
  }

  fn cancel_node_timeout(&mut self, node_name: &str) {
    if let Some(handle) = self.node_timeout_handles.remove(node_name) {
      handle.abort();
    }
  }

  async fn handle_node_completed(&mut self, node_name: &str, success: bool) -> bool {
    if success {
      if !self.run.mark_success(node_name) {
        warn!(run_id = %self.run_id, node_name, "Unexpected state transition: node was not Running");
      } else {
        info!(run_id = %self.run_id, node_name, "Node completed successfully");
      }
      self.dispatch_ready_nodes().await;
      false
    } else {
      if !self.run.mark_failed(node_name) {
        warn!(run_id = %self.run_id, node_name, "Unexpected state transition: node was not Running");
      } else {
        warn!(run_id = %self.run_id, node_name, "Node failed");
      }
      if self.fail_fast_enabled() {
        warn!(run_id = %self.run_id, node_name, "Fail-fast enabled; cancelling pipeline");
        true
      } else {
        self.dispatch_ready_nodes().await;
        false
      }
    }
  }

  async fn handle_node_timed_out(&mut self, node_name: &str) -> bool {
    if !self.run.mark_failed(node_name) {
      return false;
    }
    warn!(run_id = %self.run_id, node_name, "Node timed out");
    if let Err(e) = self
      .dispatcher
      .cancel_node(&self.run_id, node_name, &self.config)
      .await
    {
      warn!(run_id = %self.run_id, node_name, error = %e, "Failed to cancel timed-out node");
    }
    if self.fail_fast_enabled() {
      warn!(run_id = %self.run_id, node_name, "Fail-fast enabled; cancelling pipeline");
      true
    } else {
      self.dispatch_ready_nodes().await;
      false
    }
  }

  fn fail_fast_enabled(&self) -> bool {
    self
      .pipeline
      .fail_fast_override()
      .unwrap_or_else(|| self.config.default_fail_fast())
  }

  async fn dispatch_ready_nodes(&mut self) {
    for node_name in self.run.ready_nodes() {
      let Some(node) = self.node_info_cache.get(&node_name) else {
        error!(run_id = %self.run_id, node_name = %node_name, "Node info not found in pipeline");
        self.run.mark_dispatch_failed(&node_name);
        continue;
      };

      match self
        .dispatcher
        .dispatch(&self.run_id, node, &self.pipeline, &self.config)
        .await
      {
        Ok(()) => {
          if !self.run.mark_running(&node_name) {
            warn!(run_id = %self.run_id, node_name = %node_name, "Unexpected state transition: node was not Pending");
          } else {
            info!(run_id = %self.run_id, node_name = %node_name, "Node dispatched");
            let timeout_handle = self.spawn_node_timeout(&node_name, node.timeout_secs);
            self.node_timeout_handles.insert(node_name, timeout_handle);
          }
        }
        Err(e) => {
          error!(run_id = %self.run_id, node_name = %node_name, error = %e, "Failed to dispatch node");
          self.run.mark_dispatch_failed(&node_name);
        }
      }
    }
  }

  fn spawn_node_timeout(&self, node_name: &str, override_secs: Option<u64>) -> JoinHandle<()> {
    let timeout_secs = override_secs.unwrap_or_else(|| self.config.default_node_timeout_secs());
    let outbox = self.outbox.clone();
    let name = node_name.to_string();
    tokio::spawn(async move {
      tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
      let _ = outbox
        .send(CoordinatorMessage::NodeTimedOut { node_name: name })
        .await;
    })
  }

  async fn handle_cancellation(mut self) -> RunSummary {
    info!(run_id = %self.run_id, "Coordinator handling cancellation");

    for (_, handle) in self.node_timeout_handles.drain() {
      handle.abort();
    }

    for (node_name, status) in self.run.statuses() {
      if *status == NodeStatus::Running {
        if let Err(e) = self
          .dispatcher
          .cancel_node(&self.run_id, node_name, &self.config)
          .await
        {
          warn!(run_id = %self.run_id, node_name, error = %e, "Failed to cancel node");
        }
      }
    }

    let node_statuses = self.run.statuses().clone();
    RunSummary {
      run_id: self.run_id,
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
