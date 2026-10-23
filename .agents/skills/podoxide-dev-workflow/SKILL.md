---
name: podoxide-dev-workflow
description: >-
  Build, test, and develop Podoxide. Covers the edit-verify cycle and testing standards.
---

# Podoxide Development Workflow

## 1. Edit-Build-Test Cycle
1. **Verify Baseline:** Run `cargo test` and `cargo clippy -- -D warnings` before editing files.
2. **Edit:** Modify source files under `src/` following the deterministic file layout, zero-dependency, and flat control flow rules (`P-VERTICAL-CLARITY`, `P-FLAT-CONTROL-FLOW`, `P-FLATTEN-DISPATCH`, `P-NO-NESTED-LOOPS`, `P-LOOP-MODULARITY`) in `AGENTS.md`. Ensure `crate::` precedes `self::` and `super::` in all import blocks (including `mod tests`), and enforce `MAX_*` bounds before scanning or allocating the $(N+1)$-th item.
3. **Flatness & Readability Gate:** Before running final verification (`cargo test` and `cargo clippy -- -D warnings`), scan all modified functions for:
   - Nested loops (`P-NO-NESTED-LOOPS`)
   - Indentation depth exceeding 3 levels (`P-FLAT-CONTROL-FLOW`)
   - Complex inline closures or large loop bodies (`P-VERTICAL-CLARITY`, `P-LOOP-MODULARITY`)
   If any function violates these rules, refactor the inner blocks into small, alphabetically sorted helper functions (following Section 2 of `AGENTS.md`) before completing the task.
4. **Test & Lint:** Run `cargo test` and `cargo clippy -- -D warnings` after every code modification.
5. **Sync Docs, Templates & Changelog:** Follow **`update-documentation`** to sync `USAGE.md`, `README.md`, `PSL_SPEC.md`, CLI help output, `.pods-completion.bash`, `pods.conf`, `recipe.pod`, `ARCHITECTURE.md`, and `AGENTS.md`. Follow **`changelog-updater`** to update `CHANGELOG.md`.

## 2. Testing Standards (`cargo test`)
* **Coverage:** Write unit and integration tests for every feature, parser routine, or bug fix.
* **Assertions:** Use semantic assertions (`.contains()`) over brittle exact-string matches.
* **Edge Cases:**
  * Cover fallback `_ =>` branches (`M-FALLBACK-TESTS`).
  * Cover extreme capacity bounds (`M-EXTREME-BOUND-TESTS`) and delimiter/quote injection in PSL files (`M-DELIM-INJECT-TESTS`).
  * Test path edge cases: hidden files, dot paths, multi-extensions, and empty paths (`M-PATH-EDGE-TESTS`).
  * Test I/O and process logic using temporary directories, in-memory cursors, or mock executors (`M-IO-TESTS`).
