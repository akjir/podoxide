# Podoxide

Podoxide (`pods`) is a Linux CLI tool written in pure standard Rust to manage Podman pods and container lifecycles deterministically using the block-based PodScript Language (`PSL`).

## Overview

Podoxide replaces complex orchestration wrappers and shell scripts with a single compiled binary and declarative recipes (`*.pod`). It translates recipes into direct `podman` operating system processes in a predictable order.

- **Zero External Dependencies:** Implemented strictly with the Rust standard library (`std`). `Cargo.toml` contains zero third-party runtime or build dependencies.
- **Direct Process Execution:** Spawns `podman` commands directly via `std::process::Command` without shell interpolation (`sh -c`).
- **Deterministic Lifecycle:** Creates and starts containers in top-to-bottom declaration order (`up`) and stops and removes them in reverse bottom-to-top order (`down`).
- **Unified Block DSL (`PSL`):** Uses a single-pass, zero-copy scanner for both application configuration (`pods.conf`) and pod recipes (`*.pod`).
- **Target Grouping:** Resolves individual recipes and nested `@group` targets with cycle detection.
- **Simulation & Diagnostics:** Previews generated `podman` commands via `--simulate`, validates recipe path syntax before execution, and checks host directory existence via `pods config inspect`.

## Requirements

| Component | Requirement |
| :--- | :--- |
| Operating System | Linux |
| Container Engine | Podman 5.8.0+ (rootful or rootless) |
| Build Toolchain | Rust 1.88+ (`rustc`, `cargo`, Edition 2024) |
| External Crates | None (`std` only) |

## Installation

Download a prebuilt static Linux archive from [GitHub Releases](https://github.com/akjir/podoxide/releases) (verified with `SHA256SUMS` and `gh attestation verify`), or build from source (or install into `~/.cargo/bin` with `cargo install --path .`):

```bash
git clone https://github.com/akjir/podoxide.git
cd podoxide
cargo build --release
sudo install -m 755 target/release/pods /usr/local/bin/pods
```

Enable Bash tab-completion for the current session or install it persistently:

```bash
source .pods-completion.bash
install -Dm 644 .pods-completion.bash ~/.local/share/bash-completion/completions/pods
```

## Quick Start

1. Initialize workspace templates (`pods.conf` with `simulate true` and `recipe.pod`):

```bash
pods init
```

2. Validate the configuration and inspect resolved paths:

```bash
pods config inspect
```

3. Preview the generated Podman commands, then set `simulate false` in `pods.conf` to run live:

```bash
pods --simulate up recipe
pods up recipe
```

4. Check container status or tear down the pod:

```bash
pods status
pods down recipe
```

## CLI Overview & Documentation

Podoxide uses a mode-driven CLI syntax with flexible flag placement and optional sub-actions:

```text
pods [OPTIONS] <lifecycle-action> [TARGETS...]
pods [OPTIONS] <mode> [sub-action] [ARGUMENTS...]
```

| Mode / Scope | Alias | Primary Purpose |
| :--- | :--- | :--- |
| *(default)* | - | Manage pod lifecycles (`up`, `down`, `create`, `remove`, `start`, `stop`, `restart`, `reload`, `update`, `status`, `help`, `version`). |
| `init` | - | Initialize default `pods.conf` and `recipe.pod` template files. |
| `config` | `cfg` | Validate, display, or edit `pods.conf`. |
| `recipe` | `rec` | List, inspect, show, or edit `*.pod` recipes. |
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

Build, format-check, lint, and test the workspace:

```bash
cargo build
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Disclaimer & License

This software is provided "as is", without warranty of any kind, express or implied. Use in production environments is at your own risk.

Licensed under the **GNU General Public License 3.0 or later (GPL-3.0-or-later)**. See [LICENSE](LICENSE) for details.
