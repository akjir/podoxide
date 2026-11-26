# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Add a mode-driven CLI with 3-letter mode aliases, global configuration and dry-run options, unified help output, and build version reporting.
- Add declarative PodScript Language (PSL) support for configuration and recipe files, including quoted flag assignments and semantic validation.
- Add host environment checks for Linux, Podman 5.8.0 or newer, and interactive confirmation when running as root.
- Add structured diagnostic logging with optional debug verbosity.
- Add compiler-style diagnostic messages with source context, span highlights, and help hints for configuration and recipe errors.
- Add pod and container lifecycle actions (`create`, `start`, `stop`, `remove`, `up`, `down`, `restart`, `reload`, and `update`) with group expansion, volume mounts, and `--simulate` dry-run output.
- Add individual container targeting across lifecycle actions, status, and image updates, including a target name column in full status output.
- Add `status` reporting to display aligned runtime tables for managed and unmanaged containers, including missing container and recipe error states.
- Add `init` mode to generate default configuration and recipe templates in the workspace.
- Add `command` mode (`cmd`) to list and run recipe maintenance commands by name or index.
- Add `config` mode (`cfg`) to inspect settings, group hierarchies, and recipe validation status, view configuration source, and edit configuration files.
- Add `connect` mode (`con`) to open interactive shell sessions in running managed or unmanaged containers.
- Add `image` mode (`img`) to pull recipe images and preview or prune dangling and unused images.
- Add `logs` mode (`log`) to view or stream pod and container logs with time-range, tail, and timestamp filters.
- Add `recipe` mode (`rec`) to list, inspect, view with line numbers, and edit recipe files.
- Add a grouped update summary with outcome categories and totals after pod and image updates.
- Add dynamic Bash tab-completion for configured groups, recipes, container targets, recipe maintenance commands, and `.conf` files, with action-scoped flag filtering and `=` flag value handling.

### Changed

- Start containers directly in recipe declaration order during pod startup actions instead of starting the pod as a whole.
- Check for prunable images before prompting during `image prune` and display the distinct image count and reclaimable space in the confirmation prompt on `stderr`.
- Report known, missing, invalid, and unreferenced recipes in configuration inspection summaries.
- Display raw configuration source with line numbers in `config show`, bypassing PSL validation so broken configuration files remain readable.
- Deduplicate identical image pulls per run, compare running container image IDs against pulled images to catch shared or pre-pulled updates, reload only changed targets, and display bounded error output when a pull fails.
- Replace verbose image pull output with a quiet animated progress spinner and clear per-image outcomes.
- Indent container pull and reload output under pod headers and separate consecutive targets with blank lines during updates.
- Enforce action-scoped CLI flag validation, rejecting unsupported flags per sub-action, `--all` with explicit status targets, `--config` in `init` mode, and combined `--force` and `--preview` flags.
- Bypass Podman version checks for offline actions (`help`, `version`, `init`, `config`, `recipe`, and `command list`), and bypass elevated-privilege prompts for read-only offline actions.
- Detect interactive terminals for `command exec` and `connect` (`-it` on TTY, `-i` when piped) and for colored `logs` output while respecting non-empty `NO_COLOR` values.
- Group multi-tag aliases per distinct image in `image prune --preview` and display a truncation warning with a lower-bound reclaimable total when more than 1024 unused images are present.
- Change the default recipe template pod path to a rootless-friendly relative path (`./pods/example-stack`).
- Suppress the `Simulate mode is active.` notice on read-only and live-query actions (`status`, `config inspect`, `recipe list`, `recipe inspect`, `recipe show`, `command list`, and `image prune --preview`).
- Display `-` and `[NOT SET]` in `config inspect` when `pods.path` is omitted in `pods.conf`, and avoid writing partial output when directory inspection fails.

### Removed

- Remove the `detach` directive from recipe container definitions.
- Remove the undocumented `-c` short flag from CLI parsing and shell completion.

### Fixed

- Continue lifecycle execution across remaining containers and pods with a warning while propagating a non-zero exit status when any Podman operation fails without duplicating error messages.
- Respect configured simulation mode settings during `image prune`, deduplicate multi-tagged images by full image ID, and exit with a non-zero status when confirmation is declined or receives EOF.
- Always query live container state in `status` even when simulation mode is active, and distinguish missing `.pod` files (`Not Found`) from invalid recipes (`Recipe Error`).
- Distinguish stopped from nonexistent unmanaged containers in `connect`, prioritize registered recipes over unreferenced files on disk, and accept uppercase names for unmanaged containers.
- Validate recipe `command` blocks (including target container resolution and `execute` tokenization) and resolved `<registry>/<image>` references consistently across recipe inspection, listing, and execution.
- Accept `all` (case-insensitive, forwarded as `-1` to Podman) for `logs --tail` and space-separated timestamps for `logs --since` and `--until`.
- Preserve quoted arguments containing spaces when executing recipe maintenance commands and in pod and container option directives.
- Prevent `init` dry-run simulation from writing template files to disk and document simulation support in `init` help and shell completion.
- Point cyclic group reference diagnostics to the offending group definition line instead of the enclosing block header.
- Create missing parent pods automatically before creating individually targeted containers.
- Prevent failed image pulls from reporting targets as up to date or triggering pod reloads, and print a reload notice when a container runs an older image than the cached tag.
- Propagate non-zero exit codes from `config edit` and `recipe edit` when the configured editor exits unsuccessfully.
- Keep containers with identical explicit names in separate pods distinct in `status` output and trim trailing spaces from status table lines.
- Reject mount options on anonymous volumes, descending port ranges in `publish`, and malformed empty components in `image` and `registry` values.
- Fix Bash completion for word-break splitting on `=`, `@`, and `:`, quoted, escaped, or `~/` `--config` paths, directory slashes on glued `--config=` paths, quoted recipe headers and group entries, empty candidates, duplicate targets, CLI-rejected flag or target combinations (including omitting `help` after `logs` filter flags), and `nocasematch` or `set -e` shell settings.
- Prevent shell variable leaks in Bash completion and drain bounded subprocess pipes cleanly to avoid broken-pipe stalls.
- Fix configuration root-block argument diagnostics, multi-argument `command` and `option` directive spans, CLI container typo line attribution, and diagnostic help hints for invalid recipes and PSL limits.
- Fix interactive confirmation prompt newlines on piped `stdin` or read errors, wrapped-line terminal spinner scrollback flooding, exit-code `130` warnings on `SIGINT`, simulated-vs-live pod creation state isolation, redundant stale checks after failed pulls, container reload failure summaries, status cell sanitization before trimming, and error/warning formatting consistency.

### Security

- Eliminate `<Tab>` command-execution risk in Bash `--config` filename completion by replacing `compgen -f` with NUL-delimited pure-Bash globbing and literal prefix matching.
- Harden editor, port publishing, registry, image, option (blocking quoted option smuggling), pod and container identifier, shell command quoting, and host path validation against flag injection, path traversal, and malformed specifications.
- Enforce non-blocking bounded file reads, bounded symlink path resolution, symlink protection and partial-write cleanup on exclusive file creation, pre-scan PSL parser resource limits, JSON-encoded `podman ps` command parsing against record-delimiter spoofing, and control/BiDi/Unicode-line-separator sanitization across PSL comments, diagnostics, tables, and logs.
