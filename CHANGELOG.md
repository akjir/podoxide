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
