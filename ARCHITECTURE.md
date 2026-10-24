# Podoxide Architecture and File Structure

This document defines the module hierarchy, layer boundaries, and file responsibilities for Podoxide (`pods`).

> [!IMPORTANT]
> Update this document and [`AGENTS.md`](AGENTS.md) whenever you add, rename, move, or remove modules or files.

## Architectural Layers

Podoxide separates pure domain logic from operating system interactions across three primary modules under `src/`:

| Layer            | Path             | Responsibility   |
| :--------------- | :--------------- | :--------------- |
| **Core**         | `src/core/`      | Domain models, CLI argument parsing, runtime context, logging, and mode orchestration. |
| **I/O & System** | `src/io/`        | Process execution (`std::process::Command`), PSL (`.pod` / `pods.conf`) parsing, and filesystem I/O. |
| **Utilities**    | `src/utilities/` | Stateless string, path, formatting, and collection helper functions. |

> [!NOTE]
> Only modules inside `src/io/` execute external processes or access the filesystem directly (`P-IO-ISOLATION`). Modules in `src/core/` and `src/utilities/` remain pure and deterministic for unit testing.

## Directory Layout

```text
podoxide/
├── .pods-completion.bash
├── AGENTS.md
├── ARCHITECTURE.md
├── Cargo.toml
├── CHANGELOG.md
├── LICENSE
├── pods.conf
├── recipe.pod
├── README.md
├── USAGE.md
└── src/
    ├── error.rs
    ├── lib.rs
    ├── main.rs
    ├── core/
    │   ├── cli.rs
    │   ├── config.rs
    │   ├── container.rs
    │   ├── context.rs
    │   ├── log.rs
    │   ├── mod.rs
    │   ├── pod.rs
    │   ├── recipe.rs
    │   └── modes/
    │       ├── mod.rs
    │       ├── command/
    │       │   ├── exec.rs
    │       │   ├── help.rs
    │       │   ├── list.rs
    │       │   └── mod.rs
    │       ├── config/
    │       │   ├── edit.rs
    │       │   ├── help.rs
    │       │   ├── mod.rs
    │       │   └── show.rs
    │       ├── connect/
    │       │   ├── help.rs
    │       │   ├── mod.rs
    │       │   └── shell.rs
    │       ├── default/
    │       │   ├── create.rs
    │       │   ├── down.rs
    │       │   ├── help.rs
    │       │   ├── mod.rs
    │       │   ├── reload.rs
    │       │   ├── remove.rs
    │       │   ├── restart.rs
    │       │   ├── start.rs
    │       │   ├── status.rs
    │       │   ├── stop.rs
    │       │   ├── up.rs
    │       │   └── update.rs
    │       ├── image/
    │       │   ├── help.rs
    │       │   ├── mod.rs
    │       │   ├── prune.rs
    │       │   └── update.rs
    │       ├── init/
    │       │   ├── create.rs
    │       │   ├── help.rs
    │       │   └── mod.rs
    │       ├── logs/
    │       │   ├── follow.rs
    │       │   ├── help.rs
    │       │   ├── mod.rs
    │       │   └── show.rs
    │       └── recipe/
    │           ├── edit.rs
    │           ├── help.rs
    │           ├── list.rs
    │           ├── mod.rs
    │           └── show.rs
    ├── io/
    │   ├── mod.rs
    │   ├── exec/
    │   │   ├── command.rs
    │   │   ├── mod.rs
    │   │   └── podman.rs
    │   ├── psl/
    │   │   ├── ast.rs
    │   │   ├── mod.rs
    │   │   └── parser.rs
    │   └── system/
    │       ├── env.rs
    │       ├── fs.rs
    │       └── mod.rs
    └── utilities/
        ├── collection.rs
        ├── format.rs
        ├── mod.rs
        ├── path.rs
        └── string.rs
```

## Root Files and Templates

| File                    | Function |
| :---------------------- | :------- |
| `Cargo.toml`            | Defines crate metadata, the `podoxide` library target, the `pods` binary target, and release optimizations. |
| `pods.conf`             | Provides the root configuration template embedded and written by `pods init`. |
| `recipe.pod`            | Provides the root recipe template embedded and written by `pods init`. |
| `.pods-completion.bash` | Provides Bash autocompletion for all CLI modes, actions, and flags. |
| `src/main.rs`           | Serves as the binary entry point (`pods`), delegates execution to `podoxide`, and returns `ExitCode`. |
| `src/lib.rs`            | Serves as the library root, enforces `#![forbid(unsafe_code)]`, and exports `core`, `error`, `io`, and `utilities`. |
| `src/error.rs`          | Defines the central `CliError`, `ConfigError`, `DiagnosticError`, `PodError`, `PslError`, `RecipeError`, and `SystemError` domain error types, `build_caret_annotation`, and implements `Error` and `Display`. |

## Core Domain (`src/core/`)

The `src/core/` directory contains application state, domain models, argument parsing, and top-level orchestration.

| File                    | Function |
| :---------------------- | :------- |
| `src/core/mod.rs`       | Declares and re-exports the core domain modules and the `modes` submodule. |
| `src/core/cli.rs`       | Parses CLI arguments (`OsStr` / `OsString`), extracts global and mode-specific flags (`--config`, `--debug`, `--simulate`), and selects modes. |
| `src/core/config.rs`    | Defines the configuration model (`pods.conf`), collects active recipes, checks recipe membership, and expands `@group` targets. |
| `src/core/container.rs` | Defines the container model, resolves container names (`*name` and index fallbacks), validates container fields, and builds Podman commands. |
| `src/core/context.rs`   | Defines the runtime `Context` structure, mode and action enumerations, parsed flags, and positional parameters. |
| `src/core/log.rs`       | Provides structured output and diagnostic logging (`debug`, `info`, `pod_error`, `print`, `write_*`) with debug state control. |
| `src/core/pod.rs`       | Defines the pod model and orchestrates pod-level lifecycle operations (`up`, `down`, `create`, `remove`, `start`, `stop`, `restart`, `reload`, `update`) across containers. |
| `src/core/recipe.rs`    | Defines the `.pod` recipe model, validates pod and container definitions, and resolves target container specifications (`<recipe>/<spec>`). |

## CLI Modes (`src/core/modes/`)

The `src/core/modes/` directory contains the mode router and one subdirectory per CLI mode. Each mode directory isolates its dispatcher in `mod.rs` and places each CLI action in a dedicated file.

| File                    | Function |
| :---------------------- | :------- |
| `src/core/modes/mod.rs` | Re-exports the mode enumerations and dispatches execution to the active mode handler in O(1) time. |

### Command Mode (`src/core/modes/command/`)

Executes or lists predefined maintenance commands declared inside a recipe's `pod` block.

| File                             | Function |
| :------------------------------- | :------- |
| `src/core/modes/command/mod.rs`  | Parses `command` mode parameters, validates command definitions, and dispatches to `exec`, `list`, or `help`. |
| `src/core/modes/command/exec.rs` | Resolves the target container and executes the selected recipe command via `podman exec -it`. |
| `src/core/modes/command/help.rs` | Formats and outputs the CLI help text for `pods command`. |
| `src/core/modes/command/list.rs` | Collects, sorts, and displays all valid commands and descriptions defined in a target recipe. |

### Config Mode (`src/core/modes/config/`)

Inspects, validates, and edits the active `pods.conf` configuration file.

| File                            | Function |
| :------------------------------ | :------- |
| `src/core/modes/config/mod.rs`  | Parses `config` mode parameters and dispatches to `show` (default), `edit`, or `help`. |
| `src/core/modes/config/edit.rs` | Opens the active configuration file in the configured editor process. |
| `src/core/modes/config/help.rs` | Formats and outputs the CLI help text for `pods config`. |
| `src/core/modes/config/show.rs` | Displays configuration settings, directory status, group hierarchy trees, and missing or unreferenced recipes. |

### Connect Mode (`src/core/modes/connect/`)

Opens an interactive shell session inside a running container.

| File                              | Function |
| :-------------------------------- | :------- |
| `src/core/modes/connect/mod.rs`   | Parses `connect` mode parameters, rejects group targets, and dispatches to `shell` (default) or `help`. |
| `src/core/modes/connect/help.rs`  | Formats and outputs the CLI help text for `pods connect`. |
| `src/core/modes/connect/shell.rs` | Resolves the target container (by recipe, `<recipe>/<container>`, or absolute name) and spawns an interactive shell. |

### Default Mode (`src/core/modes/default/`)

Executes primary pod lifecycle operations and displays container runtime status when no explicit mode is given.

| File                                | Function |
| :---------------------------------- | :------- |
| `src/core/modes/default/mod.rs`     | Validates the requested default action, expands `@group` targets, and dispatches to the action handler. |
| `src/core/modes/default/create.rs`  | Loads and validates target recipes, then creates pods and containers in top-to-bottom declaration order without starting them. |
| `src/core/modes/default/down.rs`    | Stops target containers in reverse startup order and removes the containers and parent pod (`stop` + `remove`). |
| `src/core/modes/default/help.rs`    | Formats and outputs the global CLI help text for `pods`. |
| `src/core/modes/default/reload.rs`  | Tears down and rebuilds target pods and containers from their `.pod` recipes (`down` + `up`). |
| `src/core/modes/default/remove.rs`  | Removes target containers in reverse startup order and removes the parent pod without stopping them first. |
| `src/core/modes/default/restart.rs` | Stops and starts existing target pods and containers without removing or recreating them (`stop` + `start`). |
| `src/core/modes/default/start.rs`   | Starts existing stopped target pods and their containers in top-to-bottom declaration order. |
| `src/core/modes/default/status.rs`  | Queries `podman ps -a`, cross-references managed recipes, and formats the runtime status table. |
| `src/core/modes/default/stop.rs`    | Stops running target containers in reverse bottom-to-top order and stops the parent pod without removing them. |
| `src/core/modes/default/up.rs`      | Loads and validates target recipes, then creates and starts pods and containers in top-to-bottom order (`create` + `start`). |
| `src/core/modes/default/update.rs`  | Pulls the latest container images for target recipes and rebuilds the pods (`image update` + `reload`). |

### Image Mode (`src/core/modes/image/`)

Pulls recipe container images and prunes dangling or unused Podman container images.

| File                             | Function |
| :------------------------------- | :------- |
| `src/core/modes/image/mod.rs`    | Parses `image` mode parameters, expands `@group` targets for `update`, and dispatches to `update`, `prune`, or `help`. |
| `src/core/modes/image/help.rs`   | Formats and outputs the CLI help text for `pods image`. |
| `src/core/modes/image/prune.rs`  | Previews reclaimable image space (`--preview`) or prunes dangling and unused images (`--all`, `--force`). |
| `src/core/modes/image/update.rs` | Pulls the latest container images from configured registries for all containers in target recipes without reloading pods. |

### Init Mode (`src/core/modes/init/`)

Initializes a workspace with default configuration and recipe template files.

| File                            | Function |
| :------------------------------ | :------- |
| `src/core/modes/init/mod.rs`    | Parses `init` mode parameters and dispatches to template creation (default) or `help`. |
| `src/core/modes/init/create.rs` | Verifies that target files do not exist and writes the default `pods.conf` and `recipe.pod` templates to disk. |
| `src/core/modes/init/help.rs`   | Formats and outputs the CLI help text for `pods init`. |

### Logs Mode (`src/core/modes/logs/`)

Fetches or streams Podman logs for a recipe's pod or a specific container.

| File                            | Function |
| :------------------------------ | :------- |
| `src/core/modes/logs/mod.rs`    | Parses `logs` mode parameters, validates single-recipe targeting, and dispatches to `show`, `follow`, or `help`. |
| `src/core/modes/logs/follow.rs` | Streams live logs (`podman pod logs -f`) with optional `--tail`, `--since`, `--until`, and `--timestamps` flags. |
| `src/core/modes/logs/help.rs`   | Formats and outputs the CLI help text for `pods logs`. |
| `src/core/modes/logs/show.rs`   | Fetches and displays static logs (`podman pod logs`) for a pod or container and exits. |

### Recipe Mode (`src/core/modes/recipe/`)

Lists, inspects, and edits `.pod` recipe files.

| File                            | Function |
| :------------------------------ | :------- |
| `src/core/modes/recipe/mod.rs`  | Parses `recipe` mode parameters, validates recipe targets, and dispatches to `list`, `show`, `edit`, or `help`. |
| `src/core/modes/recipe/edit.rs` | Opens the specified `.pod` recipe file in the configured text editor. |
| `src/core/modes/recipe/help.rs` | Formats and outputs the CLI help text for `pods recipe`. |
| `src/core/modes/recipe/list.rs` | Lists configured recipes with descriptions and file status, plus unlinked `.pod` files when `--all` is set. |
| `src/core/modes/recipe/show.rs` | Reads a `.pod` recipe file and outputs its contents with line numbers. |

## I/O, Process Execution & PSL (`src/io/`)

The `src/io/` directory isolates all external process calls, filesystem operations, and `.pod` / `pods.conf` parsing.

| File            | Function |
| :-------------- | :------- |
| `src/io/mod.rs` | Declares and re-exports the `exec`, `psl`, and `system` submodules. |

### Process Execution (`src/io/exec/`)

Executes external commands directly via `std::process::Command` without shell interpolation.

| File                     | Function |
| :----------------------- | :------- |
| `src/io/exec/mod.rs`     | Declares and re-exports process execution types and helpers. |
| `src/io/exec/command.rs` | Executes external processes synchronously, interactively, or in simulate mode, and captures `stdout` / `stderr`. |
| `src/io/exec/podman.rs`  | Provides Podman-specific process queries, including version verification (`5.8.0+`) and container state checks. |

### PodScript Language (`src/io/psl/`)

Parses Podoxide configuration files (`pods.conf`) and declarative recipe files (`*.pod`) using a single-pass state machine.

| File                   | Function |
| :--------------------- | :------- |
| `src/io/psl/mod.rs`    | Declares PSL resource bound constants, re-exports AST types, and exposes the `parse_psl` entry point. |
| `src/io/psl/ast.rs`    | Defines the untyped intermediate AST structures (`PslBlock`, `PslDirective`, `PslDocument`, `PslStatement`). |
| `src/io/psl/parser.rs` | Scans and parses `.pod` and `pods.conf` input in a single O(N) pass into an untyped `PslDocument` AST. |

### Host System & Filesystem (`src/io/system/`)

Provides host operating system checks and filesystem operations.

| File                    | Function |
| :---------------------- | :------- |
| `src/io/system/mod.rs`  | Declares and re-exports the `env` and `fs` submodules. |
| `src/io/system/env.rs`  | Verifies that the host OS is Linux and detects elevated execution privileges (`UID 0`). |
| `src/io/system/fs.rs`   | Checks file and directory existence, resolves canonical paths, lists directories, and reads or writes files. |

## Utilities (`src/utilities/`)

The `src/utilities/` directory contains pure, allocation-bounded helper routines shared across the crate.

| File                          | Function |
| :---------------------------- | :------- |
| `src/utilities/mod.rs`        | Declares and re-exports the utility submodules. |
| `src/utilities/collection.rs` | Provides slice and vector helpers such as option token splitting (`append_option_tokens`), order-preserving deduplication (`dedup_preserve_order`), and bounded pre-allocation utilities (`bounded_capacity`). |
| `src/utilities/format.rs`     | Formats shell-escaped command strings (`format_command`), byte counts (`format_bytes`), parses human-readable sizes (`parse_size_to_bytes`), and aligns status columns (`format_line`). |
| `src/utilities/path.rs`       | Provides lexical path normalization (`normalize_lexical_path`), full path construction (`build_full_path`), parent and relative display path resolution (`parent_or_cur_dir`, `relative_display_path`), directory, editor, and target path validation (`has_regular_target_name`, `is_valid_directory`, `is_valid_directory_path`, `is_valid_editor_value`, `is_valid_target_path`), path token sanitization (`sanitize_path_token`), extension matching (`matches_extension`), parent traversal checks (`has_parent_dir`), and `.conf` stem/path resolution (`extract_config_stem`, `resolve_config_name`, `resolve_config_path`). |
| `src/utilities/string.rs`     | Provides UTF-8 boundary clamping (`clamp_utf8_prefix`), control/BiDi character detection and sanitization (`contains_control_or_bidi`, `contains_control_or_whitespace`, `sanitize_char`, `sanitize_text`), shell quoting (`escape_shell`), identifier, target, token, text, and pod/container name validation and normalization (`is_valid_group_target`, `is_valid_identifier`, `is_valid_pod_or_container_name`, `is_valid_recipe_identifier`, `is_valid_target_segment`, `is_valid_text`, `is_valid_token`, `normalize_name`), and strict ASCII integer parsing (`parse_ascii_u32`). |

## Execution Flow

1. **Entry (`src/main.rs`):** Invokes the library runner and maps the result to `std::process::ExitCode`.
2. **Environment Verification (`src/io/system/env.rs`, `src/io/exec/podman.rs`):** Verifies Linux OS, checks Podman `5.8.0+`, and prompts for confirmation when running as `root`.
3. **CLI Parsing (`src/core/cli.rs`):** Scans CLI arguments into `Context` (`src/core/context.rs`), extracting global flags (`--config`, `--debug`, `--simulate`), mode selection, action, and targets.
4. **Configuration Loading (`src/io/psl/`, `src/core/config.rs`):** Loads and parses `pods.conf` unless bypassed by standalone actions (`init`, `image prune`, `help`, or parameterless mode calls).
5. **Mode Dispatch (`src/core/modes/mod.rs`):** Routes `Context` to the selected mode handler (`default`, `command`, `config`, `connect`, `image`, `init`, `logs`, or `recipe`).
6. **Recipe Resolution & Execution (`src/core/recipe.rs`, `src/core/pod.rs`, `src/io/exec/`):** Expands `@group` targets, parses and validates `.pod` recipes via `src/io/psl/`, and executes or simulates the resulting `podman` commands.
