---
title: Reduct agent instructions
type: reference
status: active
created: 2026-11-08
project: [reduct]
tags: [agents, ci, build]
---

# Agent instructions

## Building and testing

Reduct is a Windows-only app. It is built and tested only on GitHub Actions ([.github/workflows/ci.yml](.github/workflows/ci.yml)), on an x64 and an ARM64 Windows runner.

**When not on Windows, do not try to compile, test or run anything locally.** This covers `cargo build`, `cargo check`, `cargo test` and `cargo run`, cross-compiling with `--target *-pc-windows-msvc`, and installing Rust targets or toolchains to make that work. The crates depend on the Windows API and on a Windows build of mozjpeg, so local builds fail or give misleading results. Even the platform-neutral tests in `reduct-common` fail off Windows, because they check Windows paths.

Instead:

1. Make the change, run `cargo fmt --all` and review the diff carefully. Formatting doesn't compile anything, so it is fine on any platform; CI fails on unformatted code.
2. Commit and push. CI runs the tests, the release build, a batch smoke test and the resource, export and C-runtime checks.
3. Check the workflow run (`gh run watch`, `gh run view --log-failed`) and fix what fails.

## Testing on Windows

- `bin-x64` / `bin-arm64` artifacts: the bare binaries. `Reduct.exe` opens the settings window. These don't include the File Explorer context menu, because Explorer only loads `ReductShell.dll` for an installed package.
- `reduct-sideload` artifact: the test-signed MSIX bundle. Install it to test the context menu. The steps are in the README and in the artifact's `INSTALL.txt`. It is built only for pushes to `main`, not for pull requests.

## Changelog

Every change a user can notice adds a line under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md), in the same commit: `Added`, `Changed`, `Fixed` or `Removed`. Describe what changes for the user, not the code: "The progress bar fell behind the count on fast batches", not "Reworked set_bar". Changes to CI, tests, refactors and docs that users don't see get no entry.

Releasing a version has its own steps: use the `releasing-reduct` skill ([.claude/skills/releasing-reduct/SKILL.md](.claude/skills/releasing-reduct/SKILL.md)).
