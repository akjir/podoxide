---
name: podoxide-dev-workflow
description: >-
  Build, test, and develop Podoxide. Covers the edit-verify cycle and testing standards.
---

# Podoxide Development Workflow

## 1. Edit-Build-Test Cycle
1. **Verify Baseline:** Run `cargo test` and `cargo clippy -- -D warnings` before editing files.
2. **Edit (`AGENTS.md` Sections 1–3):** Keep `mod.rs` files minimal (declarations, re-exports, constants, thin dispatchers) and place core logic in leaf `.rs` files:
   - **Layout & Imports (`P-LICENSE-HEADER`):** Follow the 8-part vertical layout (verbatim GPL-3.0 header from `src/main.rs` + blank line + `//!` docs). Enforce `std::` then `crate::`, `self::`, `super::` import order (including in `mod tests`), single-module braces (`imports_granularity = Module`, no nested `::`), no production globs (`use ...::*;`), and direct type imports vs. module-qualified free functions.
   - **Types & Docs (`P-FN-DOCS`, `P-NO-DEAD-CODE`):** Favor concrete types, named-field structs, and two-state enums over ambiguous `bool` parameters. Document every production item (`pub`, `pub(crate)`, private, and extracted helpers) with `///`; omit redundant `///` in `#[cfg(test)] mod tests`. Write only directly called items (or active `Ok(())` / `Default` compilation stubs).
3. **Flatness, Readability & Allocation Gate (`P-VERTICAL-CLARITY`, `P-FLAT-CONTROL-FLOW`, `P-FLATTEN-DISPATCH`, `P-NO-NESTED-LOOPS`, `P-LOOP-MODULARITY`, `P-FUNCTIONAL-COMBINATORS`, `P-ZERO-ALLOC`, `P-ZERO-COPY`, `P-BOUND-ALLOC`):** Refactor any violating block into small, alphabetically sorted, `///`-documented helper functions:
   - Enforce zero nested loops (`P-NO-NESTED-LOOPS`), indentation $\le 3$ levels (`P-FLAT-CONTROL-FLOW`), loop bodies $< 25$ lines (`P-LOOP-MODULARITY`), and dispatchers $< 40$ lines (`P-FLATTEN-DISPATCH`).
   - Deconstruct `Option`/`Result` via `match`, `if let`, `?`, or `let ... else` (never check `.is_some()`/`.is_ok()` then unwrap). Prefer flat `for` loops calling a helper over complex combinator chains (`P-FUNCTIONAL-COMBINATORS`, `P-VERTICAL-CLARITY`).
   - Eliminate avoidable loop allocations (`.to_string()`, `.to_owned()`, `.clone()`, `PathBuf::from()`, intermediate `.collect::<Vec<_>>()`), and check `MAX_*` bounds before scanning or allocating the $(N+1)$-th element (`P-BOUND-ALLOC`).
4. **License, Documentation & Dead-Code Gate (`P-LICENSE-HEADER`, `P-FN-DOCS`, `P-NO-DEAD-CODE`):** Verify license headers, `//!` and `///` docs, and delete all orphaned functions, fields, variants, imports, `_unused` items, `#[allow(dead_code)]`, and obsolete unit tests.
5. **Test & Lint:** Run `cargo test` and `cargo clippy -- -D warnings` after every modification.
6. **Sync Docs, Templates & Changelog:** Follow **`update-documentation`** (`USAGE.md`, `README.md`, `PSL_SPEC.md`, CLI help, `.pods-completion.bash`, `pods.conf`, `recipe.pod`, `ARCHITECTURE.md`, `AGENTS.md`) and **`changelog-updater`** (`CHANGELOG.md`).

## 2. Testing Standards (`cargo test`)
- **Coverage & Assertions:** Write unit and integration tests for every feature, parser routine, or bug fix. Use semantic assertions (`.contains()`) over exact-string matches, and include concise context strings in `assert!` / `assert_eq!` for loops, table-driven cases, and state transitions.
- **Required Edge Cases (`AGENTS.md` Section 5):** Cover fallback `_ =>` arms (`M-FALLBACK-TESTS`), `MAX_*` bounds (`M-EXTREME-BOUND-TESTS`), PSL delimiter/quote injection (`M-DELIM-INJECT-TESTS`), path edge cases (`.` / `..` / hidden / multi-extension / empty; `M-PATH-EDGE-TESTS`), and isolated I/O/process tests via temp dirs, cursors, or mock executors (`M-IO-TESTS`).
