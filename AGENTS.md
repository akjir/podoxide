# Podoxide AI Agent Rules

> [!IMPORTANT]
> Update this file, [`ARCHITECTURE.md`](ARCHITECTURE.md), and [`PSL_SPEC.md`](PSL_SPEC.md) when the architecture, module structure, file names, or PSL syntax change.

## 1. Core Constraints & Workflow
- **Goal:** Manage Podman pods and containers with a deterministic CLI tool (`pods`) in pure standard Rust using declarative configuration (`pods.conf`) and recipes (`*.pod`) written in the custom block-based PodScript Language (`PSL`).
- **Zero Dependencies:** Use `std` only (`P-STDLIB-ONLY`). Keep `[dependencies]` and `[dev-dependencies]` in `Cargo.toml` completely empty.
- **Target & Execution:** Support Linux only (Rust Edition 2024, Podman 5.8.0+). Execute processes directly via `std::process::Command` without shell interpolation (`sh -c`).
- **Safety:** Enforce `#![forbid(unsafe_code)]` in crate roots. Avoid global mutable state (`static mut`). Never use `.unwrap()` or `.expect()` in production code (allowed only for proven mathematical invariants).
- **Version Control:** NEVER commit changes unless the user explicitly instructs you to do so.
- **Language & Tone:** Write all responses, code, comments, documentation, tests, and commits in English. Follow ASD-STE100 (80%). Be concise and precise. Do not use emojis or horizontal rules (`---`) in Markdown files.
- **Simplicity & Vertical Clarity (`P-VERTICAL-CLARITY`):** Prioritize human readability, clear structure, and flat control flow over compact code density. Writing more lines of code and extracting small, well-named helper functions is ALWAYS preferred over deep nesting, dense expressions, or complex inline closures. Smart and efficient algorithms are encouraged, provided their implementation is broken down into flat, single-purpose functions.
- **Token Efficiency:** Avoid redundant information across code, docs, rules, and skills. When restructuring code without logic changes, preserve all business logic, signatures, error types, and test semantics.
- **Direct Use & Zero Dead Code (`P-NO-DEAD-CODE`):** Write ONLY code (functions, methods, types, fields, enum variants, and imports) that is directly called and required right now. NEVER write speculative helper functions, unused getters/setters, hypothetical fallbacks, or legacy code versions. Whenever a modification leaves existing code without a direct caller, delete the unused code immediately in the same step. Never comment out old code. **Exception for Compilation Stubs:** Empty placeholder functions or minimal stub structs (returning `Ok(())` or `Default`) that are actively wired into the call graph to allow incremental compilation for later development stages are explicitly permitted.

## 2. Deterministic File Layout (`.rs`)
Organize every Rust source file strictly in this vertical order:
1. **Module Docs:** `//! ...` at top of file. Keep `///` above items and `//` for domain quirks.
2. **Crate Attributes:** `#![...]` (mandatory `#![forbid(unsafe_code)]` in `main.rs` / `lib.rs`).
3. **Imports:** 2 groups separated by a blank line:
   - `std::` imports
   - `crate::`, `self::`, and `super::` imports (strictly in that ASCII order, including inside `mod tests`; never let `rustfmt` place `super::` before `crate::`)
   Alphabetize lines and braced items (`pub use x::{A, B};`).
4. **Constants & Statics:** Module-local `const` and `static` items, sorted alphabetically.
5. **Types & Impls:** `struct`, `enum`, and `type` definitions sorted alphabetically (`pub` before private). Every `impl` block MUST immediately follow its type definition:
   - **Inherent `impl Type`:** Constructors at top (`new`, `with_capacity`), all remaining methods sorted alphabetically below.
   - **Trait `impl Trait for Type`:** Sorted alphabetically by trait name directly below the inherent `impl`.
6. **Free Functions:** Sorted alphabetically (`pub` before private).
7. **Tests:** `#[cfg(test)] mod tests { ... }` at the bottom of the file.

## 3. Architecture, Types & Idioms
- **Formatting & Naming:** Indent with 4 spaces (`rustfmt`). Remove trailing spaces. Alphabetize `#[derive(...)]` macros and struct fields/enum variants within visibility tiers (`pub` > private). Do not abbreviate variable names (`container` > `cnt`, `element` > `el`; loop indices `i`, `j` are allowed) (`P-NO-ABBREV-VARS`).
- **Constructors & Traits:** Implement `Default` for all ZSTs and parameterless types. `new()` MUST delegate to `Self::default()` (or `Self` for ZSTs). Enforce `#[must_use]` on constructors and pure getters. Implement standard traits (`Display`, `FromStr`) instead of custom duplicate methods.
- **Error Handling:** Use strongly typed domain error enums (e.g., `PodError`); NEVER use `Result<T, String>`. Preserve error causes via `#[source]` / `Error::source` chaining (`P-ERR-PRESERVE`). Use `format!` for error messages over manual allocation micro-optimizations.
- **I/O & Process Isolation:** Isolate all direct `std::fs` and `std::process::Command` calls to dedicated `src/io/` modules (`src/io/exec/`, `src/io/psl/`, `src/io/system/`) (`P-IO-ISOLATION`; see [`ARCHITECTURE.md`](ARCHITECTURE.md)). Never use `println!` in core library logic; route diagnostics to `stderr` or structured output buffers.
- **State & Dispatch:** Use bitflags or fixed arrays instead of chained boolean fields (`P-MASK-SAFE`). Use static arrays (`[&'static dyn Trait; N]`) or index routing for O(1) dispatch (`P-O1-DISPATCH`).
- **Flat Control Flow & Dispatch (`P-FLAT-CONTROL-FLOW`, `P-FLATTEN-DISPATCH`):** Never exceed 2 to 3 levels of block indentation inside any function. Eliminate deep `if` / `if let` / `match` pyramids by using guard clauses, early `return` / `continue` / `break`, the `?` operator, and `let ... else { return ... };` statements. Keep dispatchers flat (<40 lines) and delegate branch logic immediately to dedicated helper functions.
- **Attributes:** Use `#[inline]` ONLY on trivial getters or proven hot-paths. NEVER put `#[inline]` on functions performing allocations, I/O, process spawning, CLI parsing, or large `match` blocks (`P-ATTR-USAGE`). NEVER use `#[allow(dead_code)]` or underscore-prefixed items (`_unused_fn`) to keep unused functions, types, or legacy fallbacks in the codebase (`P-NO-DEAD-CODE`).

## 4. Memory, Parsing & Performance
- **Zero-Waste Borrowing:** Accept `&str`, `&[u8]`, `&Path`, or `Cow<'a, str>` in parameters; NEVER accept owned `String` or `PathBuf` when read-only. Avoid unnecessary `.to_string()`, `.to_owned()`, or `PathBuf::from()`.
- **Bounded Allocations:** Always pre-allocate collections with `.with_capacity()` when size is known or estimable. Use named constants (`const MAX_CAPACITY: usize`) instead of magic numbers, clamp dynamic input sizes, and check `MAX_*` cardinality bounds BEFORE scanning the next element or allocating buffers to prevent OOM (`P-BOUND-ALLOC`). Query maps (`HashMap::get`) with borrowed keys (`P-ZERO-COST-LOOKUP`).
- **Zero-Copy Slicing:** Use `.split_once()`, `.strip_prefix()`, and safe cursor index offsets instead of intermediate `Vec` allocations or raw pointer arithmetic. Use `str::repeat()` over manual repeat loops (`P-IDIOM-REPEAT`).
- **PSL (`pods.conf` / `.pod`) & CLI Scanners:**
  - Treat [`PSL_SPEC.md`](PSL_SPEC.md) as the normative Single Source of Truth for PSL lexical rules, mandatory root-level block grammar, error conditions, and parser bounds (`P-BOUND-ALLOC`). When `PSL_SPEC.md` changes, strictly execute Spec-Driven TDD: update unit and integration tests FIRST, verify failure via `cargo test`, and modify `src/io/psl/` SECOND.
  - Parse PSL configuration (`pods.conf`) and recipe (`*.pod`) files using O(N) single-pass, quote-aware (`'`, `"`) and comment-aware (`#`) state-machine scanners (`P-UNIFIED-PARSER`, `P-NO-QUADRATIC`). Enforce identical post-token delimiter and block-header error kinds across bare words, single-quoted strings, and double-quoted strings.
  - NEVER use chained `.replace()` for templates or sanitization; use single-pass streaming.
  - Encapsulate UTF-8 boundary checks in small helpers (`P-UTF8-LOOKAROUND`).
  - Use `eq_ignore_ascii_case` in match guards instead of allocating `.to_ascii_lowercase()` (`P-ZERO-ALLOC-CASE`).
  - Parse CLI arguments and file paths using pure iterators over `OsStr` / `OsString` (`P-CLI-PURE`), handling `=` flags and rejecting empty values.
- **Flat Loops & Combinators (`P-NO-NESTED-LOOPS`, `P-LOOP-MODULARITY`, `P-FUNCTIONAL-COMBINATORS`):**
  - NEVER write nested loops (`for`, `while`, `loop` inside another loop) or place multi-arm `match` trees directly inside a loop body. Extract the inner loop or branch body into a dedicated, stateless helper function.
  - Keep every loop body under 25 lines and single-level (`P-LOOP-MODULARITY`).
  - Order `match` arms by enum declaration with `_ =>` last.
  - Use functional combinators (`.filter()`, `.map()`, `.fold()`) only when they remain shallow and readable (`P-FUNCTIONAL-COMBINATORS`). Never nest loops or multi-branch `if/else` blocks inside closures; extract a named helper function instead.
- **Buffers:** Use `out.write_str("...")` for static strings and `write!(out, "...", vars)` for dynamic formatting without splitting into multiple fragmented calls.

## 5. Testing, Documentation & Verification
- **TDD & Test Coverage:** Write unit and integration tests (`cargo test`) for every feature, parser routine, or bug fix:
  - Use semantic assertions (`.contains()`) over brittle exact-string matches.
  - Cover fallback `_ =>` branches (`M-FALLBACK-TESTS`), extreme capacity bounds (`M-EXTREME-BOUND-TESTS`), and delimiter/quote injection in PSL files (`M-DELIM-INJECT-TESTS`).
  - Test path edge cases: hidden files, dot paths, multi-extensions, and empty paths (`M-PATH-EDGE-TESTS`).
  - Test I/O and process logic using temporary directories, in-memory cursors, or mock executors (`M-IO-TESTS`).
- **Documentation & Sync:** Keep `USAGE.md`, `README.md`, `PSL_SPEC.md`, `.pods-completion.bash`, CLI help output, and root `pods.conf` / `recipe.pod` templates in sync per **`update-documentation`**. Whenever CLI modes, aliases, actions, or flags are added, modified, or removed, you MUST update `.pods-completion.bash` and `USAGE.md` in the same step. Follow **`changelog-updater`** for `CHANGELOG.md` and **`podoxide-dev-workflow`** for the edit-verify cycle.
- **Verification:** Run `cargo test` and `cargo clippy -- -D warnings` after every code modification.