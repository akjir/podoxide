---
name: changelog-updater
description: Updates and maintains CHANGELOG.md adhering to Keep a Changelog under [Unreleased].
---

# Changelog Updater

Maintain `CHANGELOG.md` per Keep a Changelog 1.1.0 and SemVer 2.0.0.

## 1. Use When
- Adding, changing, deprecating, or removing notable user-facing features or options (CLI modes, aliases, actions, flags, PSL configuration/recipe syntax, Podman lifecycle behavior, or output formatting).
- Fixing user-facing bugs or applying notable security or stability hardening.
- Preparing release notes.

## 2. Rules
- **Section:** Write entries ONLY under `## [Unreleased]` (create `## [Unreleased]` above the latest release heading if absent). Never commit or leave an empty `## [Unreleased]` section.
- **Categories:** Use `### Added`, `### Changed`, `### Deprecated`, `### Removed`, `### Fixed`, and `### Security`. Include only non-empty categories.
- **Scope:**
  - **Include:** Notable user-facing, security, or release changes (CLI modes, actions, flags, PSL directives and fields, observable behavior or output changes, user bugfixes, and user-impacting hardening).
  - **Omit:** Purely internal refactors, test-only changes, internal build or verification scripts, agent rules or skills, and toolchain adjustments without user impact.
- **Entry Format:**
  - Write one concise, high-level sentence per entry in the imperative mood (ASD-STE100), starting with a verb (for example `Add`, `Change`, `Fix`, `Harden`, `Improve`, `Prevent`, `Remove`, `Show`, `Support`).
  - Focus on observable outcomes and general impact rather than how the code works.
  - Omit internal code details: no struct, enum, trait, function, method, or variable names, no source or config file paths, and no algorithmic, regex, or policy/linter-rule minutiae.
  - Group related internal hardening, refactoring, or compliance fixes into a single summary bullet instead of listing each code change separately.
  - Use concise English with zero emojis, end each entry with a period, and reference an issue or PR (`(#123)`) when applicable.

## 3. Workflow
1. Verify notable user-facing, security, or release impact. Skip if purely internal or test-only.
2. Locate `## [Unreleased]` in `CHANGELOG.md` (or create it above the latest release heading).
3. Add or group the entry under the matching category heading, creating the category heading if missing.
4. Do not commit changes unless explicitly instructed.
