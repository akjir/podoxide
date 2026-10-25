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
- Added `image` mode (`pods image` / `pods img`) with `prune` (`--preview`, `--all`, `--force`), `update`, and `help` actions and human-readable byte size formatting and parsing helpers.
- Added `logs` mode (`pods logs` / `pods log`) with `show`, `follow`, and `help` actions and `--since`, `--until`, `--tail`, and `--timestamps` flags for fetching or streaming pod and container logs.
- Added `recipe` mode (`pods recipe` / `pods rec`) with `list` (`--all`), `show`, `edit`, and `help` actions for listing configured and unlinked `.pod` recipes, viewing numbered recipe source lines, and opening recipes in the configured editor.
- Added the `pods status` default mode action (`--all`, `--full`) for querying `podman ps -a`, cross-referencing managed recipes, synthesizing `Not Found` container rows, and rendering aligned runtime status tables.
- Added `<recipe>/<container>` container targeting (`<recipe>/<index>`, `<recipe>/*<suffix>`, `<recipe>/<short_name>`, and `<recipe>/<full_name>`) to all default mode actions (`create`, `down`, `reload`, `remove`, `restart`, `start`, `status`, `stop`, `up`, and `update`) and `pods image update`, plus the `TARGET NAMES` column (`<recipe>/<index>`) after `NAMES` in `pods status --full`.
- Added `pods config inspect` (default `config` action) and `pods recipe inspect <recipe>` for validating `.pod` recipes via `Recipe::inspect`, reporting `[ERROR]` badges in `pods config inspect` and `pods recipe list`, and synthesizing `Recipe Error` rows in `pods status`.
- Added support for bare-word `=` assigned quotes (`--flag='...'` and `--flag="..."`) in PSL directives and inner-quote validation (`is_valid_option_value`) for `option` directives in `pod` and `container` blocks.

### Changed

- Changed pod startup (`start`, `up`, `restart`, `reload`, and `update`) to start containers directly in deterministic `1..N` declaration order without executing `podman pod start`.
- Changed `pods image prune` to check for prunable images before prompting, output `No images found to prune.` when none exist, and display the prunable image count and reclaimable disk space in the confirmation prompt.
- Changed `pods config inspect` to report known and unreferenced recipe counts in `Directories:`, list unreferenced `.pod` recipe stems under `Unreferenced Recipes:`, and summarize missing, invalid, and unreferenced recipes in `Validation Summary:`.
- Changed `pods config show` to print the raw `pods.conf` file with 1-based line numbers matching `pods recipe show <recipe>`.

### Fixed

- Fixed pod and container lifecycle commands aborting on the first non-zero `podman` exit status by logging a warning and continuing execution across all remaining containers and the pod.
- Fixed `pods image prune` bypassing `pods.conf` loading so `general.simulate` (`true` or `false`) and `Simulate mode is active.` logging are respected.
- Fixed `pods command exec` splitting `execute` directives on unquoted whitespace so single-quoted and double-quoted arguments containing spaces are preserved.
- Fixed `pods init --simulate` writing template files to disk and added `--simulate` to `pods init help`, `USAGE.md`, and `.pods-completion.bash`.
- Fixed cyclic `@<group>` reference diagnostics in `pods.conf` pointing to the `groups {` block header instead of the offending group directive line.
- Fixed `option` directive token splitting in `pod` and `container` blocks breaking `--flag='value with spaces'` and `--flag 'value with spaces'` into separate `podman` arguments.

### Removed

- Removed the `detach` directive from `container` blocks in `.pod` recipes.
- Removed the undocumented `-c` short flag from CLI argument parsing and `.pods-completion.bash`.
