# 0046. Homebrew installs the release binaries

- **Status:** Accepted; hand-edited bump superseded by [0047](0047-org-homebrew-tap.md)
- **Date:** 2026-10-27
- **Extends:** [0025](0025-distribution.md), [0044](0044-releases.md)

## Context

The formula was HEAD-only and built from source. Homebrew's `rust` depends on its `llvm`, so a first install downloaded several hundred megabytes of LLVM and then compiled the workspace: many minutes for two small binaries. [0044](0044-releases.md) planned to point the formula at the release's source archive, which would still build.

## Decision

- The formula's stable version is the release's `aarch64-apple-darwin` archive, with its SHA-256. Installing copies `armls` and `armtool` into `bin`. It requires macOS on arm64, the only macOS archive we build.
- `--HEAD` still builds `main` with Homebrew's Rust; only the `head` block depends on `rust`.
- Each release updates `url`, `version`, and `sha256` by hand, as the last step in AGENTS.md's release checklist.

## Consequences

- `brew install arm-tools` takes seconds and needs no toolchain.
- The formula trails a release until someone updates it. The step is short and listed.
- homebrew-core wouldn't take a binary formula, but this is our own tap.
- Intel Macs aren't served by the formula. They weren't a target.

## Alternatives considered

- **Source archive, built by Homebrew.** Still pulls in Rust and LLVM; only saves the git clone.
- **Bottles built in CI.** Homebrew's own mechanism, but needs a bottle-building workflow and a place to host bottles. More than this needs today.
- **The release workflow commits the formula bump.** Saves a step but gives CI write access to `main`. Revisit if releases get frequent.
