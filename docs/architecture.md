## **The Conn: Architecture Specification**

### **1. Core Philosophy**
* **Reactive & Distributed:** The system is a distributed state machine. Any server node in the cluster can progress a pipeline by interacting with the shared Redis state and RabbitMQ message backplane.
* **Stateless Ingress:** The **server** module is interchangeable. Traffic is balanced across replicas without losing track of active runs.
* **Resilient & Infrastructure-Blind:** The system assumes hardware and network failures. It self-heals via lease expiration and orphan reclamation, shielding the user from underlying instability.

---

### **2. Component & Module Breakdown**

#### **A. app_config (The Foundation)**
* **Layered Configuration:** Loads defaults from TOML and overrides them via environment variables using a double-underscore (`__`) separator for nested fields.
* **Secrets Management:** Handles sensitive credentials for **Redis**, **RabbitMQ**, and **GitHub** injected via OpenShift SecretKeyRefs.

#### **B. providers (The Gatekeeper)**
* **GitHub Logic:** Contains the HMAC-SHA256 signature verification logic to ensure webhooks are authentic.
* **Repository Access:** Manages GitHub App authentication (JWT and Installation Tokens) to read `.jefferies/` YAML files directly from the source repository via the Contents API.

#### **C. pipelines (The Logic)**
* **Discovery:** Parses YAML to match incoming events (e.g., `push`) to specific execution plans.
* **State Schema:** Defines the `RunTracker` structure stored in **Redis**, which tracks node status (Pending, Running, Success, Failed) and dependency resolution.

#### **D. coordinator (The Reactor)**
* **Distributed Lifecycle:** Manages the **Lease** and **Fencing Token** for a specific Run ID.
* **Event Handling:** Consumes "Step Complete" messages from **RabbitMQ**, updates the Redis state, and triggers the next set of unlocked nodes.
* **The Reaper:** A background task that identifies orphaned runs (active in Redis but without a heartbeating lease) and re-enqueues them for recovery.

#### **E. server (The Interface)**
* **Stateless Endpoints:** Axum handlers for GitHub webhooks and the secure status callbacks from **Tubes**.
* **Event Dispatch:** Validates requests and publishes the resulting state change to RabbitMQ, acting as the entry point for all external signals.

---

### **3. Data & Execution Flow**

1.  **Ingress:** A Webhook hits a **server** node. **providers** validates the signature and reads the pipeline YAML.
2.  **Initialization:** **pipelines** creates the initial state in **Redis**. **server** publishes a "Run Started" event to **RabbitMQ**.
3.  **Coordination:** A node in the **coordinator** module claims the Redis lease and becomes the temporary manager for that Run ID.
4.  **Execution Loop:**
    * **Tubes** finishes a task and hits the **server** status endpoint.
    * The **server** updates **Redis** and publishes to **RabbitMQ**.
    * The leasing **coordinator** reacts, identifies "unlocked" nodes, and triggers the next Pods.
5.  **Cleanup:** Once all nodes reach a terminal state, the lease is released and the state is archived.

---

### **4. Resiliency & Error Handling**

| Scenario | System Response |
| :--- | :--- |
| **Server Instance Failure** | The Redis **Lease** expires. The **coordinator's** Reaper detects the orphan and publishes a recovery event. |
| **Network Partition** | **Fencing Tokens** ensure that "Zombie" servers cannot write outdated state to Redis once a new leader is established. |
| **Worker Pod Failure** | **Tubes** reports a `Fail` status. The **coordinator** marks the node as failed and halts downstream execution for that branch. |
| **Node Deletion** | A K8s Watcher in the background detects the missing Pod; the **coordinator** reconciles the state and marks it as a system error. |

---

### **5. Key Operational Concepts**
* **Externalized State:** No local RAM or disk is used for run-time data. Redis and RabbitMQ provide the cluster's "shared brain."
* **Idempotent Dispatch:** Before starting a Pod, the system checks OpenShift to ensure a Pod for that `RunID/Step` doesn't already exist.
* **Rehydration:** Any **coordinator** can resume any pipeline by reading the current snapshot from Redis, ensuring zero downtime during rolling updates of the backend.
