---
name: changelog-updater
description: Updates and maintains CHANGELOG.md adhering to Keep a Changelog under [Unreleased].
---
# Changelog Updater
**Goal:** Maintain `CHANGELOG.md` per Keep a Changelog 1.1.0 & SemVer 2.0.0.

## USE WHEN
- Implementing, changing, or removing user-facing features/options (CLI modes, actions, flags, PSL configuration/recipe syntax, Podman lifecycle execution).
- Fixing user-facing bugs.

## RULES
- **Section:** ONLY under `## [Unreleased]`. Never commit/leave an empty `[Unreleased]` section.
- **Allowed Categories:** `### Added`, `### Changed`, `### Deprecated`, `### Removed`, `### Fixed`, `### Security`. Only include non-empty categories.
- **Scope (User-Facing ONLY):**
  - **INCLUDE:** Visible features, CLI modes/actions/flags, PSL (`pods.conf` / `*.pod`) directives and fields, output formatting changes, user bugfixes.
  - **BANNED:** Internal refactors, test additions/modifications (M-rules), internal build/verification scripts, internal agent directives/skills, toolchain adjustments without user impact.
- **Entry Format:**
  - 1-line concise bullet.
  - Action verb (e.g. `Added ...`, `Fixed ...`, `Updated ...`, `Removed ...`).
  - Strict English, zero emojis. Reference issue/PR if applicable (`(#123)`).

## WORKFLOW
1. Verify user-facing impact. Skip if internal/refactor/test.
2. Locate `## [Unreleased]` in `CHANGELOG.md`.
3. Add entry under matching category (`### Added`, `### Fixed`, etc.). Create category header if missing.
