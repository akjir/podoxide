---
name: release
description: Executes the end-to-end Podoxide release workflow (pre-release fmt, clippy, test, rustdoc, and release build gates, Cargo.toml/Cargo.lock version alignment, CHANGELOG.md finalization without empty Unreleased section, git commit, annotated X.Y.Z tag without v prefix, push, and attested GitHub Release verification via gh). Triggered when the user types `/release <version>`, `/release v<version>`, or asks to release a version.
---

# Podoxide Release Workflow (`/release`)

Execute this end-to-end workflow autonomously whenever the user invokes `/release <X.Y.Z>` or `/release v<X.Y.Z>`. Because invoking `/release` is an explicit release instruction from the user, execute the git commit, annotated tag, push, and GitHub Release verification steps directly once all verification gates pass.

## 1. Input Parsing & Pre-Flight Validation
1. **Normalize Version & Tag:** Strip any leading `v` from the input argument to obtain `VERSION` (`X.Y.Z`) and validate strict SemVer (`^\d+\.\d+\.\d+$`). Set `TAG` to `X.Y.Z` without a leading `v` prefix (exact match to `Cargo.toml` `version` and Podoxide compile-time version metadata `<VERSION>+<count>.<hash>`). If no valid version is provided, abort and ask the user for `X.Y.Z`.
2. **Check Branch & Working Tree:** Confirm `git branch --show-current` is `main` (or the explicitly requested release branch) and verify `git status --porcelain` is empty so no unintended dirty or untracked files enter the release commit or trigger `.dev` metadata. Abort if the tree is dirty and ask the user to commit or stash first.
3. **Verify `gh` CLI & Repository:** Run `gh auth status` and resolve `REPO` via `gh repo view --json nameWithOwner --jq .nameWithOwner`. Abort immediately if `gh` is unavailable or unauthenticated.
4. **Check Conflicting Tags & Releases:**
   - Local tags: `git tag -l "X.Y.Z" "vX.Y.Z"` must return empty.
   - Remote tags: `git ls-remote --tags origin "refs/tags/X.Y.Z" "refs/tags/vX.Y.Z"` must return empty.
   - GitHub Releases: `gh release view X.Y.Z` and `gh release view vX.Y.Z` must report not found.
   Abort immediately on any conflict.
5. **Repository, Workflow, & Documentation Check:**
   - Verify required root files exist and are non-empty: `Cargo.toml`, `Cargo.lock`, `README.md`, `USAGE.md`, `PSL_SPEC.md`, `ARCHITECTURE.md`, `CHANGELOG.md`, `LICENSE`, `pods.conf`, `recipe.pod`, and `.pods-completion.bash`.
   - Verify `.github/workflows/release.yml` exists and includes `fetch-depth: 0` (so `build_metadata.rs` embeds the full Git commit count), strict `X.Y.Z` tag validation without `v`, `actions/attest-build-provenance`, `SHA256SUMS` generation, and `gh release create`. Abort if `.github/workflows/release.yml` is missing or misconfigured.
   - Verify `[dependencies]` and `[dev-dependencies]` in `Cargo.toml` remain empty (`P-STDLIB-ONLY`).
   - Verify `README.md`, `USAGE.md`, `PSL_SPEC.md`, `ARCHITECTURE.md`, `AGENTS.md`, and `.pods-completion.bash` match `src/` (per `update-documentation`).

## 2. Pre-Release Verification Gate
Run the full verification suite before modifying metadata:
1. `cargo fmt -- --check` (zero formatting diffs).
2. `cargo clippy --all-targets -- -D warnings` (zero warnings).
3. `cargo test --locked` (all unit, integration, and doc tests must pass).
4. `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked` (zero rustdoc warnings or broken intra-doc links).
5. `cargo build --release --locked` (clean optimized release binary build).
6. If Rust `1.88` is installed locally (`rustup toolchain list | grep -q '^1\.88'`), also run `cargo +1.88 check --all-targets --locked` and `cargo +1.88 test --locked`.
If any check fails, abort the release immediately and report the exact failure.

## 3. Version Alignment
Align all version-bearing files to `X.Y.Z`:
1. **`Cargo.toml`:** Set `[package]` `version = "X.Y.Z"`.
2. **`Cargo.lock`:** Update the root `podoxide` package entry to `version = "X.Y.Z"` via `cargo update --workspace --offline` (or direct alignment) and confirm no external dependencies were added.
3. **Templates & Docs:** Check whether `README.md`, `USAGE.md`, or `PSL_SPEC.md` require updates for this release (note that `PSL_SPEC.md` tracks the PSL specification version independently unless PSL syntax changed).

## 4. Changelog Finalization (`CHANGELOG.md`)
1. Verify `CHANGELOG.md` contains non-empty release notes under `## [Unreleased]` (or `## [X.Y.Z]`). If missing or empty, populate notable user-facing changes from commits since the last tag per `changelog-updater` (or abort if there are no changes to release).
2. Audit `## [Unreleased]` per `changelog-updater`: keep only notable user-facing, security, or release entries and remove internal implementation details or pre-release noise.
3. Replace `## [Unreleased]` with `## [X.Y.Z] - YYYY-MM-DD` using the current local date (`YYYY-MM-DD`).
4. **No Empty `[Unreleased]`:** Never insert or leave an empty `## [Unreleased]` heading in `CHANGELOG.md`.

## 5. Post-Bump Verification & Release Build
1. Re-run `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked`, and `cargo build --release --locked` to confirm all gates pass with the updated `Cargo.toml` and `Cargo.lock`.
2. Verify `target/release/pods` exists, is non-empty, and `./target/release/pods version` starts with `Podoxide X.Y.Z+` (no leading `v`).
3. Verify `cargo package --list --allow-dirty` excludes repository-internal files and directories (`.agents/`, `.github/`, `.test/`, `AGENTS.md`, `rustfmt.toml`).

## 6. Git Commit, Tag (`X.Y.Z`), & Push
1. Stage all release changes (`git add -A`) and inspect `git status --short` and `git diff --cached --stat` to confirm only intended version and changelog files are staged and no temporary, ignored, or `.test/` files are included.
2. Commit: `git commit -m "Release X.Y.Z"`
3. Create an annotated tag without a `v` prefix: `git tag -a "X.Y.Z" -m "Release X.Y.Z"`
4. Confirm `git status --porcelain` is empty so clean builds resolve to `X.Y.Z+<count>.<hash>` without a leading `v` or `.dev`.
5. Push branch and tag to `origin` to trigger `.github/workflows/release.yml`:
   `git push origin HEAD && git push origin "X.Y.Z"`

## 7. GitHub Release & Artifact Attestation Verification (`gh`)
GitHub build provenance attestations (`actions/attest-build-provenance`) require GitHub Actions OIDC signing and cannot be generated locally. Never bypass `.github/workflows/release.yml` by calling `gh release create` manually.
1. Resolve `COMMIT_SHA=$(git rev-parse "X.Y.Z^{commit}")` and `COMMIT_COUNT=$(git rev-list --count "$COMMIT_SHA")`, and find the triggered workflow run:
   `gh run list --workflow release.yml --commit "$COMMIT_SHA" --event push --limit 1 --json databaseId,status,conclusion,url`
   If GitHub has not registered the run yet, wait briefly (without a shell `sleep` loop) and query again.
2. Monitor the workflow run until completion:
   `gh run watch <run-id> --exit-status`
   If the run fails, inspect `gh run view <run-id> --log-failed`, abort, and report the exact failure.
3. Verify the published GitHub Release via `gh release view "X.Y.Z" --json tagName,isDraft,isPrerelease,url,assets`:
   - `tagName` equals `"X.Y.Z"` (no leading `v`).
   - `isDraft` is `false` and `isPrerelease` is `false`.
   - `assets` includes `podoxide-X.Y.Z-linux-x86_64.tar.gz` and `SHA256SUMS` with `size > 0`.
4. Download the published assets into a temporary directory `$TMP_VERIFY` and verify integrity, provenance attestations, and embedded version metadata:
   - `gh release download "X.Y.Z" -D "$TMP_VERIFY"`
   - Verify checksums: `(cd "$TMP_VERIFY" && sha256sum -c SHA256SUMS)`
   - Verify provenance attestations for every published asset (`podoxide-X.Y.Z-linux-x86_64.tar.gz` and `SHA256SUMS`):
     `gh attestation verify "$asset" --repo "$REPO" --signer-workflow "$REPO/.github/workflows/release.yml"`
   - Extract `podoxide-X.Y.Z-linux-x86_64.tar.gz` into `$TMP_VERIFY/extract`, verify bundled files (`pods`, `.pods-completion.bash`, `LICENSE`, `README.md`, `USAGE.md`, `PSL_SPEC.md`), and run `"$TMP_VERIFY/extract/podoxide-X.Y.Z-linux-x86_64/pods" version` to confirm output matches `Podoxide X.Y.Z+${COMMIT_COUNT}.<short-hash>` where `<short-hash>` is a prefix of `$COMMIT_SHA` with no leading `v` and no `.dev` suffix.
   - Remove `$TMP_VERIFY` on exit.
5. Report the final release summary: commit SHA, tag `X.Y.Z`, workflow run URL, GitHub Release URL, verified assets (`podoxide-X.Y.Z-linux-x86_64.tar.gz`, `SHA256SUMS`), SHA-256 checksum status, attestation verification status, and verified `pods version` output.
