---
name: podoxide-dev-workflow
description: >-
  Build, test, and develop Podoxide. Covers the edit-verify cycle and testing standards.
---

# Podoxide Development Workflow

## 1. Edit-Build-Test Cycle
1. **Verify Baseline:** Run `cargo test` and `cargo clippy -- -D warnings` before editing files.
2. **Edit:** Modify source files under `src/` following the deterministic file layout (`P-LICENSE-HEADER`), mandatory documentation (`P-FN-DOCS`), zero-dependency, direct-use (`P-NO-DEAD-CODE`), and flat control flow rules (`P-VERTICAL-CLARITY`, `P-FLAT-CONTROL-FLOW`, `P-FLATTEN-DISPATCH`, `P-NO-NESTED-LOOPS`, `P-LOOP-MODULARITY`) in `AGENTS.md`:
   - **File Layout Gate (`P-LICENSE-HEADER`):** Ensure step 1 of every `.rs` vertical file layout contains the verbatim GPL-3.0 license header from `src/main.rs` followed by a single blank line and `//!` module-level documentation.
   - **Implementation & Refactoring Gate (`P-FN-DOCS`):** Document every newly created or extracted production `fn`, method, constructor, `struct`, `enum`, or `type` (`pub`, `pub(crate)`, or private) with a concise `///` doc comment. Keep `#[test]` functions and test-only helpers inside `#[cfg(test)] mod tests` free of redundant `///` doc comments unless a comment is strictly required to explain a non-obvious edge case or attack vector.
   - Ensure `crate::` precedes `self::` and `super::` in all import blocks (including `mod tests`), and enforce `MAX_*` bounds before scanning or allocating the $(N+1)$-th item.
   - Do not create speculative helper methods, unused trait implementations, or fallback branches that have no active caller (`P-NO-DEAD-CODE`).
   - If downstream modules are not implemented yet, create only the minimal no-op placeholder stubs (`Ok(())` or `Default`) directly invoked by the current module so the crate compiles cleanly.
3. **Flatness & Readability Gate:** Before running final verification (`cargo test` and `cargo clippy -- -D warnings`), scan all modified functions for:
   - Nested loops (`P-NO-NESTED-LOOPS`)
   - Indentation depth exceeding 3 levels (`P-FLAT-CONTROL-FLOW`)
   - Complex inline closures or large loop bodies (`P-VERTICAL-CLARITY`, `P-LOOP-MODULARITY`)
   If any function violates these rules, refactor the inner blocks into small, alphabetically sorted helper functions (following Section 2 of `AGENTS.md`) with `///` doc comments (`P-FN-DOCS`) before completing the task.
4. **License, Documentation & Dead-Code Audit Gate (`P-LICENSE-HEADER`, `P-FN-DOCS`, `P-NO-DEAD-CODE`):** Before running final verification (`cargo test` and `cargo clippy -- -D warnings`):
   - Flag any `.rs` file missing the verbatim license header (`P-LICENSE-HEADER`) or `//!` module docs, or any production `fn`, `struct`, `enum`, or `type` missing a `///` doc comment (`P-FN-DOCS`), as a violation that must be fixed immediately.
   - Scan all modified and affected files for orphaned functions, unused struct fields, obsolete error variants, legacy code paths, underscore-prefixed unused items (`_unused_fn`), or `#[allow(dead_code)]` attributes, and remove every piece of code without a direct use (along with its obsolete unit tests).
5. **Test & Lint:** Run `cargo test` and `cargo clippy -- -D warnings` after every code modification.
6. **Sync Docs, Templates & Changelog:** Follow **`update-documentation`** to sync `USAGE.md`, `README.md`, `PSL_SPEC.md`, CLI help output, `.pods-completion.bash`, `pods.conf`, `recipe.pod`, `ARCHITECTURE.md`, and `AGENTS.md`. Follow **`changelog-updater`** to update `CHANGELOG.md`.

## 2. Testing Standards (`cargo test`)
* **Coverage:** Write unit and integration tests for every feature, parser routine, or bug fix.
* **Assertions:** Use semantic assertions (`.contains()`) over brittle exact-string matches.
* **Edge Cases:**
  * Cover fallback `_ =>` branches (`M-FALLBACK-TESTS`).
  * Cover extreme capacity bounds (`M-EXTREME-BOUND-TESTS`) and delimiter/quote injection in PSL files (`M-DELIM-INJECT-TESTS`).
  * Test path edge cases: hidden files, dot paths, multi-extensions, and empty paths (`M-PATH-EDGE-TESTS`).
  * Test I/O and process logic using temporary directories, in-memory cursors, or mock executors (`M-IO-TESTS`).
