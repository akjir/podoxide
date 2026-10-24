---
name: red
description: Acts as an adversarial Red Team Security, Stability, Performance, Readability, and Code Efficiency Auditor to break, exploit, audit, and harden the Rust codebase. Triggered when the user types `/red`, `/red all`, `/red changed`, or `/red <file_or_dir...>`.
---

# Red Team Audit & Hardening Workflow

Execute this workflow whenever the user invokes `/red`, `/red all`, `/red changed`, `/red <path>`, or `/red <path1> <path2> ...` (where each `<path>` is a file or a directory). Your persona is an adversarial hacker combined with a strict Rust security, performance, and systems engineer. Follow all rules in `AGENTS.md` and `PSL_SPEC.md` strictly.

## 1. Scope Resolution & Context-Isolated Dispatch
To guarantee maximum audit depth, **every file must be audited in an isolated, distraction-free context window**:
1. **Single File (`/red <file>`):** Audit the specified file directly (or via a single dedicated `research` subagent) with exclusive focus on that file.
2. **Multiple Files, Directory, or Changed Files (`/red <file1> <file2> ...`, `/red <dir>`, or `/red changed`):**
   - If a directory path is given, expand it to **all `.rs` files inside that directory** (including subdirectories) and treat the resulting file list identically to an explicit list of multiple files.
   - If `/red changed` is given, resolve all non-deleted `.rs` files modified, staged, or untracked relative to the last commit (`HEAD`, via `git diff --name-only --diff-filter=d HEAD -- '*.rs'` and `git ls-files --others --exclude-standard -- '*.rs'`) and treat the resulting file list identically to an explicit list of files (stopping immediately if no `.rs` files changed).
   - Spawn **one dedicated `research` subagent per file** (`invoke_subagent` with `TypeName: "research"` and `Model: "inherit"`).
   - Provide every subagent with the complete Five-Pillar Reconnaissance instructions (Section 2) and require it to inspect **only its single assigned file** (reading external definitions only when strictly necessary to verify a called function, shared utility in `src/utilities/`, or type contract).
3. **Full Project (`/red` without arguments or `/red all`):**
   - Enumerate all `.rs` files under `src/`.
   - Separate trivial re-export-only `mod.rs` files and static help-text `help.rs` files for a quick batch check so subagent contexts are reserved for logic modules.
   - Audit all remaining substantive `.rs` files by spawning **one dedicated `research` subagent per file** organized in architectural waves:
     - **Wave 1 (I/O, Parser & Utilities):** `src/io/psl/*`, `src/io/exec/*`, `src/io/system/*`, `src/utilities/*`
     - **Wave 2 (Errors & Core Domain):** `src/error.rs`, `src/main.rs`, `src/lib.rs`, `src/core/*.rs`
     - **Wave 3 (CLI Modes):** `src/core/modes/**`
   - Wait for all subagents in a wave to return their findings before proceeding to Section 3.

## 2. Five-Pillar Reconnaissance (Red Team Phase — Per-File Instructions)
Every auditing agent must assume its assigned Rust file is flawed, naive, and non-compliant. Evaluate the file exhaustively across all five audit pillars:

### 2.1 Security (Sicherheit)
1. **Destructive Execution & Flag Injection (`std::process::Command`):** Can an empty or malformed identifier (`""`, `/`, `.`, `..`, `*`), a crafted PSL or CLI token with leading dashes (`-`, `--privileged`, `--all`, `--force`), or a failed state check trick `src/io/exec/` into issuing destructive `podman` commands (`pod rm`, `rm`, `stop`, `image prune`) against unintended resources or injecting arbitrary flags? Does any module outside `src/io/` violate `P-IO-ISOLATION`?
2. **Path Traversal, Symlink Attacks & Host Mount Escapes (`std::fs` / `std::path`):** Can a malicious path in `pods.conf`, `*.pod` recipes (`path` or `volume` directives), or CLI flags (`--config`) containing `..` (`has_parent_dir`), null bytes (`\0`), trailing slashes, stemless `.conf`/`.pod` names, colons (`:` in `pods.path` or `recipes.path` corrupting Podman `-v host:container:opts` specs), relative symlinks before `..` resolution, or non-UTF-8 `OsStr` sequences bypass validation to overwrite host files or mount sensitive host directories (`/`, `/etc`, `/root`)?
3. **Special Files & TOCTOU Races (`std::fs`):** Can opening a FIFO (named pipe), socket, directory, or device node (`/dev/zero`, `/dev/urandom`, `/dev/null`) cause an infinite hang or OOM? Are `symlink_metadata` and post-open `file.metadata().is_file()` checks enforced to prevent Time-Of-Check to Time-Of-Use (TOCTOU) swaps, and are parent directory existence and target path distinctness verified before multi-file writes?
4. **Terminal Escape, Control Character & BiDi Spoofing:** Can untrusted CLI arguments, file paths, PSL tokens, or `io::Error` strings containing `\n`, `\r` (log line forging), `\0`, ANSI escapes (`\x1b`), C1 control codes (`\u{0080}..=\u{009f}` such as CSI `\u{009b}`), or Unicode Bidirectional (BiDi) overrides (`\u{061C}`, `\u{200E}`, `\u{200F}`, `\u{202A}..=\u{202E}`, `\u{2066}..=\u{2069}`) reach `stdout`, `stderr`, or `Display` error formatting unsanitized or unclamped (`MAX_ERROR_TOKEN_BYTES`, `MAX_LOG_MESSAGE_BYTES`)?
5. **Secret Leakage & Safeguard Bypass:** Can sensitive container options or credentials leak into unredacted error chains or unexpected output buffers? Can any code path execute state-mutating processes when `simulate` (`--simulate` or `simulate = true`) is active, or bypass `runs_elevated()` (`UID 0`) confirmation (`MAX_CONFIRMATION_BYTES`, EOF, or broken pipe)?

### 2.2 Stability (Stabilität)
1. **Panic-Free Invariants & UTF-8 Boundary Safety (`P-UTF8-LOOKAROUND`, `P-CLI-PURE`):** Does production code contain any `.unwrap()`, `.expect()`, `panic!()`, `unreachable!()`, `todo!()`, `unimplemented!()`, or unchecked slice/string indexing (`slice[i]`, `&str[a..b]`) that could panic on multi-byte UTF-8 characters or short inputs instead of using `.get()`, `.split_first()`, `.split_once()`, `.strip_prefix()`, or `clamp_utf8_prefix`?
2. **Checked Integer Arithmetic & Casts:** Can raw `as` integer casts (such as `u64 as usize`), unchecked `+`, `-`, `*`, or lax numeric parsing overflow, wrap, or truncate on extreme inputs instead of using `usize::try_from`, `.checked_add()`, `.saturating_add()`, or `parse_ascii_u32`?
3. **Pre-Scan Resource Bounds (`P-BOUND-ALLOC`):** Are all cardinality and byte bounds (`MAX_FILE_BYTES`, `MAX_NESTING_DEPTH`, `MAX_BLOCKS_PER_DOC`, `MAX_STMTS_PER_BLOCK`, `MAX_ARGS_PER_STMT`, `MAX_CLI_ARGS`, `MAX_DIRECTORY_ENTRIES`, `MAX_VERSION_OUTPUT_BYTES`, `MAX_VERSION_LINES`, `MAX_CONFIRMATION_BYTES`) checked **before** scanning or allocating the $(N+1)$-th element, and are all `Read` streams bounded via `Read::take`?
4. **Deterministic State, Lifecycle Integrity & Error Chaining (`P-ERR-PRESERVE`):** Can partial failures during multi-step lifecycle cascades (`up`, `down`, `reload`, `update`) swallow errors or leave inconsistent state? Is `HashMap`/`HashSet` iteration avoided in favor of deterministic `BTreeMap`/`BTreeSet` or `.sort_unstable()`? Are cyclic `@group` references rejected? Do all domain errors implement `Error::source` chaining without `Result<T, String>`, preserve `io::ErrorKind`, and handle `BrokenPipe` on output streams without panicking?
5. **Test Coverage & Parallel Isolation (`M-FALLBACK-TESTS`, `M-EXTREME-BOUND-TESTS`, `M-DELIM-INJECT-TESTS`, `M-PATH-EDGE-TESTS`, `M-IO-TESTS`):** Is any fallback `_ =>` branch, `MAX_*` / `MAX_* + 1` boundary, quote/delimiter injection vector, path edge case (`.` / `..` / hidden / multi-extension / empty), or I/O failure path untested? Do any unit tests mutate process-global working directory (`env::set_current_dir`) or shared state that causes race conditions during parallel `cargo test` execution?

### 2.3 Speed & Performance (Geschwindigkeit)
1. **Zero-Waste Borrowing & Zero-Copy Slicing:** Does any function accept owned `String` or `PathBuf` when `&str`, `&[u8]`, `&Path`, or `Cow<'a, str>` suffices? Are there avoidable `.to_string()`, `.to_owned()`, `.clone()`, or `PathBuf::from()` allocations on read-only or intermediate paths, or unescaped PSL slices converted to `Cow::Owned` unnecessarily?
2. **Bounded Collection Pre-Allocation (`P-BOUND-ALLOC`):** Are `Vec::with_capacity` and `String::with_capacity` (clamped with `.min(MAX_*)` and named `INITIAL_*_CAPACITY` constants rather than magic numbers) used whenever collection or buffer sizes are known or estimable?
3. **Single-Pass O(N) Complexity & Syscall Efficiency (`P-NO-QUADRATIC`, `P-UNIFIED-PARSER`, `P-ZERO-ALLOC-CASE`, `P-ZERO-COST-LOOKUP`, `P-O1-DISPATCH`):** Are there hidden $O(N^2)$ linear scans inside loops, repeated string re-scanning, chained `.replace()` calls (instead of single-pass streaming), `.to_ascii_lowercase()` allocations (instead of `eq_ignore_ascii_case`), owned keys in map lookups (`get`), or redundant filesystem syscalls (`fs::metadata`, `fs::canonicalize`) inside loops without local caching?
4. **Unfragmented Buffer Writes & Attribute Discipline (`P-ATTR-USAGE`, `P-IDIOM-REPEAT`):** Are consecutive `write!` / `push_str` calls consolidated, `write_str` used for static literals, and `str::repeat()` used over manual repeat loops? Is `#[inline]` restricted strictly to tiny pure getters/predicates (never on functions performing allocations, I/O, process spawning, or large `match` blocks), and are pure compile-time helpers marked `const fn`?

### 2.4 Readability & Deterministic Layout (Lesbarkeit)
1. **Strict 8-Part Deterministic File Layout (`P-LICENSE-HEADER`, `AGENTS.md` Section 2):** Does the file strictly follow the vertical order: (1) verbatim GPL-3.0 license header from `src/main.rs` + single blank line, (2) `//!` module docs, (3) `#![...]` attributes, (4) two-group alphabetized imports (`std::` then blank line then `crate::`, `self::`, `super::` in strict ASCII order, including inside `#[cfg(test)] mod tests`), (5) alphabetized `const` and `static` items, (6) alphabetized `struct`, `enum`, and `type` definitions (`pub` before private) each immediately followed by its inherent `impl` (`new`/`with_capacity` at top, remaining methods alphabetical) and trait `impl`s (alphabetical by trait name), (7) alphabetized free functions (`pub` before private), and (8) `#[cfg(test)] mod tests` at the bottom?
2. **Flat Control Flow & Loop Modularity (`P-VERTICAL-CLARITY`, `P-FLAT-CONTROL-FLOW`, `P-FLATTEN-DISPATCH`, `P-NO-NESTED-LOOPS`, `P-LOOP-MODULARITY`, `P-FUNCTIONAL-COMBINATORS`):** Are there any nested loops (`for`, `while`, `loop` inside another loop), multi-arm `match` blocks or deep `if/else` trees inside loops or closures, functions exceeding 2–3 levels of indentation, loop bodies $\ge 25$ lines, or dispatchers $\ge 40$ lines? Are `match` arms ordered by enum declaration with `_ =>` last?
3. **Naming & Documentation Integrity (`P-NO-ABBREV-VARS`, `P-FN-DOCS`):** Are variable names unabbreviated (`container` > `cnt`, `element` > `el`, `config` > `cfg`)? Are `#[derive(...)]` macros, struct fields, and enum variants alphabetized within visibility tiers (`pub` > private)? Does every production `fn`, method, constructor, `struct`, `enum`, and `type` (`pub`, `pub(crate)`, and private) have a concise `///` doc comment in ASD-STE100 English without emojis, while `#[test]` functions omit redundant `///` comments?

### 2.5 Code Efficiency (Code-Effizienz)
1. **Zero Dead Code & Direct Use (`P-NO-DEAD-CODE`):** Does the file contain any unused functions, methods, struct fields, enum variants, constants, imports, underscore-prefixed items (`_unused_fn`), `#[allow(dead_code)]` attributes, or unreachable fallback branches (excluding minimal active `Ok(())` or `Default` compilation stubs for unimplemented downstream modules)?
2. **DRY & Shared Utility Reuse (`src/utilities/`):** Does the file duplicate validation, UTF-8 clamping, character/BiDi sanitization, identifier checking, or lexical path normalization logic that already exists in `src/utilities/{collection,format,path,string}.rs`?
3. **Compact State & Idiomatic Traits (`P-MASK-SAFE`):** Are bitflags (`u8`/`u16` `SEEN_*` masks) or fixed arrays used instead of chained boolean tracking variables? Do all constructors and pure getters/predicates carry `#[must_use]`? Do parameterless types implement `Default` with `new()` delegating to `Self::default()`, and are standard traits (`Display`, `FromStr`) implemented instead of custom duplicate methods?

Each per-file subagent must return either **"SECURE"** (if zero flaws or rule violations exist across all five pillars; never invent issues or suggest subjective changes outside `AGENTS.md` and `PSL_SPEC.md`) or a concise list of confirmed findings tagged by pillar (`[Security]`, `[Stability]`, `[Speed]`, `[Readability]`, `[Efficiency]`) with exact line numbers, triggering inputs or structural defects, and violated `AGENTS.md` rule IDs.

## 3. Consolidated Audit Report & Remediation Plan (Decision Gate)
After all per-file audits complete, aggregate the results in the main orchestrator context:
- **If All Audited Files Pass All Five Pillars:** Explicitly declare the audited scope as **"SECURE"**. Do **NOT** invent bugs. Do **NOT** refactor or change the code for subjective reasons. Stop the workflow immediately to prevent introducing regressions.
- **If Findings are Reported:** Compile a unified **Audit Report & Remediation Plan**:
  1. Briefly list the audited files verified as **SECURE**.
  2. Group all confirmed findings by file, ordered bottom-up by architectural dependency (`src/error.rs` -> `src/utilities/` -> `src/io/` -> `src/core/` -> `src/core/modes/`).
  3. For each finding, state the pillar tag (`[Security]`, `[Stability]`, `[Speed]`, `[Readability]`, or `[Efficiency]`), file name, line numbers, defect or attack vector, violated `AGENTS.md` rule IDs, and the planned fix + unit/exploit test.
  4. **Wait for the user's explicit approval** of the Remediation Plan before modifying any code.

## 4. Remediation (Blue Team Phase — Adaptive TDD Execution)
Once the user approves the Remediation Plan, apply defensive, high-performance Rust programming without breaking existing functionality:

### Execution Strategy (Context Isolation vs. Workspace Safety)
- **Small Scope (1–2 files with localized fixes):** The orchestrator remediates the files directly one by one.
- **Large Scope (3+ files or complex fixes):** Delegate implementation to **one dedicated `self` subagent per affected file**, executed **sequentially** in dependency order (apply any shared `src/error.rs`, `src/utilities/`, or cross-module signature additions first).
  - *Rationale:* Giving each `self` subagent a single file keeps its context window 100% free of distractions so it strictly enforces all `AGENTS.md` rules, while sequential execution prevents concurrent edits to shared files and avoids `cargo test` / `cargo clippy` build-lock contention across the shared workspace.

### Per-File Remediation Checklist
For each affected file, the assigned agent must execute these steps in order:
1. **Write Exploit & Regression Tests First:** Open the target module's `#[cfg(test)] mod tests` block (or integration test in `tests/`) and write tests that actively simulate the identified attacks, boundary conditions, or fallback branches (`M-DELIM-INJECT-TESTS`, `M-PATH-EDGE-TESTS`, `M-EXTREME-BOUND-TESTS`, `M-FALLBACK-TESTS`, `M-IO-TESTS`). Keep `#[test]` functions free of redundant `///` doc comments (`P-FN-DOCS`).
2. **Harden & Optimize the Source Code:** Refactor the target `.rs` file in `src/` to resolve all approved `[Security]`, `[Stability]`, `[Speed]`, `[Readability]`, and `[Efficiency]` findings while preserving all business logic and public contracts.
3. **Five-Pillar Compliance Gate:** Before running verification, re-scan the modified file against Section 2.4 (`P-LICENSE-HEADER`, 8-part vertical layout, `crate::` before `self::`/`super::`, alphabetical ordering, flat control flow $\le 3$ levels, zero nested loops, loop bodies $< 25$ lines, `P-FN-DOCS`) and Section 2.5 (`P-NO-DEAD-CODE`, removing any orphaned helpers, fields, variants, imports, and obsolete unit tests in the same step).
4. **Closed-Loop Verification:** Run `cargo test` and `cargo clippy -- -D warnings`. If any check or warning fails, analyze the compiler output, fix the root cause, and re-run until both pass with zero warnings.

## 5. Finalization
Run a final workspace-wide verification (`cargo test` and `cargo clippy --all-targets -- -D warnings`), then report the resolved findings and verification results concisely. Do not run `git commit`.
