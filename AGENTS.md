## Agents.md

This document outlines the coding standards and workflow expectations for contributors and automated agents.

---

### **Code Style & Readability**
* **No Comments:** Avoid inline comments. Code should be self-documenting through clear naming and logical flow.
* **Functional Decomposition:** Break large functions into smaller, focused helper functions.
    * Main functions should outline high-level logic.
    * Helper functions should handle specific implementation details to support reuse and readability.
* **Asynchronous Execution:** Since this is an **Axum** backend, logic should be non-blocking and utilize `async/await` where appropriate to handle concurrent requests efficiently.
* **Logging:** Use structured logging at appropriate levels (`info`, `warn`, `error`, `debug`) to aid in debugging and provide visibility into program execution.

### **Error Handling & Security**
* **No Panics:** `unwrap()` and `expect()` are strictly prohibited outside of test suites. Failures must be handled gracefully using proper error types.
* **Data Integrity:** All database interactions must be safe and protected against injection attacks.
* **Graceful Failures:** The system should remain resilient; a failure in one process should not bring down the entire server.

### **Testing & Documentation**
* **Targeted Testing:** Tests are required for new features, functional updates, and integration points where different modules connect.
    * *Note:* Purely structural changes (refactoring module locations, updating dependencies, or documentation tweaks) do not require new tests unless logic is altered.
* **Test Scope:** Focus on "large ideas," happy paths, and common failure modes rather than testing every individual helper function.
* **README Maintenance:** Keep the `README.md` updated with the project's high-level overview and architectural structure. Avoid deep implementation details; focus on helping new contributors understand the major modules.

### **Development Workflow**
A commit is only ready for review if it fulfills the following:

1.  **`make fmt`**: Code must be consistently formatted.
2.  **`make test`**: All existing and new tests must pass.
3.  **Documentation**: Relevant high-level changes are reflected in the README.

---

Check out [architecture.md](./docs/architecture.md) for a high level description of the architecture of this CI framework.
