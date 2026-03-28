# pipelines

This repository is the backend for **The Conn**, a CI/CD framework built in Rust.

## Overview

The project is structured as a Cargo workspace with the following crates:

- **config** - configuration loading and management
- **execution** - pipeline execution engine
- **server** - HTTP controller (planned)

## Execution Engine

The execution engine defines the core abstractions for running pipelines: pipelines, nodes, and runs. A pipeline is an ordered sequence of nodes, each of which executes a series of steps inside a container image.

The current implementation targets **Podman** as the container runtime. A Kubernetes execution engine is planned but not yet implemented.

## Planned Work

- Kubernetes execution engine
- Axum-based HTTP server to act as the controller and interface between pipelines and the broader CI/CD framework
