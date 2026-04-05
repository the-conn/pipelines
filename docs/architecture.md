## **The Conn: Architecture Specification**

### **1. Core Philosophy**
* **Reactive & Event-Driven:** The system reacts to state changes (webhooks, pod completions) rather than following a static, pre-defined schedule.
* **Distributed & Fault-Tolerant:** Multiple server instances can run simultaneously; distributed leases and durable state ensure exactly-one execution and crash recovery.
* **Shared-Nothing Execution:** Every node starts with a fresh workspace. State and artifacts are moved via S3-compatible storage (**Noobaa**).
* **Multi-Pipeline Discovery:** Jefferies treats the `.jefferies/` folder as a collection of potential execution plans, selecting the right one based on the incoming event.

---

### **2. Component Breakdown**

#### **A. Jefferies (The Brain)**
* **Pipeline Discovery:** On a webhook (Push/PR), Jefferies scans all files in the repository's `.jefferies/` folder. It parses every YAML file to find the one whose `on` triggers match the event (e.g., a `pull_request` on branch `main`).
* **The Reactive Engine:** Instead of a rigid DAG, Jefferies maintains a **"To-Run" Queue** and a **"Completed" List**.
* **Scan-and-Release Logic:** Every time a status update arrives, Jefferies scans the "To-Run" queue. Any node whose `after` requirements are now fully present in the "Completed" list is scheduled immediately.

#### **B. State Store (`state_store` crate)**
* Backed by **Redis** in production, with an `InMemoryStateStore` for testing.
* Stores durable `RunState` (node statuses, dependencies, pipeline definition) keyed by `run_id`.
* Implements **optimistic concurrency / version fencing** via Lua scripts: a save is only accepted if the caller's expected version matches the stored version.
* Manages **distributed leases** per run: only one server instance may coordinate a given run at a time. Leases have TTLs and must be renewed via heartbeat.

#### **C. Backplane (`backplane` crate)**
* Backed by **RabbitMQ** in production (topic exchange, per-run queues), with an `InMemoryBackplane` for testing.
* Decouples event producers (e.g., `Jefferies Tubes` calling `POST /runs/{id}/status`) from the coordinator that owns a run.
* Events: `StepFinished { node_name, success }` and `Cancel`.

#### **D. Coordinator (`coordinator` crate)**
* Per-run async `tokio` task managing the lifecycle of a specific pipeline run.
* Acquires a distributed lease before starting; renews it via heartbeat every 15 seconds.
* Persists state to the `StateStore` after every node transition (with version fencing).
* Listens to the `Backplane` for node completion events.
* Handles node timeouts, fail-fast logic, and graceful cancellation.

#### **E. Reaper**
* A background task that polls for "orphaned runs": runs with `Running` nodes but no active lease.
* Reclaims orphaned runs by re-acquiring the lease and resuming the coordinator from persisted state.

#### **F. Jefferies Tubes (The Ghost Binary)**
* An init-container and sidecar wrapper for every K8s Pod.
* **Lifecycle:** `Fresh Clone` → `Pull Artifacts (S3)` → `Run User Steps` → `Push Artifacts (S3)` → `Callback to Jefferies`.
* **Security:** Uses a single-use token mounted via a `tmpfs` Secret volume to authenticate the callback.

#### **G. The Executor (The Muscle)**
* A "dumb" worker using `kube-rs` to interface with OpenShift.
* Transforms `ExecutableNode` instructions into Pod manifests.
* **Resilience:** Monitors for Pod deletions via a K8s Watcher to trigger "System Error" states or clean retries.

---

### **3. Data & Execution Flow**

1. **Trigger:** A Webhook (e.g., Push to `prod`) hits Jefferies.
2. **Discovery:** Jefferies finds `.jefferies/integration.yaml` because its `on: push` matches.
3. **Lease Acquisition:** The server acquires a distributed lease in Redis for the new `run_id`.
4. **Initialization:** Initial `RunState` is persisted to Redis. All nodes with no dependencies are dispatched.
5. **Reactive Loop:**
   * **Tubes** finishes a node and hits `POST /runs/{id}/status`.
   * Jefferies publishes a `StepFinished` event to RabbitMQ.
   * The coordinator (holding the lease) receives the event and updates state.
   * Jefferies scans the remaining queue and releases any nodes that are now "unlocked."
6. **Completion:** The coordinator cleans up the lease and run state when all nodes finish.
7. **Crash Recovery:** If a server crashes mid-run, the **Reaper** detects the orphaned run (Running nodes, no lease) and reclaims it on the next polling cycle.

---

### **4. Error & Abort Handling**

| Scenario | System Response |
| :--- | :--- |
| **Manual Abort** | A `Cancel` event is published to the backplane. The coordinator cancels all active Pods and terminates. |
| **Node Failure** | Backplane reports `StepFinished { success: false }`. With fail-fast, the pipeline is cancelled. Without fail-fast, downstream nodes remain blocked. |
| **Pod Vanishes** | The Watcher detects the missing Pod. Jefferies can safely re-trigger a fresh Pod because the workspace is ephemeral. |
| **Server Crash** | The lease TTL expires. The Reaper detects the orphaned run and resumes it on another server instance. |
| **Version Conflict** | The state store rejects the save. The coordinator stops to avoid split-brain execution. |

---

### **5. Key Concepts to Remember**
* **Encapsulation:** The internal pipeline structs are private. The Executor only sees a flat list of instructions per node.
* **No Shared State:** No shared PVCs. If Node B needs Node A's output, it must be declared as an artifact and passed through **Noobaa**.
* **Speed:** Because of the "Scan-and-Release" logic, nodes run the exact millisecond their specific dependencies are met, ignoring unrelated slow nodes.
* **Exactly-Once Coordination:** Distributed leases ensure only one server coordinates each run at a time.

---

### **6. Module Boundaries**
* `pipelines`: Pipeline YAML parsing, `NodeInfo` types.
* `state_store`: `RunState` persistence (Redis + in-memory), distributed leases, `StateStore` trait.
* `backplane`: Event pub/sub for run events (RabbitMQ + in-memory), `Backplane` trait.
* `coordinator`: Per-run coordinator task, `Dispatcher` trait, `LogDispatcher`, `start_reaper`.
* `providers`: GitHub webhook handler, `ProviderState`.
* `server`: Axum server, HTTP handlers.
* `app_config`: Configuration loading.
