---
name: red
description: Acts as an adversarial Red Team Security, Stability, Performance, Readability, and Code Efficiency Auditor to break, exploit, audit, and harden the Rust codebase. Triggered when the user types `/red`, `/red all`, `/red changed`, or `/red <file_or_dir...>`.
---

# Red Team Audit & Hardening Workflow

Execute this workflow on `/red`, `/red all`, `/red changed`, or `/red <path...>` (files or directories). Act as an adversarial hacker and strict Rust security, performance, and systems engineer. Enforce `AGENTS.md` and `PSL_SPEC.md` strictly.

## 1. Scope Resolution & Context-Isolated Dispatch
Audit **every file in an isolated context window**:
1. **Single File (`/red <file>`):** Audit the target file directly (or via one `research` subagent) with exclusive focus on that file.
2. **Multiple Files, Directory, or Changed Files (`/red <file1> <file2> ...`, `/red <dir>`, `/red changed`):**
   - Expand any directory path to all `.rs` files inside it (recursively).
   - For `/red changed`, resolve non-deleted `.rs` files modified, staged, or untracked relative to `HEAD` (`git diff --name-only --diff-filter=d HEAD -- '*.rs'` and `git ls-files --others --exclude-standard -- '*.rs'`); stop immediately if none changed.
   - Spawn **one `research` subagent per file** (`invoke_subagent`, `TypeName: "research"`, `Model: "inherit"`) with Section 2 instructions. Each subagent must inspect **only its assigned file** (reading external definitions only to verify called functions, `src/utilities/` helpers, or type contracts).
3. **Full Project (`/red` or `/red all`):**
   - Enumerate all `.rs` files under `src/`. Batch-check trivial re-export `mod.rs` and static `help.rs` files directly.
   - Audit remaining substantive `.rs` files via **one `research` subagent per file** in architectural waves, waiting for each wave to finish before starting the next:
     - **Wave 1 (I/O, Parser & Utilities):** `src/io/psl/*`, `src/io/exec/*`, `src/io/system/*`, `src/utilities/*`
     - **Wave 2 (Errors & Core Domain):** `src/error.rs`, `src/main.rs`, `src/lib.rs`, `src/core/*.rs`
     - **Wave 3 (CLI Modes):** `src/core/modes/**`

## 2. Five-Pillar Reconnaissance (Red Team Phase — Per-File Instructions)
Assume the assigned `.rs` file is flawed and non-compliant. Audit exhaustively across all five pillars:

### 2.1 Security (Sicherheit)
1. **Destructive Execution & Flag Injection (`std::process::Command`, `P-IO-ISOLATION`, `P-PODMAN-API`):** Can empty/malformed identifiers (`""`, `/`, `.`, `..`, `*`), leading-dash tokens (`-`, `--privileged`, `--all`, `--force`), or failed state checks trigger destructive `podman` commands (`pod rm`, `rm`, `stop`, `image prune`) or inject flags? Does any module outside `src/io/` violate `P-IO-ISOLATION` or build/invoke `"podman"` outside `src/io/exec/podman.rs` (`P-PODMAN-API`)?
2. **Path Traversal, Symlink Attacks & Host Mount Escapes (`std::fs` / `std::path`):** Can `pods.conf`, `*.pod` (`path` / `volume`), or `--config` paths with `..` (`has_parent_dir`), `\0`, trailing slashes, stemless `.conf`/`.pod` names, colons (`:` in `pods.path`/`recipes.path` corrupting `-v host:container:opts`), relative symlinks before `..` resolution, or non-UTF-8 `OsStr` values overwrite host files or mount sensitive directories (`/`, `/etc`, `/root`)?
3. **Special Files & TOCTOU Races (`std::fs`):** Can opening a FIFO, socket, directory, or device node (`/dev/zero`, `/dev/urandom`, `/dev/null`) hang or OOM? Are `symlink_metadata` and post-open `file.metadata().is_file()` checks enforced against TOCTOU swaps, and are parent directory existence and target path distinctness verified before multi-file writes?
4. **Terminal Escape, Control Character & BiDi Spoofing:** Can untrusted CLI args, paths, PSL tokens, or `io::Error` strings containing `\n`, `\r` (log forging), `\0`, ANSI escapes (`\x1b`), C1 controls (`\u{0080}..=\u{009f}`, CSI `\u{009b}`), or Unicode BiDi overrides (`\u{061C}`, `\u{200E}`, `\u{200F}`, `\u{202A}..=\u{202E}`, `\u{2066}..=\u{2069}`) reach `stdout`, `stderr`, or `Display` unsanitized or unclamped (`MAX_ERROR_TOKEN_BYTES`, `MAX_LOG_MESSAGE_BYTES`)?
5. **Secret Leakage & Safeguard Bypass:** Can credentials or sensitive options leak into errors or output buffers? Can any path mutate state when `simulate` (`--simulate` / `simulate = true`) is active, or bypass `runs_elevated()` (`UID 0`) confirmation (`MAX_CONFIRMATION_BYTES`, EOF, broken pipe)?

### 2.2 Stability (Stabilität)
1. **Panic-Free Invariants, Pattern Matching & UTF-8 Safety (`P-UTF8-LOOKAROUND`, `P-CLI-PURE`, `P-FLAT-CONTROL-FLOW`):** Does production code use `.unwrap()`, `.expect()`, `panic!()`, `unreachable!()`, `todo!()`, `unimplemented!()`, check-then-unwrap (`.is_some()` / `.is_ok()` instead of `match`, `if let`, or `let ... else`), or unchecked indexing (`slice[i]`, `&str[a..b]`) instead of `.get()`, `.split_first()`, `.split_once()`, `.strip_prefix()`, or `clamp_utf8_prefix`?
2. **Checked Integer Arithmetic & Casts:** Can raw `as` casts (`u64 as usize`), unchecked `+`/`-`/`*`, or lax numeric parsing overflow, wrap, or truncate instead of using `usize::try_from`, `.checked_add()`, `.saturating_add()`, or `parse_ascii_u32`?
3. **Pre-Scan Resource Bounds & Stream Limits (`P-BOUND-ALLOC`):** Are all bounds (`MAX_FILE_BYTES`, `MAX_NESTING_DEPTH`, `MAX_BLOCKS_PER_DOC`, `MAX_STMTS_PER_BLOCK`, `MAX_ARGS_PER_STMT`, `MAX_CLI_ARGS`, `MAX_DIRECTORY_ENTRIES`, `MAX_VERSION_OUTPUT_BYTES`, `MAX_VERSION_LINES`, `MAX_CONFIRMATION_BYTES`) checked **before** scanning or allocating the $(N+1)$-th element, and are `Read` streams bounded via `Read::take`?
4. **Deterministic State, Lifecycle Integrity & Error Chaining (`P-ERR-PRESERVE`):** Can partial failures in multi-step cascades (`up`, `down`, `reload`, `update`) swallow errors or leave inconsistent state? Are `BTreeMap`/`BTreeSet` or `.sort_unstable()` used instead of non-deterministic `HashMap`/`HashSet` iteration? Are cyclic `@group` refs rejected? Do domain errors chain `Error::source` without `Result<T, String>`, preserve `io::ErrorKind`, and handle `BrokenPipe` without panicking?
5. **Test Coverage, Assertion Context & Parallel Isolation (`M-FALLBACK-TESTS`, `M-EXTREME-BOUND-TESTS`, `M-DELIM-INJECT-TESTS`, `M-PATH-EDGE-TESTS`, `M-IO-TESTS`):** Is any fallback `_ =>` arm, `MAX_*` / `MAX_* + 1` boundary, delimiter/quote injection, path edge case (`.` / `..` / hidden / multi-extension / empty), or I/O failure untested? Do loop or table-driven assertions omit context strings in `assert!` / `assert_eq!`? Do tests mutate process-global state (`env::set_current_dir`) causing parallel `cargo test` races?

### 2.3 Speed & Performance (Geschwindigkeit)
1. **Zero-Waste Borrowing & Zero-Copy Slicing (`P-ZERO-ALLOC`, `P-ZERO-COPY`):** Does any function accept owned `String` / `PathBuf` when `&str`, `&[u8]`, `&Path`, or `Cow<'a, str>` suffices? Are there avoidable `.to_string()`, `.to_owned()`, `.clone()`, `PathBuf::from()`, intermediate `.collect::<Vec<_>>()` allocations (especially in loops), or unescaped PSL slices converted to `Cow::Owned`?
2. **Bounded Collection Pre-Allocation (`P-BOUND-ALLOC`):** Are `Vec::with_capacity` and `String::with_capacity` (clamped with `.min(MAX_*)` and named `INITIAL_*_CAPACITY` constants) used whenever sizes are known or estimable?
3. **Single-Pass O(N) Complexity & Syscall Efficiency (`P-NO-QUADRATIC`, `P-UNIFIED-PARSER`, `P-ZERO-ALLOC-CASE`, `P-ZERO-COST-LOOKUP`, `P-O1-DISPATCH`):** Are there hidden $O(N^2)$ loop scans, string re-scans, chained `.replace()`, `.to_ascii_lowercase()` allocations (vs. `eq_ignore_ascii_case`), owned lookup keys in `get`, or uncached `fs::metadata` / `fs::canonicalize` syscalls inside loops?
4. **Unfragmented Buffer Writes & Attribute Discipline (`P-ATTR-USAGE`, `P-IDIOM-REPEAT`):** Are consecutive `write!` / `push_str` calls consolidated, `write_str` used for static literals, and `str::repeat()` used over manual loops? Is `#[inline]` restricted to trivial getters/predicates (never on allocations, I/O, process spawning, or large `match` blocks), and are pure compile-time helpers marked `const fn`?

### 2.4 Readability & Deterministic Layout (Lesbarkeit)
1. **Strict 8-Part File Layout & Module Hygiene (`P-LICENSE-HEADER`, `AGENTS.md` Section 2):** Are `mod.rs` files minimal (declarations, `pub use` re-exports, constants, thin dispatchers) with core logic in leaf `.rs` files? Does the file follow the exact 8-part vertical order: (1) verbatim GPL-3.0 header + blank line, (2) `//!` docs, (3) `#![...]` attributes, (4) two-group alphabetized imports (`std::` then `crate::`, `self::`, `super::` in strict ASCII order including `mod tests`, single-module braces without nested `::`, no production glob `use ...::*;`, direct type imports vs. module-qualified non-local free functions), (5) alphabetized `const`/`static`, (6) alphabetized types + adjacent inherent/trait `impl`s, (7) alphabetized free functions, (8) `#[cfg(test)] mod tests`?
2. **Flat Control Flow, Combinators & Loop Modularity (`P-VERTICAL-CLARITY`, `P-FLAT-CONTROL-FLOW`, `P-FLATTEN-DISPATCH`, `P-NO-NESTED-LOOPS`, `P-LOOP-MODULARITY`, `P-FUNCTIONAL-COMBINATORS`):** Are there nested loops, multi-arm `match` or deep `if/else` blocks inside loops/closures, indentation $> 3$ levels, loop bodies $\ge 25$ lines, dispatchers $\ge 40$ lines, complex combinator chains (`.filter().map().and_then()`) where a flat `for` loop with a helper is clearer, or out-of-order `match` arms (`_ =>` not last)?
3. **Naming, Concrete Types & Documentation Integrity (`P-NO-ABBREV-VARS`, `P-FN-DOCS`):** Are variable names unabbreviated (`container` > `cnt`, `element` > `el`, `config` > `cfg`)? Are concrete types, named-field structs, and two-state enums used over premature generics, multi-element tuples, or ambiguous `bool` parameters? Are `#[derive(...)]`, fields, and variants alphabetized within visibility tiers (`pub` > private)? Does every production item have a concise `///` comment while `#[test]` functions omit redundant `///` comments?

### 2.5 Code Efficiency (Code-Effizienz)
1. **Zero Dead Code & Direct Use (`P-NO-DEAD-CODE`):** Does the file contain unused functions, methods, fields, variants, constants, imports, `_unused` items, `#[allow(dead_code)]`, or unreachable fallbacks (excluding minimal active `Ok(())` / `Default` stubs for unimplemented modules)?
2. **DRY & Shared Utility Reuse (`src/utilities/`):** Does the file duplicate validation, UTF-8 clamping, control/BiDi sanitization, identifier checking, or path normalization already provided in `src/utilities/{collection,format,path,string}.rs`?
3. **Compact State & Idiomatic Traits (`P-MASK-SAFE`):** Are bitflags (`SEEN_*`) or fixed arrays used instead of chained booleans? Do constructors and pure getters carry `#[must_use]`? Do parameterless types implement `Default` (`new()` delegating to `Self::default()`), and are standard traits (`Display`, `FromStr`) implemented instead of custom duplicates?

Each subagent must return either **"SECURE"** (if zero violations exist; never invent subjective issues outside `AGENTS.md` and `PSL_SPEC.md`) or confirmed findings tagged by pillar (`[Security]`, `[Stability]`, `[Speed]`, `[Readability]`, `[Efficiency]`) with line numbers, triggering inputs/defects, and violated rule IDs.

## 3. Consolidated Audit Report & Remediation Plan (Decision Gate)
Aggregate all per-file results in the orchestrator context:
- **Guard Against Allocation Over-Optimization:** Classify bounded, structurally required allocations (`P-BOUND-ALLOC`, AST nodes, `Cow::Owned` for unescaped PSL strings, `OsString` command vectors, `format!` errors) as **"SECURE"**. Never sacrifice `P-VERTICAL-CLARITY` for contrived lifetimes; report only unbounded (OOM) or trivially avoidable allocations (`P-ZERO-ALLOC`, `P-ZERO-COPY`, `P-ZERO-ALLOC-CASE`).
- **If All Files Pass:** Declare the audited scope **"SECURE"** and stop immediately without modifying code.
- **If Findings Exist:** Present a unified **Audit Report & Remediation Plan** and **wait for explicit user approval** before editing code:
  1. List files verified as **SECURE**.
  2. Group findings by file in bottom-up dependency order (`src/error.rs` -> `src/utilities/` -> `src/io/` -> `src/core/` -> `src/core/modes/`).
  3. State each finding's pillar tag, file, line numbers, defect/attack vector, violated rule IDs, and planned fix + test.

## 4. Remediation (Blue Team Phase — Adaptive TDD Execution)
After user approval, remediate findings without breaking existing behavior:
- **Small Scope (1–2 files):** Remediate directly one by one.
- **Large Scope (3+ files):** Delegate to **one `self` subagent per file**, executed **sequentially** in dependency order (shared `src/error.rs` or `src/utilities/` changes first) to keep each subagent's context isolated while preventing concurrent edits and Cargo build-lock contention.
- **Per-File Remediation Checklist:**
  1. **Exploit & Regression Tests First:** Add unit/integration tests covering the identified attacks, bounds, or fallback arms (`M-DELIM-INJECT-TESTS`, `M-PATH-EDGE-TESTS`, `M-EXTREME-BOUND-TESTS`, `M-FALLBACK-TESTS`, `M-IO-TESTS`, `P-FN-DOCS`).
  2. **Harden & Optimize Source:** Resolve all approved findings while preserving business logic and public contracts.
  3. **Compliance Gate:** Re-verify the modified file against Sections 2.4 and 2.5 (`AGENTS.md` Sections 2–4, `P-LICENSE-HEADER`, `P-FN-DOCS`, `P-NO-DEAD-CODE`).
  4. **Closed-Loop Verification:** Run `cargo test` and `cargo clippy -- -D warnings`, fixing any issues until both pass with zero warnings.

## 5. Finalization
Run workspace-wide `cargo test` and `cargo clippy --all-targets -- -D warnings`, then report the resolved findings and verification status concisely. Never run `git commit`.
