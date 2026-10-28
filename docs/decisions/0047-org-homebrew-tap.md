# 0047. The formula lives in the maytech-labs tap, and releases bump it

- **Status:** Accepted
- **Date:** 2026-10-27
- **Supersedes:** the tap in [0025](0025-distribution.md), and the hand-edited bump in [0046](0046-homebrew-binaries.md)

## Context

This repo was its own tap, so installing took `brew tap maytech-labs/arm-tools https://github.com/maytech-labs/arm-tools`. maytech-labs now has a shared tap, `maytech-labs/homebrew-tap`, for all its tools. Every release also needed a hand-edited formula commit, which [0046](0046-homebrew-binaries.md) kept to avoid giving CI write access to `main`.

## Decision

- `Formula/arm-tools.rb` moves to `maytech-labs/homebrew-tap`. This repo no longer has a `Formula/` directory.
- Installing is `brew tap maytech-labs/tap`, `brew trust maytech-labs/tap`, `brew install arm-tools`.
- The `homebrew` job in `.github/workflows/release.yml` runs after the release is published. It updates the formula's `url`, `version`, and `sha256` from the macOS archive's `.sha256` file and pushes `chore(arm-tools): X.Y.Z` to the tap's `main`.
- It authenticates with `HOMEBREW_TAP_TOKEN`, a fine-grained token with contents write on `maytech-labs/homebrew-tap` only.

## Consequences

- Shorter install commands, and one tap for every maytech-labs tool.
- The formula is current as soon as a release is, with no manual step.
- CI can write to the tap's `main`, not this repo's. A bad push breaks `brew install`, not the source.
- The token expires and has to be renewed in this repo's secrets. When it lapses, the `homebrew` job fails and the release still stands; bump the formula by hand until it's renewed.
- Formula changes other than a version bump are made in the tap.

## Alternatives considered

- **Keep this repo as its own tap.** No token, but a longer install command and a manual step per release.
- **Both taps.** `brew install arm-tools` is ambiguous for anyone with both tapped.
- **A GitHub App instead of a token.** Doesn't expire, but more setup than one repo needs today.
