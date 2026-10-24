# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Added initial Rust crate layout, core I/O and process modules, and documentation templates.
- Added single-pass O(N) PSL scanner, parser, intermediate AST, and domain error types.
- Added CLI argument scanner, flag validator, runtime context, and O(1) mode dispatcher with 3-letter mode aliases.
- Added host environment verification for Linux OS, Podman 5.8.0+, and interactive root privilege confirmation.
- Added structured diagnostic logging (`DEBUG`, `INFO`, `WARNING`, `ERROR`), standard output helper, and `--debug` state control.
- Added PSL configuration loading, semantic validation, and host filesystem I/O helpers.
- Added `init` mode (`pods init` and `pods init help`) for generating default `pods.conf` and `recipe.pod` template files with workspace directory logging.
- Added default mode help output (`pods help` and parameterless `pods`).
- Added PSL recipe loading, pod and container domain models, semantic validation, and container target resolution.
- Added Podman pod and container lifecycle execution (`create`, `start`, `stop`, `remove`, `up`, `down`, `restart`, `reload`, `pull`, and `update`), volume mount formatting, and shell-escaped `--simulate` command rendering.
- Added configuration target expansion (`@group` and `<recipe>`) with first-appearance deduplication and wired CLI lifecycle handlers (`create`, `down`, `reload`, `remove`, `restart`, `start`, `stop`, `up`, `update`, and `image update`).
- Added Rust-compiler-style (`rustc`) diagnostic error output (`--> file:line:col`, line gutter, caret span annotations, and `= help:` hints) for PSL syntax, configuration (`pods.conf`), and recipe (`*.pod`) errors.
- Added `command` mode (`pods command` / `pods cmd`) with `exec`, `list`, and `help` actions for listing and executing maintenance commands defined in `.pod` recipes by name or 1-based numeric index.
- Added `config` mode (`pods config` / `pods cfg`) with `show`, `edit`, and `help` actions for inspecting configuration settings, directory status, group hierarchy trees, and recipe validation summaries, and editing `pods.conf`.
- Added `connect` mode (`pods connect` / `pods con`) with `shell` and `help` actions for opening interactive shell sessions (`bash` with `sh` fallback) inside running managed or unmanaged containers.
- Added compile-time SemVer 2.0.0 Git build metadata (`v<version>+<count>.<hash>[.dev]`), the `pods version` default mode action, and unified version headers across CLI help outputs.

### Changed

- Changed pod startup (`start`, `up`, `restart`, `reload`, and `update`) to start containers directly in deterministic `1..N` declaration order without executing `podman pod start`.

### Removed

- Removed the `detach` directive from `container` blocks in `.pod` recipes.
