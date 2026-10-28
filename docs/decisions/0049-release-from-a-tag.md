# 0049. Pushing a tag is the whole release

- **Status:** Accepted
- **Date:** 2026-10-28
- **Amends:** [0044](0044-releases.md)

## Context

A release took a commit by hand before the tag: the version in `Cargo.toml` and `editors/vscode/package.json`, `Cargo.lock`, and the changelog heading. 0.1.1 missed the extension's version and the fresh **Unreleased** heading.

## Decision

- A release is a tag `vX.Y.Z` on `main`'s latest commit, with entries under **Unreleased**.
- The release workflow's first job, `prepare`, sets the version from the tag, runs `cargo update --workspace`, renames **Unreleased** to `X.Y.Z - YYYY-MM-DD` under a fresh empty one, commits that to `main` as `chore: release X.Y.Z` with the workflow's token, and moves the tag onto that commit. The build checks out that commit.
- A tag on a commit that already has its version is built as it is, so a rerun, or a release bumped by hand, works.
- The job stops before changing anything when the tag isn't `main`'s latest commit or **Unreleased** is empty.

## Consequences

- One command releases: `git tag vX.Y.Z && git push upstream vX.Y.Z`.
- The tag moves once, seconds after it's pushed, before anything is published.
- Pushes made with the workflow's token don't start workflows, so CI doesn't run on the release commit; it changes only versions and the changelog.
- `main` has to accept pushes from GitHub Actions. If a branch rule blocks them, `prepare` fails and nothing is released.

## Alternatives considered

- **release-plz or cargo-release opening a release PR.** Another tool and another PR to merge, when a tag already says the version.
- **A manually run workflow that takes the version.** Same result, but a form in the Actions tab instead of a tag.
