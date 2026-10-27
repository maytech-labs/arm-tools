# 0044. Releases: tagged builds with prebuilt binaries

- **Status:** Accepted
- **Date:** 2026-10-27
- **Extends:** [0025](0025-distribution.md)

## Context

Until now the only ways to install were Homebrew's `--HEAD` (a build from `main`) and `make install` from a clone, both needing Rust. Going open source, people need a version to depend on and a binary to download.

## Decision

- Versions follow Semantic Versioning, starting at 0.1.0. The workspace shares one version, in `Cargo.toml`.
- A release is a tag `vX.Y.Z` on `main`. `.github/workflows/release.yml` checks the tag matches `Cargo.toml`, runs `cargo xtask check` on each platform, builds `armls` and `armtool`, and publishes a GitHub release with one archive per target (macOS arm64, Linux x86_64, Linux arm64), a SHA-256 checksum for each, and the version's section of `CHANGELOG.md` as notes.
- Each archive carries `LICENSE` and `crates/isa/LICENSE-LLVM.txt`, since the binaries contain tables derived from LLVM.
- `CHANGELOG.md` collects changes under **Unreleased**; cutting a release renames that section to the version and date.
- The libraries aren't published to crates.io yet: their APIs aren't stable, and the binaries are what people install.
- The Homebrew formula stays `--HEAD` until the first release exists; then it can point at the release's source archive with its checksum.

## Consequences

- Installing needs no Rust toolchain.
- macOS Intel isn't built; the tools target Apple silicon, and an Intel build is a small matrix addition if wanted.
