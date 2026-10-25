# Podoxide

Podoxide (`pods`) is a Linux CLI tool written in pure standard Rust to manage Podman pods and container lifecycles deterministically using the block-based PodScript Language (`PSL`).

## Overview

Podoxide replaces complex orchestration wrappers and shell scripts with a single compiled binary and declarative recipes (`*.pod`). It translates recipes into direct `podman` operating system processes in a predictable order.

- **Zero External Dependencies:** Implemented strictly with the Rust standard library (`std`). `Cargo.toml` contains zero third-party runtime or build dependencies.
- **Direct Process Execution:** Spawns `podman` commands directly via `std::process::Command` without shell interpolation (`sh -c`).
- **Deterministic Lifecycle:** Creates and starts containers in top-to-bottom declaration order (`up`) and stops and removes them in reverse bottom-to-top order (`down`).
- **Unified Block DSL (`PSL`):** Uses a single-pass, zero-copy scanner for both application configuration (`pods.conf`) and pod recipes (`*.pod`).
- **Target Grouping:** Resolves individual recipes and nested `@group` targets with cycle detection.
- **Simulation & Diagnostics:** Previews generated `podman` commands via `--simulate` and validates host paths and configuration settings before execution.

## Requirements

| Component | Requirement |
| :--- | :--- |
| Operating System | Linux |
| Container Engine | Podman 5.8.0+ (rootful or rootless) |
| Build Toolchain | Rust (`rustc`, `cargo`, Edition 2024) |
| External Crates | None (`std` only) |

## Installation

Clone the repository and compile the release binary:

```bash
git clone https://github.com/akjir/podoxide.git
cd podoxide
cargo build --release
sudo install -m 755 target/release/pods /usr/local/bin/pods
```

Enable Bash tab-completion (optional):

```bash
source .pods-completion.bash
```

## Quick Start

1. Initialize a workspace configuration (`pods.conf`):

```bash
pods init
```

2. Validate the configuration and inspect resolved paths:

```bash
pods config inspect
```

3. Preview and start a recipe or `@group`:

```bash
pods --simulate up example-stack
pods up example-stack
```

4. Check status or tear down the pod:

```bash
pods status
pods down example-stack
```

## CLI Overview & Documentation

Podoxide uses a mode-driven CLI syntax:

```text
pods [global-flags] <lifecycle-action> [action-flags] <target...>
pods [global-flags] <mode> <sub-action> [action-flags] [arguments...]
```

| Mode / Scope | Alias | Primary Purpose |
| :--- | :--- | :--- |
| *(default)* | - | Manage pod lifecycles (`up`, `down`, `create`, `remove`, `start`, `stop`, `restart`, `reload`, `update`, `status`). |
| `init` | - | Initialize a default `pods.conf` file. |
| `config` | `cfg` | Validate, display, or edit `pods.conf`. |
| `recipe` | `rec` | List, inspect, create, or edit `*.pod` recipes. |
| `image` | `img` | Pull or prune container images defined in recipes. |
| `command` | `cmd` | List or execute maintenance commands defined inside recipes. |
| `connect` | `con` | Open an interactive shell inside a running container. |
| `logs` | `log` | Show or follow container logs for a recipe. |

For complete documentation, refer to the dedicated specifications in this repository:
- **[USAGE.md](USAGE.md):** Complete CLI reference, mode sub-actions, flags, and operational workflows.
- **[PSL_SPEC.md](PSL_SPEC.md):** Normative lexical and structural specification for `pods.conf` and `*.pod` files.
- **[ARCHITECTURE.md](ARCHITECTURE.md):** Internal module structure, I/O isolation rules, and design principles.
- **[CHANGELOG.md](CHANGELOG.md):** Version history and release notes.

## Development

Build the binary in debug mode:

```bash
cargo build
```

Run the unit and integration test suite:

```bash
cargo test
```

Run static analysis (must pass with zero warnings):

```bash
cargo clippy -- -D warnings
```

## Disclaimer & License

This software is provided "as is", without warranty of any kind, express or implied. Use in production environments is at your own risk.

Licensed under the **GNU General Public License v3.0 (GPL-3.0)**. See [LICENSE](LICENSE) for details.
