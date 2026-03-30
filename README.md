# Jefferies

Jefferies is the backend for The Conn, a CI/CD framework. It is a Cargo workspace composed of four crates.

## Crates

### config

Holds the application configuration model. This includes the choice of container executor, executor-specific settings, the storage backend connection, and the HTTP server address. Configuration is loaded at startup and threaded through the rest of the application.

### execution

Defines the core abstractions for running CI pipelines. A pipeline is an ordered sequence of nodes, each of which runs a series of shell steps inside a container image with an optional set of environment variables. Nodes within a pipeline share a workspace volume, so files and environment exports produced by one node are available to subsequent nodes.

The executor trait abstracts over container runtimes. The current implementation targets Podman.

### storage

Provides persistence for two concerns: a pipeline registry and a run history.

The pipeline registry stores named pipeline definitions. The run history records every pipeline execution and its individual node runs. Each run record carries a snapshot of the pipeline definition that was executed at the time of the run, so the historical record is unaffected by later changes or deletions to the registry.

### server

An Axum-based HTTP server that acts as the primary interface to the system. It exposes endpoints for managing the pipeline registry and for triggering and querying pipeline runs. Pipelines can be submitted inline as YAML, referenced by name from the registry, fetched from a URL, or pulled from a Git repository.
