## **The Conn: Architecture Specification (v1.2)**

### **1. Core Philosophy**
* **Reactive & Event-Driven:** The system reacts to state changes (webhooks, pod completions) rather than following a static, pre-defined schedule.
* **Shared-Nothing Execution:** Every node starts with a fresh `emptyDir` workspace. State and artifacts are moved via S3-compatible storage (**Noobaa**).
* **Multi-Pipeline Discovery:** **Jefferies** (the backend) treats the `.jefferies/` folder as a collection of potential execution plans, selecting the right one based on the incoming event.

---

### **2. Component Breakdown**

#### **A. Jefferies (The Brain)**
* **Pipeline Discovery:** On a webhook (Push/PR), Jefferies scans all files in the repository's `.jefferies/` folder. It parses every YAML file to find the one whose `on` triggers match the event (e.g., a `pull_request` on branch `main`).
* **The Reactive Engine:** Instead of a rigid DAG, Jefferies maintains a **"To-Run" Queue** and a **"Completed" List**.
* **Scan-and-Release Logic:** Every time a status update arrives, Jefferies scans the "To-Run" queue. Any node whose `after` requirements are now fully present in the "Completed" list is scheduled immediately.

#### **B. Jefferies Tubes (The Ghost Binary)**
* An init-container and sidecar wrapper for every K8s Pod.
* **Lifecycle:** `Fresh Clone` $\rightarrow$ `Pull Artifacts (S3)` $\rightarrow$ `Run User Steps` $\rightarrow$ `Push Artifacts (S3)` $\rightarrow$ `Callback to Jefferies`.
* **Security:** Uses a single-use token mounted via a `tmpfs` Secret volume to authenticate the callback.

#### **C. The Executor (The Muscle)**
* A "dumb" worker using `kube-rs` to interface with OpenShift.
* Transforms `ExecutableNode` instructions into Pod manifests.
* **Resilience:** Monitors for Pod deletions via a K8s Watcher to trigger "System Error" states or clean retries.

---

### **3. Data & Execution Flow**



1.  **Trigger:** A Webhook (e.g., Push to `prod`) hits Jefferies.
2.  **Discovery:** Jefferies finds `.jefferies/integration.yaml` because its `on: push` matches.
3.  **Initialization:** All nodes with no `after` dependencies are scheduled.
4.  **Reactive Loop:**
    * **Tubes** finishes a node and hits `POST /status`.
    * **Jefferies** updates the run state.
    * **Jefferies** scans the remaining queue and releases any nodes that are now "unlocked."
5.  **Completion:** The run finishes when the queue is empty.

---

### **4. Error & Abort Handling**

| Scenario | System Response |
| :--- | :--- |
| **Manual Abort** | Jefferies issues a `DELETE` for all active Pods. The "To-Run" queue is purged. |
| **Node Failure** | Tubes reports `Fail`. Downstream nodes stay in the "To-Run" queue indefinitely (Blocked). |
| **Pod Vanishes** | The Watcher detects the missing Pod. Jefferies can safely re-trigger a fresh Pod because the workspace is ephemeral. |

---

### **5. Key Concepts to Remember**
* **Encapsulation:** The internal pipeline structs are private. The Executor only sees a flat list of instructions per node.
* **No Shared State:** No shared PVCs. If Node B needs Node A's output, it must be declared as an artifact and passed through **Noobaa**.
* **Speed:** Because of the "Scan-and-Release" logic, nodes run the exact millisecond their specific dependencies are met, ignoring unrelated slow nodes.

---

### **6. Current Module Boundaries**
* `pipelines`: Private data structures, YAML discovery, and "Scan-and-Release" logic.
* `executor`: K8s Pod generation and `kube-rs` interaction.
* `api`: Axum handlers for Webhooks and the secure Tubes status callback.
* `coordinator`: The async `tokio` task managing the lifecycle of a specific Run.
