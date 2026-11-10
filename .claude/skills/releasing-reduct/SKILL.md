---
name: releasing-reduct
description: Use when releasing, tagging or publishing a new Reduct version, bumping the version number, preparing a Microsoft Store submission or update, or turning the Unreleased section of CHANGELOG.md into a version.
---

# Releasing Reduct

## Overview

A release turns `## [Unreleased]` in `CHANGELOG.md` into a version and tags it `v<version>`. CI then attaches the Store upload to a GitHub Release, and its `store` job submits the package and every listing in `store/` to the Microsoft Store once the maintainer approves the `microsoft-store` deployment. The version lives in three places that must agree: `Cargo.toml`, `Cargo.lock` and the tag. CI checks that the tag matches.

**Pushing to `main` and pushing the tag publish the release. Ask the maintainer before each of them, even when the release itself was requested. Never approve the `microsoft-store` deployment yourself: only the maintainer does that.**

## Choosing the version

Semantic versioning, from the Unreleased entries:

| Unreleased has | Bump | Example |
| --- | --- | --- |
| Only `Fixed` | patch | 1.1.0 → 1.1.1 |
| `Added` or `Changed` | minor | 1.1.0 → 1.2.0 |
| Settings or output names that stop working the old way | major | 1.1.0 → 2.0.0 |

If Unreleased is empty, there is nothing to release: say so and stop. If the maintainer asks for a version the table doesn't give, such as a patch for a release with `Added` entries, name the version the table gives and ask which one to use.

## Steps

1. Check that you're on `main` and that its last CI run passed: `git status`, `gh run list --branch main -L 1`. If the run is still going, wait for it with `gh run watch <id>`. If there are uncommitted changes, ask whether they belong in the release. The release commit holds only the files below.
2. `Cargo.toml`: set `version` under `[workspace.package]`.
3. `Cargo.lock`: set the same `version` for the four workspace packages `reduct`, `reduct-common`, `reduct-engine` and `reduct-shell`. Edit it by hand: this is the intended way off Windows, and CI's `--locked` build catches a mistake.
4. `CHANGELOG.md`:
   - Rename `## [Unreleased]` to `## [<version>] - <today, YYYY-MM-DD>`, the day you tag it.
   - Add an empty `## [Unreleased]` above it.
   - Update the links at the bottom: `[Unreleased]: …/compare/v<version>...HEAD`, and add `[<version>]: …/compare/v<previous>...v<version>`.
5. "What's new in this version" in `store/en-US.md`, `store/en-GB.md` and `store/nl.md`: it must match the new changelog section, in one or two plain sentences per language, with the note above it saying "For <version>.". If it's already there and matches, keep it. Otherwise, rewrite it. `en-US` uses American spelling and `en-GB` British, and the two say the same thing. The Dutch follows `store/nl.md`'s style (`je`, `Verkenner`).
6. If the release changes UI text, check that the listing descriptions and features in `store/` still match `crates/reduct-common/src/lang.rs`.
7. Run `cargo fmt --all --check`, review `git diff`, and commit `chore: released <version>`. Push `main` after the maintainer agrees, then watch CI with `gh run watch` until it passes.
8. Tag after the maintainer agrees: `git tag -a v<version> -m "Reduct <version>"`, then `git push origin v<version>`. Watch the tag's run. Its `release` job fails if the tag and the `Cargo.toml` version differ.
9. Set the GitHub Release notes: write the new changelog section's body (its `###` groups, without the `## [<version>]` heading) to a temporary file, then run `gh release edit v<version> --notes-file <file>`. Check with `gh release view v<version>` that `Reduct_<version>.0.msixupload` is attached.

## Hand-over to the maintainer (Store submission)

The tag's run waits at the `store` job for the maintainer. List these steps at the end of the release:

1. Open the tag's workflow run (`gh run view <id> --web`), check that `release` attached `Reduct_<version>.0.msixupload`, and choose **Review deployments → microsoft-store → Approve and deploy**.
2. The `store` job creates the submission, replaces the package, the four listings (`en-us`, `en-gb`, `nl-nl`, `nl-be`), the screenshots with their captions and the logos, commits it, and passes once the Store has started processing it. Certification then continues in Partner Center as usual.
3. If the job fails, its log says why. Before the commit, and after a rejection by the Store, the job deletes its submission, so re-running the job starts clean. If it says a submission is already in progress, delete that one in Partner Center first. If it timed out after the commit, check Partner Center before re-running.

Don't edit the listings in Partner Center: the next release overwrites them with `store/`.

## Common mistakes

- Bumping `Cargo.toml` without `Cargo.lock`: every CI build fails under `--locked`.
- Tagging before the version commit is on `main`: the tag's run fails its version check.
- Leaving a "What's new" that describes the previous release. The `store` job fails when its `For <version>.` note doesn't match `Cargo.toml` exactly, so bump the version in the release commit.
- Treating "just do everything" as permission to push or tag: still ask before each one.
- Changing the version for docs-only or CI-only changes: they have no changelog entry and need no release.
