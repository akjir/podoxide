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
├── .github/
│   └── workflows/
│       └── ci.yml
├── .gitignore
├── .pods-completion.bash
├── AGENTS.md
├── ARCHITECTURE.md
├── build.rs
├── build_metadata.rs
├── Cargo.toml
├── CHANGELOG.md
├── LICENSE
├── podoxide.code-workspace
├── pods.conf
├── PSL_SPEC.md
├── README.md
├── recipe.pod
├── rustfmt.toml
├── USAGE.md
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── error/
│   │   ├── cli.rs
│   │   ├── config.rs
│   │   ├── diagnostic.rs
│   │   ├── mod.rs
│   │   ├── psl.rs
│   │   ├── quote_scan.rs
│   │   ├── recipe.rs
│   │   ├── recipe_tests.rs
│   │   ├── span.rs
│   │   └── span_tests.rs
│   ├── core/
│   │   ├── constants.rs
│   │   ├── log.rs
│   │   ├── mod.rs
│   │   ├── cli/
│   │   │   ├── flags.rs
│   │   │   ├── flags_tests.rs
│   │   │   ├── mod.rs
│   │   │   ├── positionals.rs
│   │   │   └── tests.rs
│   │   ├── config/
│   │   │   ├── mod.rs
│   │   │   ├── parser.rs
│   │   │   ├── parser_tests.rs
│   │   │   ├── path.rs
│   │   │   ├── path_tests.rs
│   │   │   ├── targets.rs
│   │   │   └── tests.rs
│   │   ├── container/
│   │   │   ├── lifecycle.rs
│   │   │   ├── lifecycle_tests.rs
│   │   │   ├── mod.rs
│   │   │   ├── update.rs
│   │   │   ├── validation.rs
│   │   │   ├── validation_tests.rs
│   │   │   ├── volume.rs
│   │   │   └── volume_tests.rs
│   │   ├── context/
│   │   │   ├── actions.rs
│   │   │   ├── flags.rs
│   │   │   ├── mod.rs
│   │   │   └── tests.rs
│   │   ├── pod/
│   │   │   ├── lifecycle.rs
│   │   │   ├── lifecycle_tests.rs
│   │   │   ├── mod.rs
│   │   │   ├── update.rs
│   │   │   ├── update_tests.rs
│   │   │   └── validation_tests.rs
│   │   ├── recipe/
│   │   │   ├── directives.rs
│   │   │   ├── directives_tests.rs
│   │   │   ├── mod.rs
│   │   │   ├── parser.rs
│   │   │   ├── parser_tests.rs
│   │   │   ├── resolve.rs
│   │   │   ├── resolve_tests.rs
│   │   │   ├── tokenizer.rs
│   │   │   ├── validation.rs
│   │   │   └── validation_tests.rs
│   │   ├── update/
│   │   │   ├── mod.rs
│   │   │   ├── summary.rs
│   │   │   ├── tests.rs
│   │   │   └── writer.rs
│   │   └── modes/
│   │       ├── mod.rs
│   │       ├── targets.rs
│   │       ├── targets_tests.rs
│   │       ├── update_runner.rs
│   │       ├── update_runner_tests.rs
│   │       ├── command/
│   │       │   ├── exec.rs
│   │       │   ├── exec_tests.rs
│   │       │   ├── help.rs
│   │       │   ├── list.rs
│   │       │   └── mod.rs
│   │       ├── config/
│   │       │   ├── edit.rs
│   │       │   ├── help.rs
│   │       │   ├── inspect.rs
│   │       │   ├── inspect_tests.rs
│   │       │   ├── mod.rs
│   │       │   ├── render.rs
│   │       │   └── show.rs
│   │       ├── connect/
│   │       │   ├── help.rs
│   │       │   ├── mod.rs
│   │       │   ├── shell.rs
│   │       │   └── shell_tests.rs
│   │       ├── default/
│   │       │   ├── create.rs
│   │       │   ├── down.rs
│   │       │   ├── help.rs
│   │       │   ├── mod.rs
│   │       │   ├── reload.rs
│   │       │   ├── remove.rs
│   │       │   ├── restart.rs
│   │       │   ├── start.rs
│   │       │   ├── status/
│   │       │   │   ├── mod.rs
│   │       │   │   ├── table.rs
│   │       │   │   └── tests.rs
│   │       │   ├── stop.rs
│   │       │   ├── up.rs
│   │       │   ├── update.rs
│   │       │   └── version.rs
│   │       ├── image/
│   │       │   ├── help.rs
│   │       │   ├── mod.rs
│   │       │   ├── preview.rs
│   │       │   ├── prune.rs
│   │       │   ├── prune_tests.rs
│   │       │   └── update.rs
│   │       ├── init/
│   │       │   ├── create.rs
│   │       │   ├── help.rs
│   │       │   └── mod.rs
│   │       ├── logs/
│   │       │   ├── follow.rs
│   │       │   ├── help.rs
│   │       │   ├── mod.rs
│   │       │   ├── runner.rs
│   │       │   ├── runner_tests.rs
│   │       │   └── show.rs
│   │       └── recipe/
│   │           ├── edit.rs
│   │           ├── help.rs
│   │           ├── inspect.rs
│   │           ├── list.rs
│   │           ├── mod.rs
│   │           └── show.rs
│   ├── io/
│   │   ├── mod.rs
│   │   ├── exec/
│   │   │   ├── command.rs
│   │   │   ├── command_tests.rs
│   │   │   ├── mod.rs
│   │   │   ├── spinner.rs
│   │   │   ├── spinner_tests.rs
│   │   │   └── podman/
│   │   │       ├── lifecycle.rs
│   │   │       ├── lifecycle_tests.rs
│   │   │       ├── mod.rs
│   │   │       ├── query.rs
│   │   │       ├── query_tests.rs
│   │   │       ├── runtime.rs
│   │   │       └── runtime_tests.rs
│   │   ├── psl/
│   │   │   ├── ast.rs
│   │   │   ├── error_tests.rs
│   │   │   ├── mod.rs
│   │   │   ├── parser.rs
│   │   │   ├── scanner.rs
│   │   │   └── tests.rs
│   │   └── system/
│   │       ├── env.rs
│   │       ├── fs.rs
│   │       ├── fs_tests.rs
│   │       ├── mod.rs
│   │       └── path_resolve.rs
│   └── utilities/
│       ├── collection.rs
│       ├── collection_tests.rs
│       ├── format.rs
│       ├── mod.rs
│       ├── path.rs
│       ├── path_tests.rs
│       └── string.rs
└── tests/
    ├── build_script.rs
    ├── cli_matrix.rs
    ├── cli_runtime.rs
    ├── cli_workflows.rs
    ├── common/
    │   └── mod.rs
    ├── completion.rs
    ├── completion_edge.rs
    └── completion_security.rs
```

## Root Files and Templates

| File                        | Function |
| :-------------------------- | :------- |
| `.github/workflows/ci.yml`  | Runs formatting (`cargo fmt`), Clippy (`--all-targets -- -D warnings`), test suite, release build, and MSRV `1.88` check and test in Linux CI. |
| `.gitignore`                | Excludes build artifacts, local test workspaces (`.test/`), and editor/backup files from Git tracking. |
| `.pods-completion.bash`     | Provides pure-Bash autocompletion for modes, actions, action-scoped flags, `.conf` files, groups, recipes, containers, and recipe commands. |
| `Cargo.toml`                | Defines crate metadata, MSRV (`1.88`), workspace lints, library and binary targets, and release optimizations. |
| `build.rs`                  | Runs the Cargo build script delegating compile-time Git version and rerun directive emission to `build_metadata.rs`. |
| `build_metadata.rs`         | Implements pure Git version formatting, output validation, worktree path resolution, and Cargo rerun directive generation shared by `build.rs` and `tests/build_script.rs`. |
| `podoxide.code-workspace`   | Defines editor workspace formatting settings (4-space indentation, final newlines, trailing-whitespace trimming). |
| `pods.conf`                 | Provides the root configuration template embedded and written by `pods init`. |
| `PSL_SPEC.md`               | Specifies the PodScript Language (PSL) syntax, lexical grammar, block schemas, and parser resource bounds. |
| `recipe.pod`                | Provides the root recipe template embedded and written by `pods init`. |
| `rustfmt.toml`              | Configures repository `rustfmt` formatting rules (Edition 2024, 100-column width, preserving ASCII import order). |
| `src/main.rs`           | Serves as the binary entry point (`pods`), delegates execution to `podoxide`, and returns `ExitCode`. |
| `src/lib.rs`            | Serves as the library root, enforces `#![forbid(unsafe_code)]`, and exports `core`, `error`, `io`, and `utilities`. |
| `src/error/mod.rs`      | Declares and re-exports the central domain error submodules, `MAX_ERROR_TOKEN_BYTES`, `PodError`, and `SystemError`. |
| `src/error/cli.rs`      | Defines `CliError` and `CliErrorKind` for CLI parsing and validation failures. |
| `src/error/config.rs`   | Defines `ConfigError` and `ConfigErrorKind` for `pods.conf` semantic validation failures. |
| `src/error/diagnostic.rs` | Defines `DiagnosticError`, `build_caret_annotation`, and sanitized diagnostic and I/O error formatters. |
| `src/error/psl.rs`        | Defines `PslError` and `PslErrorKind` for PSL lexical, grammatical, and resource-limit failures. |
| `src/error/quote_scan.rs` | Provides single-pass quote-aware byte scanning for inline comments, escapes, and unquoted delimiters. |
| `src/error/recipe.rs`     | Defines `RecipeError` and `RecipeErrorKind` for `.pod` recipe validation and target resolution failures. |
| `src/error/span.rs`       | Resolves token and column spans for compiler-style diagnostics on quote-delimited source lines. |

## Core Domain (`src/core/`)

The `src/core/` directory contains application state, domain models, argument parsing, and top-level orchestration.

| File                                | Function |
| :---------------------------------- | :------- |
| `src/core/mod.rs`                   | Declares and re-exports the core domain modules and the `modes` submodule. |
| `src/core/cli/mod.rs`               | Declares `MAX_CLI_ARGS` and exposes `parse_args` to populate `Context` from raw `OsString` CLI arguments. |
| `src/core/cli/flags.rs`             | Implements single-pass CLI flag scanning, `--tail`/`--since`/`--until`/`--config` value validation, and action-aware flag validation. |
| `src/core/cli/positionals.rs`       | Resolves modes, sub-actions, and target parameters, rejecting mixed pod/group and `<recipe>/<container>` targets (`CliErrorKind::MixedTargets`). |
| `src/core/constants.rs`             | Defines global application metadata and the compile-time build version constant (`APP_NAME`, `APP_VERSION`). |
| `src/core/context/mod.rs`           | Defines the runtime `Context` structure (deriving `Mode` from `ModeAction`), config loading, and `requires_config`, `requires_podman`, `requires_elevation_confirmation`, and `supports_simulate` predicates. |
| `src/core/context/actions.rs`       | Defines `Mode`, `ModeAction`, and all eight typed mode sub-action enumerations with `Display`, `FromStr`, and `ModeAction::mode`. |
| `src/core/context/flags.rs`         | Defines bitmask-backed `Flags` (`P-MASK-SAFE`) with getters and setters for all seven boolean CLI flags plus value flag storage. |
| `src/core/log.rs`                   | Provides structured output, numbered-line printing (`write_numbered_lines`), and diagnostic logging (`debug`, `info`, `pod_error`, `write_*`) with debug state control. |
| `src/core/config/mod.rs`            | Defines `Config`, `PodsConfig`, `RecipesConfig`, defaults, and `MAX_GROUP_EXPANSION_DEPTH`, re-exporting path and editor helpers. |
| `src/core/config/parser.rs`         | Parses and validates `pods.conf` PSL AST blocks and directives (`general`, `pods`, `recipes`, `groups`). |
| `src/core/config/path.rs`           | Resolves `.conf` paths and names (`extract_config_stem`, `resolve_config_name`, `resolve_config_path`), reads config files, and validates `editor` values (`validate_editor`). |
| `src/core/config/targets.rs`        | Validates acyclic `@<group>` DAGs within `MAX_GROUP_EXPANSION_DEPTH`, queries recipe membership, and expands targets in a single deduplicated pass. |
| `src/core/container/mod.rs`         | Defines `Container` and `VolumeMount` and re-exports container lifecycle, update, validation, and volume APIs. |
| `src/core/container/lifecycle.rs`   | Executes container lifecycle commands (`create`, `start`, `stop`, `remove`, `up`, `down`, `restart`, `reload`), propagating `ExecOutcome`. |
| `src/core/container/update.rs`      | Resolves image references, executes cached `podman pull` (`PullCache`), detects stale container image IDs, and conditionally reloads containers. |
| `src/core/container/validation.rs`  | Validates container names, images, registries, restart policies, options, commands, and volume preconditions, and resolves relative container names. |
| `src/core/container/volume.rs`      | Validates and formats `VolumeMount` entries against pod base directories. |
| `src/core/pod/mod.rs`               | Defines `Pod` (with execution-invariant `PartialEq` / `Eq`) and `PodCommand`, and manages 3-state pod creation tracking (`PodCreationState`). |
| `src/core/pod/lifecycle.rs`         | Orchestrates pod-level lifecycle operations (`up`, `down`, `create`, `remove`, `start`, `stop`, `restart`, `reload`) with upfront validation and `ExecOutcome` propagation. |
| `src/core/pod/update.rs`            | Orchestrates pod-level image pulls (`pull_with_cache`), stale-container image ID detection, and conditional indented pod reload (`update_with_cache`). |
| `src/core/recipe/mod.rs`            | Defines `Recipe` and `RECIPE_EXTENSION`, exposing `Recipe::inspect`, `read_file`, `file_exists`, and container target resolution. |
| `src/core/recipe/directives.rs`     | Parses and validates individual `.pod` block headers and directives (`publish`, `volume`, `option`, `name`, `path`, `registry`, `image`) with per-block entry caps and argument-index diagnostic spans. |
| `src/core/recipe/parser.rs`         | Loads `.pod` files, validates load paths/targets, and translates PSL AST blocks into `Recipe`. |
| `src/core/recipe/resolve.rs`        | Resolves container specifications (`1..=N`, `*suffix`, exact or prefixed name) with zero-copy fast paths for already-resolved names. |
| `src/core/recipe/tokenizer.rs`      | Splits and validates `command.execute` strings into bounded quote-aware argv tokens. |
| `src/core/recipe/validation.rs`     | Performs semantic validation on `Recipe`, `Pod`, `PodCommand`, `publish` specifications, options, resolved container images, command targets, and container names. |
| `src/core/update/mod.rs`            | Defines update outcome models (`ContainerPullRecord`, `ContainerUpdateResult`, `PodUpdateResult`, `PullCache`, `SummaryKind`, `SummaryScope`, `UpdateAction`, `UpdateRunners`) and pull formatting helpers. |
| `src/core/update/summary.rs`        | Aggregates pod/container update results (`UpdateSummary`, `has_failures`) and renders grouped `Update Summary:` reports. |
| `src/core/update/writer.rs`         | Implements `IndentedWriter`, preserving indentation across both `\n` and `\r` line boundaries. |

## CLI Modes (`src/core/modes/`)

The `src/core/modes/` directory contains the mode router, shared target/update orchestration modules, and one subdirectory per CLI mode. Each mode directory isolates its dispatcher in `mod.rs` and places each CLI action in dedicated files.

| File                              | Function |
| :-------------------------------- | :------- |
| `src/core/modes/mod.rs`           | Re-exports the mode enumerations and dispatches typed `ModeAction` variants to mode handlers in O(1) time, returning `ExecOutcome`. |
| `src/core/modes/targets.rs`       | Provides shared `<recipe>/<container>` spec parsing, index-based container target resolution, upfront pod recipe pre-validation and lifecycle dispatch, `RecipeInspection` / `RecipeStatus`, and validated unreferenced `.pod` discovery. |
| `src/core/modes/update_runner.rs` | Provides shared pod and container image-update orchestration with pull deduplication and grouped `Update Summary:` reporting for `pods update` and `pods image update`. |

### Command Mode (`src/core/modes/command/`)

Executes or lists predefined maintenance commands declared inside a recipe's `pod` block.

| File                                  | Function |
| :------------------------------------ | :------- |
| `src/core/modes/command/mod.rs`       | Parses `command` mode parameters, validates command definitions in alignment with `Recipe::inspect`, and dispatches to `exec`, `list`, or `help`. |
| `src/core/modes/command/exec.rs`      | Resolves the target container and executes the selected recipe command via TTY-aware `podman exec` (`-it` on TTY, `-i` when piped). |
| `src/core/modes/command/help.rs`      | Formats and outputs the CLI help text for `pods command`. |
| `src/core/modes/command/list.rs`      | Collects, sorts, and displays all valid commands and descriptions defined in a target recipe. |

### Config Mode (`src/core/modes/config/`)

Inspects, validates, displays, and edits the active `pods.conf` configuration file.

| File                               | Function |
| :--------------------------------- | :------- |
| `src/core/modes/config/mod.rs`     | Parses `config` mode parameters and dispatches to `inspect` (default), `show`, `edit`, or `help`. |
| `src/core/modes/config/edit.rs`    | Opens the active configuration file in the configured editor process. |
| `src/core/modes/config/help.rs`    | Formats and outputs the CLI help text for `pods config`. |
| `src/core/modes/config/inspect.rs` | Traverses configured recipe groups, detects missing and cyclic subgroups (`[MISSING GROUP]`, `[CYCLIC GROUP]`), and evaluates recipe status with bounded output tracking. |
| `src/core/modes/config/render.rs`  | Renders settings, validated directory statuses (`[OK]`, `[NOT SET]`, `[NOT FOUND]`), aligned group trees (with truncation notice when bounded), unreferenced recipes, and the validation summary. |
| `src/core/modes/config/show.rs`    | Reads the active configuration file and outputs its contents with 1-based line numbers. |

### Connect Mode (`src/core/modes/connect/`)

Opens an interactive shell session inside a running container.

| File                              | Function |
| :-------------------------------- | :------- |
| `src/core/modes/connect/mod.rs`   | Parses `connect` mode parameters, rejects group targets, and dispatches to `shell` (default) or `help`. |
| `src/core/modes/connect/help.rs`  | Formats and outputs the CLI help text for `pods connect`. |
| `src/core/modes/connect/shell.rs` | Resolves registered recipes or direct unmanaged container names, distinguishes stopped from nonexistent containers, and spawns an interactive shell. |

### Default Mode (`src/core/modes/default/`)

Executes primary pod lifecycle operations and displays container runtime status when no explicit mode is given.

| File                                      | Function |
| :---------------------------------------- | :------- |
| `src/core/modes/default/mod.rs`           | Routes typed `DefaultAction` variants to the corresponding action handler. |
| `src/core/modes/default/create.rs`        | Loads and validates target recipes, then creates pods and containers (or targeted containers) without starting them. |
| `src/core/modes/default/down.rs`          | Stops target containers in reverse startup order and removes the containers and parent pod (or targeted containers) (`stop` + `remove`). |
| `src/core/modes/default/help.rs`          | Formats and outputs the global CLI help text for `pods`. |
| `src/core/modes/default/reload.rs`        | Tears down and rebuilds target pods and containers (or targeted containers) from their `.pod` recipes (`down` + `up`). |
| `src/core/modes/default/remove.rs`        | Removes target containers in reverse startup order and removes the parent pod (or targeted containers) without stopping them first. |
| `src/core/modes/default/restart.rs`       | Stops and starts existing target pods and containers (or targeted containers) without removing or recreating them (`stop` + `start`). |
| `src/core/modes/default/start.rs`         | Starts existing stopped target pods and their containers (or targeted containers) in top-to-bottom declaration order. |
| `src/core/modes/default/status/mod.rs`    | Queries `podman ps -a`, cross-references managed recipes or targeted containers across all host records, and distinguishes missing `.pod` files (`Not Found`) from invalid recipes (`Recipe Error`). |
| `src/core/modes/default/status/table.rs`  | Parses control-delimited `podman ps` rows (decoding JSON-quoted commands and rejecting empty IDs/names), computes column widths, sanitizes cells, and renders aligned status tables without trailing spaces. |
| `src/core/modes/default/stop.rs`          | Stops running target containers in reverse bottom-to-top order and stops the parent pod (or targeted containers) without removing them. |
| `src/core/modes/default/up.rs`            | Loads and validates target recipes, then creates and starts pods and containers (or targeted containers) in top-to-bottom order (`create` + `start`). |
| `src/core/modes/default/update.rs`        | Wires live Podman pull and image-ID inspectors into `update_runner` to pull images and conditionally reload updated pods or containers. |
| `src/core/modes/default/version.rs`       | Formats and outputs the application version string for `pods version`. |

### Image Mode (`src/core/modes/image/`)

Pulls recipe container images and prunes dangling or unused Podman container images.

| File                              | Function |
| :-------------------------------- | :------- |
| `src/core/modes/image/mod.rs`     | Parses `image` mode parameters and dispatches to `update`, `prune`, or `help`. |
| `src/core/modes/image/help.rs`    | Formats and outputs the CLI help text for `pods image`. |
| `src/core/modes/image/preview.rs` | Computes dynamic column widths and renders aligned `--preview` lists and reclaimable space totals. |
| `src/core/modes/image/prune.rs`   | Deduplicates multi-tagged prunable images by `long_id`, confirms interactive pruning on `stderr` (`AbortedExecution` on decline/EOF), and executes `podman image prune`. |
| `src/core/modes/image/update.rs`  | Delegates to `update_runner` to pull the latest container images without reloading pods and print the grouped `Update Summary:`. |

### Init Mode (`src/core/modes/init/`)

Initializes a workspace with default configuration and recipe template files.

| File                            | Function |
| :------------------------------ | :------- |
| `src/core/modes/init/mod.rs`    | Parses `init` mode parameters and dispatches to template creation (default) or `help`. |
| `src/core/modes/init/create.rs` | Verifies that target files do not exist and writes (or simulates writing with `--simulate`) the default `pods.conf` and `recipe.pod` templates to disk. |
| `src/core/modes/init/help.rs`   | Formats and outputs the CLI help text for `pods init`. |

### Logs Mode (`src/core/modes/logs/`)

Fetches or streams Podman logs for a recipe's pod or a specific container.

| File                            | Function |
| :------------------------------ | :------- |
| `src/core/modes/logs/mod.rs`    | Routes typed `LogsAction` variants to `show`, `follow`, or `help`. |
| `src/core/modes/logs/follow.rs` | Streams live logs (`podman pod logs -f`) with optional `--tail`, `--since`, `--until`, and `--timestamps` flags. |
| `src/core/modes/logs/help.rs`   | Formats and outputs the CLI help text for `pods logs`. |
| `src/core/modes/logs/runner.rs` | Validates target syntax and log flags before filesystem access, enforces recipe registration, resolves containers, and executes `podman pod logs`. |
| `src/core/modes/logs/show.rs`   | Fetches and displays static logs (`podman pod logs`) for a pod or container and exits. |

### Recipe Mode (`src/core/modes/recipe/`)

Lists, inspects, displays, and edits `.pod` recipe files.

| File                               | Function |
| :--------------------------------- | :------- |
| `src/core/modes/recipe/mod.rs`     | Parses `recipe` mode parameters, validates recipe targets, and dispatches to `list`, `inspect`, `show`, `edit`, or `help`. |
| `src/core/modes/recipe/edit.rs`    | Opens the specified `.pod` recipe file in the configured text editor. |
| `src/core/modes/recipe/help.rs`    | Formats and outputs the CLI help text for `pods recipe`. |
| `src/core/modes/recipe/inspect.rs` | Validates a single registered `.pod` recipe via `Recipe::inspect` and outputs confirmation or a diagnostic error. |
| `src/core/modes/recipe/list.rs`    | Lists configured recipes with descriptions and validation status (`[OK]`, `[ERROR]`, `[NOT FOUND]`), plus unlinked `.pod` files when `--all` is set. |
| `src/core/modes/recipe/show.rs`    | Reads a `.pod` recipe file and outputs its contents with 1-based line numbers. |

## I/O, Process Execution & PSL (`src/io/`)

The `src/io/` directory isolates all external process calls, filesystem operations, and `.pod` / `pods.conf` parsing.

| File            | Function |
| :-------------- | :------- |
| `src/io/mod.rs` | Declares and re-exports the `exec`, `psl`, and `system` submodules. |

### Process Execution (`src/io/exec/`)

Executes external commands directly via `std::process::Command` without shell interpolation.

| File                              | Function |
| :-------------------------------- | :------- |
| `src/io/exec/mod.rs`              | Declares the `command`, `podman`, and internal `spinner` execution submodules. |
| `src/io/exec/command.rs`          | Provides bounded process runners (`exec_capture`, `exec_with`, `ExecOutcome`, `InterruptPolicy`) and `exec_editor_with` for interactive file editing. |
| `src/io/exec/spinner.rs`          | Provides terminal-aware Braille spinner execution with bounded `stderr` capture and non-TTY single-line fallback. |
| `src/io/exec/podman/mod.rs`       | Re-exports the central Podman CLI API and provides shared validation and process invocation helpers. |
| `src/io/exec/podman/lifecycle.rs` | Builds and executes Podman lifecycle operations (`CreateContainer`, `CreatePod`, create, start, stop, remove, and image prune). |
| `src/io/exec/podman/query.rs`     | Executes Podman inspection queries (`check_version` 5.8.0+, `container_exists`, `container_running`, `pod_exists`, `query_container_image_id`, `query_image_id`, `query_images`, `query_in_use_images`, `query_ps_status`, and control-character record delimiters). |
| `src/io/exec/podman/runtime.rs`   | Builds and executes TTY-aware `podman exec`, color/SIGINT-aware `podman pod logs`, and spinner-backed `podman pull` (`ExecContainer`, `PodLogs`, `PullOutcome`, `SHELL_FALLBACK_COMMAND`). |

### PodScript Language (`src/io/psl/`)

Parses Podoxide configuration files (`pods.conf`) and declarative recipe files (`*.pod`) using a single-pass state machine.

| File                       | Function |
| :------------------------- | :------- |
| `src/io/psl/mod.rs`        | Declares PSL resource bound constants, re-exports AST types, and exposes the `parse_psl` entry point. |
| `src/io/psl/ast.rs`        | Defines the untyped intermediate AST structures (`PslBlock`, `PslDirective`, `PslDocument`, `PslStatement`). |
| `src/io/psl/error_tests.rs` | Tests PSL resource bounds, syntax errors, 1-based column accuracy, and compiler-style diagnostics. |
| `src/io/psl/parser.rs`     | Parses statement and block structure with pre-scan resource bounds into an untyped `PslDocument` AST. |
| `src/io/psl/scanner.rs`    | Provides single-pass O(N) lexical scanning, quote and escape handling, UTF-8 cursor stepping, and 1-based column tracking. |
| `src/io/psl/tests.rs`      | Tests canonical PSL documents, root templates, quoting modes, and assigned-quote edge cases. |

### Host System & Filesystem (`src/io/system/`)

Provides host operating system checks and filesystem operations.

| File                             | Function |
| :------------------------------- | :------- |
| `src/io/system/mod.rs`           | Declares the `env`, `fs`, and internal `path_resolve` submodules. |
| `src/io/system/env.rs`           | Verifies Linux OS via `std::env::consts::OS`, parses `/proc/self/status` for effective `UID 0`, and provides TTY-aware bounded confirmation prompts (`confirm_prompt`, `ConfirmationInput`). |
| `src/io/system/fs.rs`            | Performs `O_NONBLOCK` bounded file reads, `O_NOFOLLOW` writes, `O_CREAT \| O_EXCL` exclusive file creation with cleanup on partial write failure, regular-file directory listing, and source-preserving path I/O error wrapping. |
| `src/io/system/path_resolve.rs`  | Anchors relative paths to the working directory and resolves canonical paths with bounded symlink and resolved-byte expansion (`MAX_RESOLVED_PATH_BYTES`). |

## Utilities (`src/utilities/`)

The `src/utilities/` directory contains pure, allocation-bounded helper routines shared across the crate.

| File                          | Function |
| :---------------------------- | :------- |
| `src/utilities/mod.rs`        | Declares and re-exports the utility submodules. |
| `src/utilities/collection.rs` | Provides slice and vector helpers such as option token splitting and validation (`append_option_tokens`, `is_valid_option_value`) and bounded pre-allocation utilities (`bounded_capacity`). |
| `src/utilities/format.rs`     | Formats shell-escaped command strings (`format_command`), 1-based list indices (`format_index_prefix`), quoted conjunctions (`format_quoted_conjunction`), recipe inspection hints (`format_recipe_error_hint`), and byte counts (`format_bytes`), parses human-readable sizes (`parse_size_to_bytes`), aligns status columns (`format_line`), and formats CLI version headers (`format_version_header`). |
| `src/utilities/path.rs`       | Provides pure lexical path normalization (`normalize_lexical_path`, `pop_relative_or_absolute`), full path construction (`build_full_path`), parent and relative display path resolution (`parent_or_cur_dir`, `relative_display_path`), directory, editor, volume option, and target path validation (`has_regular_target_name`, `is_valid_directory`, `is_valid_directory_path`, `is_valid_editor_value`, `is_valid_target_path`, `is_valid_volume_options`), path token sanitization (`sanitize_path_token`), extension matching (`matches_extension`), and parent traversal checks (`has_parent_dir`) with zero `crate::core` imports. |
| `src/utilities/string.rs`     | Provides UTF-8 boundary clamping (`clamp_utf8_prefix`), control/BiDi character detection and sanitization (`contains_control_or_bidi`, `contains_control_or_whitespace`, `is_bidi_control`, `sanitize_char`, `sanitize_text`), empty value literal detection (`is_empty_value_literal`), shell quoting (`escape_shell`, `is_safe_shell_byte`), identifier, target, token, text, log flag, and pod/container name validation and normalization (`is_identifier_continue`, `is_identifier_start`, `is_valid_container_suffix`, `is_valid_group_target`, `is_valid_identifier`, `is_valid_log_tail`, `is_valid_log_time`, `is_valid_pod_or_container_name`, `is_valid_recipe_identifier`, `is_valid_target_segment`, `is_valid_text`, `is_valid_token`, `is_valid_unmanaged_container_name`, `normalize_name`), and strict ASCII integer parsing (`parse_ascii_u32`). |

## Integration Tests (`tests/`)

The `tests/` directory verifies the compiled `pods` binary in isolated RAII temporary workspaces backed by a deterministic mock `podman` script on `PATH`, without depending on a live host Podman installation.

| File                       | Function |
| :------------------------- | :------- |
| `tests/common/mod.rs`      | Provides `TestWorkspace` (RAII temp directory cleanup and executable mock `podman` shim) and `CliOutput` for subprocess execution. |
| `tests/build_script.rs`    | Verifies `build_metadata.rs` version formatting, Git output validation, non-Git isolation, unborn/worktree/packed-refs handling, and Cargo rerun directives. |
| `tests/cli_matrix.rs`      | Verifies missing-config bypass/error matrices, `simulate true`/`false` and `--simulate` override matrices, argument pipeline validation, and Podman `< 5.8.0` version enforcement. |
| `tests/cli_runtime.rs`     | Verifies live `status` table alignment and shared-name isolation, `connect` unmanaged uppercase container states, `image prune` in-use filtering and alias grouping, `logs --tail=all` translation, offline Podman-free bypasses, non-zero editor exit propagation, and stale-container `update` reloads. |
| `tests/cli_workflows.rs`   | Verifies installed-symlink CWD resolution, custom `--config` debug logging and broken-config display, non-zero Podman exit propagation without duplicate `ERROR:` lines, live shared-image `pods update` pull deduplication and reload, and live `pods image prune` preview/abort/confirm/failure flows. |
| `tests/completion.rs`      | Verifies `.pods-completion.bash` in isolated Bash subshells (`set -u`): zero shell variable leaks, split and glued `--config=` `.conf` filtering, `--flag=value` scanning, action-scoped flag filtering, unknown command rejection, and dynamic group/recipe/container/command completion. |
| `tests/completion_edge.rs`     | Verifies real Bash word-splitting (`=`, `@`, `:`), quoted and escaped `--config` paths, glued `--config=<dir>` slashes, CLI-rejected target/flag suppression and duplicate filtering, quoted PSL tokens/headers, and empty-item handling in `.pods-completion.bash`. |
| `tests/completion_security.rs` | Verifies `.pods-completion.bash` against `--config` command-injection payloads, special filenames (embedded newlines, brackets), `~/` expansion, `nocasematch` isolation, and `set -eu` compatibility. |

## Execution Flow

1. **Entry (`src/main.rs`):** Invokes the CLI pipeline and maps `Result<ExecOutcome, PodError>` to `std::process::ExitCode` without duplicating error output on `ExecOutcome::Failed`.
2. **CLI Parsing (`src/core/cli/`):** Scans CLI arguments into `Context` (`src/core/context/`), extracting global and action-specific flags, derived mode action, and validated targets.
3. **Environment Verification (`src/io/system/env.rs`, `src/io/exec/podman/`):** Verifies Linux OS, checks Podman `5.8.0+` when `context.requires_podman()`, and prompts for confirmation when running as `root` and `context.requires_elevation_confirmation()`.
4. **Configuration Loading (`src/io/psl/`, `src/core/config/`):** Loads and parses `pods.conf` unless bypassed by standalone actions (`init`, `version`, `help`, `config show`, or parameterless mode calls).
5. **Mode Dispatch (`src/core/modes/mod.rs`):** Routes `Context` and its typed sub-action enum to the selected mode handler (`default`, `command`, `config`, `connect`, `image`, `init`, `logs`, or `recipe`) and propagates `ExecOutcome`.
6. **Recipe Resolution & Execution (`src/core/recipe/`, `src/core/pod/`, `src/core/container/`, `src/io/exec/`):** Expands `@group` targets, parses and validates `.pod` recipes via `src/io/psl/`, and executes or simulates the resulting `podman` commands.
