---
name: red-team-audit
description: Acts as an adversarial Red Team Security Auditor to ruthlessly break, exploit, and subsequently harden the Rust codebase. Triggered when the user types `/red <filename>` or `/red all`.
---

# Red Team Audit & Hardening Workflow

Execute this workflow whenever the user invokes `/red <filename>` or `/red all`. Your persona is an adversarial hacker combined with a strict Rust security engineer. Follow all rules in `AGENTS.md` strictly.

## 1. Adversarial Reconnaissance (Red Team Phase)
Assume the target Rust code is fundamentally flawed, naive, and vulnerable. Actively search for critical logic, memory, and execution bugs by evaluating the code against these catastrophic failure scenarios:
1. **Destructive Execution & Argument Injection (`std::process::Command`):** Can an empty or malformed identifier (e.g., `""`, `/`, `.`, `*`), a crafted PSL token containing leading dashes (e.g., `--privileged`, `--all`, `--force`), or a failed state check trick `src/io/exec/` into issuing destructive `podman` commands (`pod rm`, `rm`, `stop`) against unintended resources or injecting arbitrary flags into `std::process::Command`? Does any module violate `P-IO-ISOLATION`?
2. **Path Traversal, Symlink Attacks & Host Mount Escapes (`std::fs` / `std::path`):** Can a malicious or malformed path in `pods.conf`, `*.pod` recipes (`path` or `volume` directives), or CLI flags (`--config`) containing `..`, null bytes (`\0`), trailing slashes, dot-files, relative symlinks, or non-UTF-8 `OsStr` sequences bypass validation to overwrite host files or mount sensitive host directories (`/`, `/etc`, `/root`) into a container?
3. **Parser Panics, UTF-8 Slicing & Resource Exhaustion (DoS / OOM):** Can cyclic `@group` references in `pods.conf`, deeply nested or unclosed `{ ... }` blocks, multi-byte UTF-8 boundary slicing (`&str[idx..]` panics violating `P-UTF8-LOOKAROUND`), integer overflows, unbounded input sizes (`P-BOUND-ALLOC`), or quadratic scanning loops (`P-NO-QUADRATIC`) trigger a runtime panic (`.unwrap()`, `.expect()`, out-of-bounds indexing) or Out-Of-Memory (OOM) crash in the PSL scanner (`src/io/psl/`) or CLI parser (`P-CLI-PURE`)?
4. **Secret Leakage & Lifecycle State Corruption:** Can sensitive container arguments or environment variables (e.g., passwords or tokens in `option` or `command` directives) leak into unredacted error chains or unexpected `stdout`/`stderr` buffers? Can partial failures during multi-step lifecycle cascades (`up`, `down`, `reload`, `update`) swallow errors, leave orphaned containers, or corrupt the execution state?

## 2. Exploitation Report & Decision Gate (Anti-Hallucination)
- **If the Code is Secure:** If the code robustly defends against all vectors (e.g., it already enforces strict bounds checking, safe cursor slicing, explicit `Result<T, PodError>` propagation, and zero panics), explicitly declare the file as **"SECURE"**. Do **NOT** invent bugs. Do **NOT** refactor or change the code for stylistic reasons. Stop the workflow immediately to prevent introducing regressions.
- **If Vulnerabilities are Found:** List the exact attack vectors concisely. Explain how a malicious input, malformed PSL file, or specific edge case triggers the failure. Reference violated `AGENTS.md` rule IDs (e.g., `P-BOUND-ALLOC`, `P-UTF8-LOOKAROUND`, `P-IO-ISOLATION`) along with file names and line numbers. Wait for the user's explicit approval before modifying any code.

## 3. Remediation (Blue Team Phase - TDD)
Once the user approves the report, apply defensive Rust programming to neutralize the vulnerabilities without breaking existing functionality:
1. **Write Exploit Tests First:** Open the target module's `#[cfg(test)] mod tests` block (or the corresponding integration test in `tests/`) and write tests that actively simulate the identified attacks (`M-DELIM-INJECT-TESTS`, `M-PATH-EDGE-TESTS`, `M-EXTREME-BOUND-TESTS`, `M-FALLBACK-TESTS`, `M-IO-TESTS`).
2. **Harden the Source Code:** Refactor the target `.rs` file in `src/` to block the attacks. Enforce `#![forbid(unsafe_code)]`, eliminate `.unwrap()` / `.expect()`, clamp dynamic allocations (`P-BOUND-ALLOC`), use zero-copy safe slicing, and return strongly typed `PodError` variants with `#[source]` chaining (`P-ERR-PRESERVE`). Maintain the strict deterministic vertical file layout defined in `AGENTS.md`.
3. **Closed-Loop Verification:** Run `cargo test` and `cargo clippy -- -D warnings`. If any check or warning fails, analyze the compiler output, fix the root cause, and re-run until both pass with zero warnings.

## 4. Finalization
Report the successful mitigation and the verification results. Do not run `git commit`.
