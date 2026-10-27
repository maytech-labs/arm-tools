# 0002. Rust, built with Cargo

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

A language server runs for hours inside an editor and must start fast, use little memory, and never crash. The linter runs in CI and should be one binary with no runtime to install.

## Decision

- Rust, toolchain pinned in `rust-toolchain.toml`, built with Cargo.
- Repo automation is `cargo xtask`, in Rust, so the repo needs nothing but Cargo.
- `unsafe` is forbidden workspace-wide; `unwrap` and `expect` warn outside tests.

## Consequences

- Single static binaries for `armls` and `armtool`, easy to ship through Homebrew.
- The Rust LSP ecosystem (rust-analyzer's `rowan`, `lsp-server`, `lsp-types`) is mature and fits this design.
- A VS Code client still needs a small TypeScript extension; that's editor glue, not the tool.

## Alternatives considered

- **TypeScript on Node.** Easiest VS Code integration, but needs Node at runtime and is slower to parse large files.
- **Kotlin, reusing the old IntelliJ plugin's lexer.** The JFlex lexer is a small part of the work, and the JVM is a heavy runtime for a language server.
