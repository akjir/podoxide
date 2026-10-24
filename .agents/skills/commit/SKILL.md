---
name: commit
description: Verifies that all tests pass, the project builds cleanly without Clippy warnings, and commits all open changes. Triggered when the user types `/commit` or `/commit <message>`.
---

# Verify & Commit Workflow

Execute this workflow whenever the user invokes `/commit` or `/commit <message>`. Follow all rules in `AGENTS.md` strictly.

## 1. Pre-Commit Verification Gate
Before staging or committing any changes, verify that the entire workspace is healthy:
1. **Run Test Suite:** Execute `cargo test` and verify that all unit, integration, and doc tests pass without failure.
2. **Run Linter & Compiler Checks:** Execute `cargo clippy --all-targets -- -D warnings` and verify zero compiler or Clippy warnings.
3. **Rule & Sync Check:** Inspect `git status --short` and `git diff` to ensure modified files comply with `AGENTS.md` (deterministic vertical layout starting with the verbatim GPL-3.0 license header from `src/main.rs` per `P-LICENSE-HEADER` and `//!` module docs, concise `///` doc comments on all production types, functions, and methods per `P-FN-DOCS`, flat control flow, zero dead code or `#[allow(dead_code)]` suppressions per `P-NO-DEAD-CODE`, documentation sync via **`update-documentation`**, and `CHANGELOG.md` sync via **`changelog-updater`** when user-facing changes exist). Flag any `.rs` file missing the license header or any production `fn`, `struct`, `enum`, or `type` missing a `///` doc comment as a violation that must be fixed before completing the workflow.
4. **Remediate Failures:** If any test fails or Clippy reports a warning, fix the root cause and re-run the verification suite until every check passes cleanly before proceeding to commit.

## 2. Stage & Commit All Open Changes
Once all verification checks pass:
1. **Stage Changes:** Run `git add -A` to stage all modified, new, and deleted files in the working tree.
2. **Verify Staged State:** Run `git status --short` and `git diff --cached --stat` to confirm that all open changes are staged and no unintended scratch files are included.
3. **Commit Message:**
   - If the user provided a message via `/commit <message>`, use that message (ensuring concise English per `AGENTS.md`).
   - Otherwise, write a concise, imperative English commit message (following ASD-STE100, zero emojis) that accurately summarizes the staged changes.
4. **Create Commit:** Execute `git commit -m "<subject>"` and report the resulting commit hash and clean working tree status (`git log -n 1 --oneline` and `git status --short`).
