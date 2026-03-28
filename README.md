# pipelines

This repository is the backend for **The Conn**, a CI/CD framework built in Rust.

## Overview

The project is structured as a Cargo workspace with the following crates:

- **config** - configuration loading and management
- **execution** - pipeline execution engine
- **storage** - persistence abstraction layer
- **server** - HTTP controller (planned)

## Execution Engine

The execution engine defines the core abstractions for running pipelines: pipelines, nodes, and runs. A pipeline is an ordered sequence of nodes, each of which executes a series of steps inside a container image.

The current implementation targets **Podman** as the container runtime. A Kubernetes execution engine is planned but not yet implemented.

## Storage

The `storage` crate provides an async `Storage` trait that decouples persistence logic from server code, making it straightforward to swap backend implementations without touching application logic.

### Storage trait

The trait covers two main concerns:

**Pipeline Registry** — tracks which pipelines exist independently of whether they have been run:
- `register_pipeline` — create or overwrite a pipeline definition (name, YAML source, timestamps)
- `get_pipeline` — fetch a single definition by name
- `list_pipelines` — return all registered pipelines
- `delete_pipeline` — remove a pipeline and all its associated run history

**Execution History** — an audit trail of pipeline executions:
- `save_pipeline_run` / `save_node_run` — persist completed runs
- `list_runs(limit)` — most recent runs across all pipelines
- `get_runs_by_pipeline(name)` — full history for one pipeline
- `get_run_details(run_id)` — full run record including all node runs
- `update_run_status(run_id, status)` — overwrite a run's status (e.g. mark as `Aborted`)

### Definition snapshots

Every time a pipeline or node run is saved, the exact definition that was executed is captured as an immutable JSON snapshot. This snapshot is stored independently of the pipeline registry, so updating or deleting a pipeline never alters the historical record of what actually ran. The `PipelineRun` struct carries a `pipeline: Pipeline` field as the canonical source of truth for the definition it executed, mirroring the `JobRun { node: Node }` shape.

### SQLite implementation

`SqliteStorage` is the initial provider, suitable for local development. It accepts any SQLite connection URL (including `sqlite::memory:` for tests) and applies its schema inline on construction — no migration tooling required. A future PostgreSQL provider is planned for Kubernetes deployments.

## Planned Work

- Kubernetes execution engine
- Axum-based HTTP server to act as the controller and interface between pipelines and the broader CI/CD framework
- PostgreSQL storage provider for Kubernetes deployments
