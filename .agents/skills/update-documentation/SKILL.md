---
name: update-documentation
description: >-
  Maintain and update README.md, USAGE.md, PSL_SPEC.md, .pods-completion.bash, and root .pod / .conf templates. Activate whenever editing documentation or when CLI modes, aliases, actions, options, targeting rules, config/recipe schemas, or system requirements change.
---

# Updating Documentation

## 1. Strict Separation of Concerns (Single Source of Truth)
* **`README.md` (Project Entry Point):** Keep under 130 lines. Do NOT copy detailed CLI flag tables, sub-action matrices, target resolution rules, or PSL grammar/file templates into `README.md`. Update `README.md` ONLY when top-level CLI modes, 3-letter aliases, lifecycle action names, system requirements, installation steps, or repository document links change.
* **`USAGE.md` (CLI & Schema Reference):** Single Source of Truth for all CLI modes, 3-letter aliases (`cmd`, `cfg`, `con`, `img`, `log`, `rec`), lifecycle actions (`up`, `down`, `create`, `remove`, `start`, `stop`, `restart`, `reload`, `update`, `status`), mode sub-actions, flags, targeting rules, and `pods.conf` / `*.pod` field schemas.
* **`PSL_SPEC.md` (Language Specification):** Single Source of Truth for the PodScript Language (`PSL`) lexical rules, root-block grammar, parser bounds, and canonical syntax used in `pods.conf` and `*.pod`.
* **`.pods-completion.bash` & Templates:** Update `.pods-completion.bash` whenever CLI modes, aliases, actions, or flags change. Keep root `pods.conf`, `recipe.pod`, and CLI `help` output synchronized with `USAGE.md` and `PSL_SPEC.md`.

## 2. `README.md` Structure (<130 Lines)
Omit a Table of Contents. Maintain this exact section order:
1. **`# Podoxide`:** 1-sentence factual summary of `pods` and `PSL`.
2. **`## Overview`:** Short intro + bullet list of core architectural traits (zero external dependencies, direct process execution, deterministic lifecycle, unified PSL scanner, `@group` resolution, `--simulate`).
3. **`## Requirements`:** 2-column table (`Component | Requirement`).
4. **`## Installation`:** Copy-paste ready `git clone`, `cargo build --release`, `install`, and `.pods-completion.bash` commands.
5. **`## Quick Start`:** Self-contained numbered commands (`pods init`, `pods config show`, `pods --simulate up example-stack`, `pods up example-stack`, `pods status`, `pods down example-stack`).
6. **`## CLI Overview & Documentation`:** General mode-driven syntax block, 3-column summary table (`Mode / Scope | Alias | Primary Purpose`), and links to `[USAGE.md](USAGE.md)`, `[PSL_SPEC.md](PSL_SPEC.md)`, `[ARCHITECTURE.md](ARCHITECTURE.md)`, and `[CHANGELOG.md](CHANGELOG.md)`.
7. **`## Development`:** Self-contained `cargo build`, `cargo test`, and `cargo clippy -- -D warnings` commands.
8. **`## Disclaimer & License`:** 2-sentence warranty disclaimer and GPL-3.0 link to `[LICENSE](LICENSE)`.

## 3. `USAGE.md` Structure (Complete Technical Reference)
Maintain this exact section order:
1. **`# Podoxide Usage Guide`** + **`## Table of Contents`**.
2. **`## Command Syntax`:** `pods` syntax, parsing rules, `### Modes Overview`, and `### Global Options`.
3. **`## Configuration Reference`:** Schema table and `pods.conf` example.
4. **`## Recipe Reference`:** Root blocks, pod/container field schema tables, port/volume/naming rules, and `*.pod` example.
5. **`## Targeting Rules`:** `### Recipe and Group Targets` and `### Container Targets`.
6. **Mode Sections (`## <Name> Mode`):** Order modes identically to `Modes Overview`. Each mode section contains intro/syntax, `### Actions`, `### Options`, and `### Usage`.

## 4. Style, Tone & Formatting Enforcement
* **Language & Tone:** Write all documentation in concise, factual English following ASD-STE100 (80%). Never use emojis, marketing adjectives ("powerful", "seamless", "intuitive"), greetings, concluding summary paragraphs, or redundant explanations.
* **No Horizontal Rules:** Never use `---` section dividers in Markdown body content; rely strictly on `##`, `###`, and `####` headings.
* **Tables & Callouts:** Format tables cleanly with `:---` alignment markers and escape pipes (`\|`) inside cells. Use GitHub alerts (`> [!NOTE]`, `> [!IMPORTANT]`) instead of bold blockquotes.
* **Code & Backticks:** Wrap all CLI modes, aliases, actions, flags, file names, directives, and target patterns in backticks. Ensure all shell snippets are directly copy-paste executable.
* **Generic Examples:** Use generic placeholders (`example-app`, `db-server`, `cache-server`, `example-stack`); never use real third-party software names.
