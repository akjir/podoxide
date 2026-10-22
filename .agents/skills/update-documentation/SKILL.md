---
name: update-documentation
description: >-
  Maintain and update README.md, USAGE.md, .pods-completion.bash, and root .pod templates. Activate whenever editing documentation or when CLI modes, actions, options, targeting rules, config/recipe schemas, or system requirements change.
---

# Updating Documentation

## 1. Triggers & Sync Checklist
Update documentation whenever you change:
* **CLI Syntax:** Modes, actions, options, default actions, or target resolution. Sync: `USAGE.md`, `README.md`, CLI help output, and `.pods-completion.bash`.
* **Schemas & Behavior:** `pods.conf` or `recipe.pod` fields, defaults, volume/port/naming rules, or config-bypass rules. Sync: `USAGE.md`, `README.md` (if top-level), and root `pods.conf` / `recipe.pod`.
* **Requirements:** OS, Rust edition, or Podman minimum versions. Sync: `README.md` and `AGENTS.md`.

## 2. `README.md` Rules (Onboarding & Overview)
Keep `README.md` concise as the project entry point. Omit a Table of Contents. Maintain this section order:
1. **`# Podoxide`:** Badges (`Platform`, `Rust`, `Podman`, `License`) + 2-sentence summary (declarative `.pod` recipes to deterministic Podman CLI commands in pure standard Rust).
2. **`## Features`:** Bullet list of core capabilities.
3. **`## Requirements`:** 2-column table (`Component | Minimum Version`).
4. **`## Installation`:** Build/install instructions and `### Shell Autocompletion (Optional)`.
5. **`## Quick Start`:** Numbered flow (`pods init`, `pods up recipe --simulate`, lifecycle actions, `pods down recipe`).
6. **`## Configuration and Recipes`:** `### Configuration (pods.conf)` and `### Recipes (<name>.pod)` examples.
7. **`## CLI Overview`:** General syntax block, 4-column summary table (`Mode | Actions | Target | Description`), global options list, and link to `[USAGE.md](USAGE.md)`.
8. **`## Development`:** Single `bash` block with `cargo test`, `cargo clippy -- -D warnings`, and `cargo build --release`.
9. **`## License`:** Link to `[LICENSE](LICENSE)` + 1-line warranty disclaimer.

## 3. `USAGE.md` Rules (Complete Technical Reference)
Keep `USAGE.md` as the exhaustive technical manual. Maintain this section order:
1. **`# Podoxide Usage Guide`** + **`## Table of Contents`**.
2. **`## Command Syntax`:** `pods` syntax, parsing rules, `### Modes Overview`, and `### Global Options`.
3. **`## Configuration Reference`:** Schema table and `pods.conf` reference.
4. **`## Recipe Reference`:** Top-level, pod, and container field schema tables, port/volume/naming rules, and `recipe.pod` reference.
5. **`## Targeting Rules`:** `### Recipe and Group Targets` and `### Container Targets`.
6. **Mode Sections (`## <Name> Mode`):** Order modes identically to `Modes Overview`. Each mode section contains intro/syntax, `### Actions`, `### Options`, and `### Usage`.

## 4. Formatting & Content Standards
* **Tables:** Align all table columns with spaces and `:---` markers. Escape pipes inside cells (`\|`).
* **No Horizontal Rules:** Never use `---` section dividers; rely on `##`, `###`, and `####` headings.
* **Callouts:** Use GitHub alerts (`> [!NOTE]`, `> [!IMPORTANT]`); never use bold blockquotes.
* **Code & Backticks:** Wrap all CLI modes, actions, flags, file names, field keys, and target patterns in backticks.
* **Generic Examples:** Use generic placeholders (`example-app`, `db-server`, `cache-server`, `example-stack`, `web-stack`); never use real software names.
