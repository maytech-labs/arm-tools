# 0025. Distribution: a Homebrew formula in this repo, and a thin VS Code extension

- **Status:** Accepted; tap superseded by [0047](0047-org-homebrew-tap.md)
- **Date:** 2026-10-26

## Context

People install command-line tools on macOS with Homebrew, and VS Code needs an extension to start a language server; other editors only need configuration. AGENTS.md says Rust only, without an ADR for anything else ([0002](0002-rust-and-cargo.md)).

## Decision

- **Homebrew:** this repo is its own tap, as in the owner's other projects: `Formula/arm-tools.rb` builds `armls` and `armtool` from `main` with Homebrew's Rust (`brew tap trammel-may/arm-tools https://github.com/trammel-may/arm-tools`, then `brew install --HEAD arm-tools`). HEAD-only until the first tagged release. Its license is `MIT` and `Apache-2.0 WITH LLVM-exception` ([0014](0014-isa-from-asm-matcher.md)). Its test checks the versions, lints a clean file, and checks that an out-of-range immediate is an `invalid-operand` error. `brew style` passes.
- **VS Code:** `editors/vscode` is a TypeScript extension, the one exception to Rust only, limited to editor glue: it registers `.s` and `.S` as `arm64-asm`, starts `armls` (or `armTools.serverPath`) with `vscode-languageclient`, and ships a small TextMate grammar so files look right before the server attaches. Every feature stays in armls. It's bundled with esbuild into one file, so the `.vsix` has no `node_modules`. Dependencies are pinned exactly.
- `npm run check` type-checks and `npm run package` builds the `.vsix`. Neither runs in `cargo xtask check`, which stays Rust only.

## Consequences

- Once the repo is on GitHub, `brew install --HEAD arm-tools` works, and `brew upgrade --fetch-HEAD arm-tools` updates it.
- The extension isn't tested inside VS Code yet: that needs `@vscode/test-electron`, which downloads VS Code. Its behavior is armls's, which is tested over stdio and in Neovim.
- Publishing to the VS Code Marketplace needs a publisher account; until then, install the `.vsix` by hand.

## Alternatives considered

- **A separate `homebrew-arm-tools` repo.** A shorter tap command, and a second repo to keep in step before there's a release.
- **No VS Code extension**, pointing people to a generic LSP client extension. Those need per-language configuration that most people won't write.
