# Podoxide AI Agent Rules

> [!IMPORTANT]
> Update this file, [`ARCHITECTURE.md`](ARCHITECTURE.md), and [`PSL_SPEC.md`](PSL_SPEC.md) when the architecture, module structure, file names, or PSL syntax change.

## 1. Core Constraints & Workflow
- **Goal:** Manage Podman pods and containers via the deterministic `pods` CLI in pure `std` Rust using declarative `pods.conf` and `*.pod` recipes in the block-based PodScript Language (`PSL`).
- **Zero Dependencies (`P-STDLIB-ONLY`):** Use `std` only. Keep `[dependencies]` and `[dev-dependencies]` in `Cargo.toml` empty.
- **Target & Execution:** Support Linux only (Rust Edition 2024, Podman 5.8.0+). Execute processes directly via `std::process::Command` without shell interpolation (`sh -c`).
- **Safety:** Enforce `#![forbid(unsafe_code)]` in crate roots. Forbid `static mut`. Never use `.unwrap()` or `.expect()` in production code (allowed only for proven mathematical invariants).
- **Version Control:** NEVER commit changes unless explicitly instructed by the user.
- **Language & Tone:** Write all responses, code, comments, docs, tests, and commits in concise English (ASD-STE100 80%). Never use emojis, filler text, or Markdown horizontal rules (`---`).
- **Simplicity & Vertical Clarity (`P-VERTICAL-CLARITY`):** Prioritize readability and flat control flow over dense code. Prefer extracting small, single-purpose helper functions over deep nesting, dense expressions, or complex inline closures.
- **Token Efficiency:** Avoid redundancy across code, docs, rules, and skills. Preserve all business logic, signatures, error types, and test semantics during refactors.
- **Direct Use & Zero Dead Code (`P-NO-DEAD-CODE`):** Write ONLY directly called functions, methods, types, fields, variants, and imports. Never write speculative helpers, unused getters/setters, hypothetical fallbacks, or commented-out code; delete orphaned items immediately in the same edit. **Compilation Stub Exception:** Minimal active no-op stubs (`Ok(())` or `Default`) wired into the call graph for unimplemented downstream stages are permitted.

## 2. Deterministic File Layout (`.rs`)
Keep `mod.rs` files minimal (submodule declarations, `pub use` re-exports, constants, and thin facade/dispatch entry points); place core logic in leaf `.rs` files named after their primary type or domain. Organize every `.rs` file in this exact vertical order:
1. **License Header (`P-LICENSE-HEADER`):** Verbatim GPL-3.0 comment block from `src/main.rs` at the top of the file, followed by one blank line.
2. **Module Docs:** `//! ...` directly below the license header (`///` on production items; `//` only for non-obvious domain quirks).
3. **Crate Attributes:** `#![...]` (mandatory `#![forbid(unsafe_code)]` in `main.rs` / `lib.rs`).
4. **Imports:** 2 alphabetized groups separated by one blank line:
   - `std::` imports, then `crate::`, `self::`, and `super::` imports in strict ASCII order (including in `mod tests`; never let `rustfmt` place `super::` before `crate::`).
   - **Single-Module Braces (`imports_granularity = Module`):** Import from one module path per `use` statement with alphabetized braces (`use crate::io::psl::{PslBlock, PslDocument};`); never nest `::` inside `{ ... }`.
   - **No Globs & Qualification:** Never use `use ...::*;` in production code (`use super::*;` is allowed only in `#[cfg(test)] mod tests`). Import types (`struct`, `enum`, `trait`) directly; qualify non-local free functions by parent module (e.g., `psl::parse_psl(...)`, `fs::read_to_string(...)`) so unqualified `foo()` calls denote local functions.
5. **Constants & Statics:** Module-local `const` and `static` items, sorted alphabetically.
6. **Types & Impls:** `struct`, `enum`, and `type` definitions sorted alphabetically (`pub` before private), each immediately followed by its `impl` blocks:
   - **Inherent `impl Type`:** Constructors at top (`new`, `with_capacity`), remaining methods sorted alphabetically below.
   - **Trait `impl Trait for Type`:** Sorted alphabetically by trait name directly below the inherent `impl`.
7. **Free Functions:** Sorted alphabetically (`pub` before private).
8. **Tests:** `#[cfg(test)] mod tests { ... }` at the bottom of the file.

## 3. Architecture, Types & Idioms
- **Formatting & Naming (`P-NO-ABBREV-VARS`):** Indent with 4 spaces (`rustfmt`) and strip trailing whitespace. Alphabetize `#[derive(...)]` items and struct fields/enum variants within visibility tiers (`pub` > private). Never abbreviate variable names (`container` > `cnt`, `element` > `el`; loop indices `i`, `j` are allowed).
- **Mandatory Documentation (`P-FN-DOCS`):** Document every production `fn`, method, constructor, `struct`, `enum`, and `type` (`pub`, `pub(crate)`, and private, including extracted helpers) with a concise `///` comment in factual English (ASD-STE100 80%, no emojis or filler). **Test Exception:** Omit `///` in `#[cfg(test)] mod tests` unless explaining a non-obvious edge case or attack vector.
- **Types, Constructors & Traits:** Favor concrete types over premature generics or unused trait hierarchies (`P-NO-DEAD-CODE`). Prefer named-field structs over multi-element tuples or multi-field tuple structs, and two-state enums over ambiguous `bool` parameters. Implement `Default` for ZSTs and parameterless types (`new()` MUST delegate to `Self::default()` or `Self`). Put `#[must_use]` on constructors and pure getters. Implement standard traits (`Display`, `FromStr`) over custom duplicates.
- **Error Handling (`P-ERR-PRESERVE`):** Use typed domain error enums (e.g., `PodError`); NEVER use `Result<T, String>`. Preserve error causes via `#[source]` / `Error::source` chaining. Use `format!` for error messages over manual allocation micro-optimizations.
- **I/O & Process Isolation (`P-IO-ISOLATION`, `P-PODMAN-API`):** Isolate all `std::fs` and `std::process::Command` calls to `src/io/` (`src/io/exec/`, `src/io/psl/`, `src/io/system/`; see [`ARCHITECTURE.md`](ARCHITECTURE.md)). Centralize all Podman CLI argument construction and execution in `src/io/exec/podman.rs`; no other module may build Podman arguments or invoke `"podman"` directly. Never use `println!` in core library logic; route output to `stderr` or structured buffers.
- **State & Dispatch (`P-MASK-SAFE`, `P-O1-DISPATCH`):** Use bitflags or fixed arrays instead of chained boolean fields (`P-MASK-SAFE`), and static arrays (`[&'static dyn Trait; N]`) or index routing for O(1) dispatch (`P-O1-DISPATCH`).
- **Flat Control Flow & Dispatch (`P-FLAT-CONTROL-FLOW`, `P-FLATTEN-DISPATCH`):** Never exceed 2–3 indentation levels. Deconstruct `Option` and `Result` via `match`, `if let`, `?`, or `let ... else` (never check `.is_some()` / `.is_ok()` then unwrap). Flatten deep pyramids with guard clauses and early `return` / `continue` / `break`. Keep dispatchers <40 lines and delegate branch logic to helper functions.
- **Attributes (`P-ATTR-USAGE`, `P-NO-DEAD-CODE`):** Put `#[inline]` ONLY on trivial getters or proven hot-paths; NEVER on functions performing allocations, I/O, process spawning, CLI parsing, or large `match` blocks (`P-ATTR-USAGE`). NEVER use `#[allow(dead_code)]` or `_unused` prefixes (`P-NO-DEAD-CODE`).

## 4. Memory, Parsing & Performance
- **Zero-Waste Borrowing (`P-ZERO-ALLOC`):** Accept `&str`, `&[u8]`, `&Path`, or `Cow<'a, str>` for read-only parameters; NEVER accept owned `String` or `PathBuf`. Avoid unnecessary `.to_string()`, `.to_owned()`, `.clone()`, or `PathBuf::from()`, especially in loops.
- **Bounded Allocations (`P-BOUND-ALLOC`, `P-ZERO-COST-LOOKUP`):** Pre-allocate collections with `.with_capacity()` using named constants (`const MAX_CAPACITY: usize`). Clamp dynamic sizes and check `MAX_*` cardinality bounds BEFORE scanning or allocating the next element to prevent OOM (`P-BOUND-ALLOC`). Query maps (`get`) with borrowed keys (`P-ZERO-COST-LOOKUP`).
- **Zero-Copy Slicing (`P-ZERO-COPY`, `P-IDIOM-REPEAT`):** Use `.split_once()`, `.strip_prefix()`, and safe cursor offsets instead of intermediate `.collect::<Vec<_>>()` allocations, string copies, or pointer arithmetic (`P-ZERO-COPY`). Use `str::repeat()` over manual repeat loops (`P-IDIOM-REPEAT`).
- **Pragmatic Allocation Boundary:** Do NOT sacrifice readability (`P-VERTICAL-CLARITY`) to eliminate required domain allocations (`Cow::Owned` for unescaped PSL strings, pre-allocated AST/Context collections via `P-BOUND-ALLOC`, or `format!` error strings).
- **PSL (`pods.conf` / `.pod`) & CLI Scanners:**
  - Treat [`PSL_SPEC.md`](PSL_SPEC.md) as the normative Single Source of Truth for PSL grammar, error conditions, and parser bounds (`P-BOUND-ALLOC`). On `PSL_SPEC.md` changes, follow Spec-Driven TDD: update unit/integration tests FIRST, verify failure via `cargo test`, and update `src/io/psl/` SECOND.
  - Parse `pods.conf` and `*.pod` using O(N) single-pass, quote-aware (`'`, `"`) and comment-aware (`#`) state-machine scanners (`P-UNIFIED-PARSER`, `P-NO-QUADRATIC`) with identical post-token delimiter and block-header error kinds across bare, single-quoted, and double-quoted tokens.
  - Use single-pass streaming instead of chained `.replace()`, small helpers for UTF-8 boundary checks (`P-UTF8-LOOKAROUND`), `eq_ignore_ascii_case` over `.to_ascii_lowercase()` (`P-ZERO-ALLOC-CASE`), and pure `OsStr` / `OsString` iterators for CLI args and paths (`P-CLI-PURE`, handling `=` flags and rejecting empty values).
- **Flat Loops & Combinators (`P-NO-NESTED-LOOPS`, `P-LOOP-MODULARITY`, `P-FUNCTIONAL-COMBINATORS`):**
  - NEVER nest loops (`for`, `while`, `loop`) or place multi-arm `match` trees inside a loop body; extract a stateless helper function (`P-NO-NESTED-LOOPS`). Keep loop bodies single-level and <25 lines (`P-LOOP-MODULARITY`). Order `match` arms by enum declaration with `_ =>` last.
  - Use shallow combinators (`.filter()`, `.map()`, `.fold()`) only when readable (`P-FUNCTIONAL-COMBINATORS`); prefer a flat `for` loop calling a helper over complex chains (`.filter().map().and_then()`) when propagating `?` errors or reducing cognitive load (`P-VERTICAL-CLARITY`). Never nest loops or multi-branch `if/else` blocks inside closures.
- **Buffers:** Use `out.write_str("...")` for static literals and consolidated `write!(out, "...", vars)` calls for formatted output.

## 5. Testing, Documentation & Verification
- **TDD & Test Coverage:** Write unit and integration tests (`cargo test`) for every feature, parser routine, or bug fix:
  - Use semantic assertions (`.contains()`) over brittle exact-string matches, and include concise context strings in `assert!` / `assert_eq!` when testing loops, table-driven cases, or state transitions.
  - Cover fallback `_ =>` branches (`M-FALLBACK-TESTS`), extreme capacity bounds (`M-EXTREME-BOUND-TESTS`), PSL delimiter/quote injection (`M-DELIM-INJECT-TESTS`), path edge cases (hidden, `.`, `..`, multi-extension, empty; `M-PATH-EDGE-TESTS`), and isolated I/O/process logic using temp dirs, cursors, or mock executors (`M-IO-TESTS`).
- **Documentation & Sync:** Keep `USAGE.md`, `README.md`, `PSL_SPEC.md`, `.pods-completion.bash`, CLI help output, and root `pods.conf` / `recipe.pod` templates synchronized per **`update-documentation`** (updating `.pods-completion.bash` and `USAGE.md` in the same step whenever CLI modes, aliases, actions, or flags change). Follow **`changelog-updater`** for `CHANGELOG.md` and **`podoxide-dev-workflow`** for the edit-verify cycle.
- **Verification:** Run `cargo test` and `cargo clippy -- -D warnings` after every code modification.
