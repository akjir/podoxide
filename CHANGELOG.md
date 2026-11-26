# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.0.0] - 2026-11-26

### Added

- Add a mode-driven Linux CLI (`pods`) in pure standard Rust with 3-letter mode aliases, action-scoped flag validation, global configuration and simulation options, unified help output, and compile-time Git build version reporting.
- Add declarative PodScript Language (PSL) configuration and recipe parsing with quoted flag assignments, semantic validation, and compiler-style diagnostic messages with source context, span highlights, and help hints.
- Add host environment checks for Linux, Podman 5.8.0 or newer, and interactive root confirmation, bypassing Podman checks for offline actions and privilege prompts for read-only offline actions.
- Add deterministic pod and container lifecycle actions (`create`, `start`, `stop`, `remove`, `up`, `down`, `restart`, `reload`, and `update`) with nested `@group` expansion, automatic parent pod creation, individual `<recipe>/<container>` targeting, and `--simulate` dry-run previews.
- Add `status` reporting with aligned runtime tables for managed and unmanaged containers, `--full` target, command, and port columns, and `Not Found` or `Recipe Error` diagnostics.
- Add `init` mode to generate default rootless-friendly `pods.conf` and `recipe.pod` templates with `--simulate` support.
- Add `command` mode (`cmd`) to list and execute predefined recipe maintenance commands by name or index with TTY-aware interactive execution.
- Add `config` mode (`cfg`) to inspect settings, directory status, group hierarchies, and recipe validation summaries, display raw configuration source with line numbers, and edit configuration files.
- Add `connect` mode (`con`) to open interactive shell sessions in running managed or unmanaged containers.
- Add `image` mode (`img`) to pull deduplicated recipe images with an animated terminal progress spinner, detect stale running containers, print grouped update summaries, and preview or prune dangling and unused images.
- Add `logs` mode (`log`) to view or follow pod and container logs with `--since`, `--until`, `--tail` (including `all`), `--timestamps`, and `NO_COLOR`-aware color output.
- Add `recipe` mode (`rec`) to list configured and unreferenced recipes, validate individual recipes, display source with line numbers, and edit recipe files.
- Add dynamic Bash tab-completion for modes, actions, action-scoped flags, `.conf` paths, groups, recipes, container targets, and recipe maintenance commands.
- Add automated Linux CI checks and attested GitHub Release publishing with static binaries and SHA-256 checksums.

### Security

- Enforce direct process execution without shell interpolation and pure-Bash filename completion without `compgen` command-execution risk.
- Harden editor, port publishing, registry, image, option, identifier, command quoting, and host path validation against flag injection, option smuggling, and path traversal.
- Enforce bounded non-blocking file reads, bounded symlink resolution, symlink-safe exclusive template creation, pre-scan PSL resource limits, JSON-encoded container status parsing, and control/BiDi character sanitization across diagnostics, tables, and logs.
