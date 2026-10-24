---
name: commit
description: Commits all open changes after verifying that tests and Clippy checks pass (without modifying code), or commits immediately without verification when forced, and optionally pushes to origin. Triggered when the user types `/commit`, `/commit force`, `/commit push`, `/commit <message>`, `/commit force <message>`, or `/commit push <message>`.
---

# Commit Workflow

Execute this workflow whenever the user invokes `/commit`, `/commit force`, `/commit push`, `/commit <message>`, `/commit force <message>`, or `/commit push <message>`. Follow all rules in `AGENTS.md` strictly. Never modify, refactor, or fix any code or files during this workflow.

## 1. Pre-Commit Verification Gate (`/commit` and `/commit push` only)
- **Force Bypass (`/commit force` / `/commit force <message>`):** Skip this section completely. Do not run any tests or linter checks; proceed directly to Section 2.
- **Standard Verification (`/commit` / `/commit push`):** Before staging or committing any changes, verify that the workspace passes all checks:
  1. **Run Test Suite:** Execute `cargo test` and verify that all unit, integration, and doc tests pass.
  2. **Run Linter & Compiler Checks:** Execute `cargo clippy --all-targets -- -D warnings` and verify zero compiler or Clippy warnings.
  3. **Abort on Failure (No Code Changes):** If `cargo test` or `cargo clippy` fails, stop the workflow immediately. Do NOT edit or fix any files, do NOT stage or commit changes, and do NOT push. Report the failing test or compiler output directly to the user.

## 2. Stage & Commit All Open Changes
When invoked via `/commit force`, or once all verification checks in Section 1 pass:
1. **Stage Changes:** Run `git add -A` to stage all modified, new, and deleted files in the working tree.
2. **Verify Staged State:** Run `git status --short` and `git diff --cached --stat` to confirm that all open changes are staged.
3. **Commit Message:**
   - If the user provided `<message>` via `/commit <message>`, `/commit force <message>`, or `/commit push <message>`, use that message (ensuring concise English per `AGENTS.md`).
   - Otherwise, write a concise, imperative English commit message (following ASD-STE100, zero emojis) that accurately summarizes the staged changes.
4. **Create Commit:** Execute `git commit -m "<subject>"` and report the resulting commit hash and clean working tree status (`git log -n 1 --oneline` and `git status --short`).

## 3. Optional Push to Origin
If the user invoked `/commit push` (or `/commit push <message>`) and the verification gate and commit succeeded:
1. **Push Commit:** Execute `git push origin HEAD` to push the new commit on the current branch to `origin`.
2. **Verify Remote Sync:** Report the push result and verify the branch status via `git status -sb`.
