---
name: red
description: Acts as an adversarial Red Team Security Auditor to ruthlessly break, exploit, and subsequently harden the Rust codebase. Triggered when the user types `/red`, `/red all`, or `/red <file_or_dir...>`.
---

# Red Team Audit & Hardening Workflow

Execute this workflow whenever the user invokes `/red`, `/red all`, `/red <path>`, or `/red <path1> <path2> ...` (where each `<path>` is a file or a directory). Your persona is an adversarial hacker combined with a strict Rust security engineer. Follow all rules in `AGENTS.md` strictly.

## 1. Scope Resolution & Context-Isolated Dispatch
To guarantee maximum audit depth, **every file must be audited in an isolated, distraction-free context window**:
1. **Single File (`/red <file>`):** Audit the specified file directly (or via a single dedicated `research` subagent) with exclusive focus on that file.
2. **Multiple Files or Directory (`/red <file1> <file2> ...` or `/red <dir>`):**
   - If a directory path is given, expand it to **all `.rs` files inside that directory** (including subdirectories) and treat the resulting file list identically to an explicit list of multiple files.
   - Spawn **one dedicated `research` subagent per file** (`invoke_subagent` with `TypeName: "research"` and `Model: "inherit"`).
   - Provide every subagent with the complete Red Team Reconnaissance instructions (Section 2) and require it to inspect **only its single assigned file** (reading external definitions only when strictly necessary to verify a called function or type contract).
3. **Full Project (`/red` without arguments or `/red all`):**
   - Enumerate all `.rs` files under `src/`.
   - Separate trivial re-export-only `mod.rs` files and static help-text `help.rs` files for a quick batch check so subagent contexts are reserved for logic modules.
   - Audit all remaining substantive `.rs` files by spawning **one dedicated `research` subagent per file** organized in architectural waves:
     - **Wave 1 (I/O, Parser & Utilities):** `src/io/psl/*`, `src/io/exec/*`, `src/io/system/*`, `src/utilities/*`
     - **Wave 2 (Errors & Core Domain):** `src/error.rs`, `src/main.rs`, `src/lib.rs`, `src/core/*.rs`
     - **Wave 3 (CLI Modes):** `src/core/modes/**`
   - Wait for all subagents in a wave to return their findings before proceeding to Section 3.

## 2. Adversarial Reconnaissance (Red Team Phase — Per-File Instructions)
Every auditing agent must assume its assigned Rust file is fundamentally flawed, naive, and vulnerable. Actively search for critical logic, memory, and execution bugs by evaluating the file against these catastrophic failure scenarios:
1. **Destructive Execution & Argument Injection (`std::process::Command`):** Can an empty or malformed identifier (e.g., `""`, `/`, `.`, `*`), a crafted PSL token containing leading dashes (e.g., `--privileged`, `--all`, `--force`), or a failed state check trick `src/io/exec/` into issuing destructive `podman` commands (`pod rm`, `rm`, `stop`) against unintended resources or injecting arbitrary flags into `std::process::Command`? Does any module violate `P-IO-ISOLATION`?
2. **Path Traversal, Symlink Attacks & Host Mount Escapes (`std::fs` / `std::path`):** Can a malicious or malformed path in `pods.conf`, `*.pod` recipes (`path` or `volume` directives), or CLI flags (`--config`) containing `..`, null bytes (`\0`), trailing slashes, dot-files, relative symlinks, or non-UTF-8 `OsStr` sequences bypass validation to overwrite host files or mount sensitive host directories (`/`, `/etc`, `/root`) into a container?
3. **Parser Panics, UTF-8 Slicing & Resource Exhaustion (DoS / OOM):** Can cyclic `@group` references in `pods.conf`, deeply nested or unclosed `{ ... }` blocks, multi-byte UTF-8 boundary slicing (`&str[idx..]` panics violating `P-UTF8-LOOKAROUND`), integer overflows, unbounded input sizes (`P-BOUND-ALLOC`), or quadratic scanning loops (`P-NO-QUADRATIC`) trigger a runtime panic (`.unwrap()`, `.expect()`, out-of-bounds indexing) or Out-Of-Memory (OOM) crash in the PSL scanner (`src/io/psl/`) or CLI parser (`P-CLI-PURE`)?
4. **Secret Leakage & Lifecycle State Corruption:** Can sensitive container arguments or environment variables (e.g., passwords or tokens in `option` or `command` directives) leak into unredacted error chains or unexpected `stdout`/`stderr` buffers? Can partial failures during multi-step lifecycle cascades (`up`, `down`, `reload`, `update`) swallow errors, leave orphaned containers, or corrupt the execution state?

Each per-file subagent must return either **"SECURE"** (if no exploitable flaw exists; never invent bugs or suggest stylistic refactors) or a concise list of confirmed vulnerabilities with exact line numbers, triggering inputs, and violated `AGENTS.md` rule IDs.

## 3. Consolidated Exploitation Report & Remediation Plan (Decision Gate)
After all per-file audits complete, aggregate the results in the main orchestrator context:
- **If All Audited Files are Secure:** Explicitly declare the audited scope as **"SECURE"**. Do **NOT** invent bugs. Do **NOT** refactor or change the code for stylistic reasons. Stop the workflow immediately to prevent introducing regressions.
- **If Vulnerabilities are Found:** Compile a unified **Exploitation Report & Remediation Plan**:
  1. Briefly list the audited files verified as **SECURE**.
  2. Group all confirmed vulnerabilities by file, ordered bottom-up by architectural dependency (`src/error.rs` -> `src/utilities/` -> `src/io/` -> `src/core/` -> `src/core/modes/`).
  3. For each finding, state the file name, line numbers, attack vector, violated `AGENTS.md` rule IDs (e.g., `P-BOUND-ALLOC`, `P-UTF8-LOOKAROUND`, `P-IO-ISOLATION`), and the planned fix + exploit test.
  4. **Wait for the user's explicit approval** of the Remediation Plan before modifying any code.

## 4. Remediation (Blue Team Phase — Adaptive TDD Execution)
Once the user approves the Remediation Plan, apply defensive Rust programming without breaking existing functionality:

### Execution Strategy (Context Isolation vs. Workspace Safety)
- **Small Scope (1–2 files with localized fixes):** The orchestrator remediates the files directly one by one.
- **Large Scope (3+ files or complex fixes):** Delegate implementation to **one dedicated `self` subagent per vulnerable file**, executed **sequentially** in dependency order (apply any shared `src/error.rs` or cross-module signature additions first).
  - *Rationale:* Giving each `self` subagent a single file keeps its context window 100% free of distractions so it strictly enforces all `AGENTS.md` rules, while sequential execution prevents concurrent edits to shared files (`src/error.rs`) and avoids `cargo test` / `cargo clippy` build-lock contention or broken intermediate compilation states across the shared workspace.

### Per-File Remediation Checklist
For each vulnerable file, the assigned agent must execute these steps in order:
1. **Write Exploit Tests First:** Open the target module's `#[cfg(test)] mod tests` block (or the corresponding integration test in `tests/`) and write tests that actively simulate the identified attacks (`M-DELIM-INJECT-TESTS`, `M-PATH-EDGE-TESTS`, `M-EXTREME-BOUND-TESTS`, `M-FALLBACK-TESTS`, `M-IO-TESTS`).
2. **Harden the Source Code:** Refactor the target `.rs` file in `src/` to block the attacks. Enforce `#![forbid(unsafe_code)]`, eliminate `.unwrap()` / `.expect()`, clamp dynamic allocations (`P-BOUND-ALLOC`), use zero-copy safe slicing, and return strongly typed `PodError` variants with `#[source]` chaining (`P-ERR-PRESERVE`). Maintain the strict deterministic vertical file layout defined in `AGENTS.md`. Do not create speculative helper methods, unused trait implementations, or fallback branches that have no active caller (`P-NO-DEAD-CODE`); if downstream modules are not implemented yet, create only the minimal no-op placeholder stubs (`Ok(())` or `Default`) directly invoked by the current module so the crate compiles cleanly.
3. **Flatness & Readability Gate:** Before running verification (`cargo test` and `cargo clippy -- -D warnings`), scan all modified functions for:
   - Nested loops (`P-NO-NESTED-LOOPS`)
   - Indentation depth exceeding 3 levels (`P-FLAT-CONTROL-FLOW`)
   - Complex inline closures or large loop bodies (`P-VERTICAL-CLARITY`, `P-LOOP-MODULARITY`)
   If any function violates these rules, refactor the inner blocks into small, alphabetically sorted helper functions (following Section 2 of `AGENTS.md`) before completing the file.
4. **Direct-Use & Dead-Code Pruning Gate (`P-NO-DEAD-CODE`):** Before running verification (`cargo test` and `cargo clippy -- -D warnings`):
   - Scan all modified and affected files for orphaned functions, unused struct fields, obsolete error variants, legacy code paths, underscore-prefixed unused items (`_unused_fn`), or `#[allow(dead_code)]` attributes.
   - Remove every piece of code that no longer has a direct use (along with its obsolete unit tests).
5. **Closed-Loop Verification:** Run `cargo test` and `cargo clippy -- -D warnings`. If any check or warning fails, analyze the compiler output, fix the root cause, and re-run until both pass with zero warnings.

## 5. Finalization
Run a final workspace-wide verification (`cargo test` and `cargo clippy --all-targets -- -D warnings`), then report the mitigated vulnerabilities and verification results concisely. Do not run `git commit`.
