# 0016. armls v1: features before operand checking, full sync, and a fixed token legend

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

The roadmap had the language server (M4) after instruction tables (M2) and the semantic model (M3). The parser, tables, and per-file model were enough for a useful server, and getting it into the owner's editor early gives feedback on everything after it.

## Decision

- **Ship armls before operand checking.** v1 reports syntax and semantic errors ([0015](0015-sema-file-model.md)), and offers hover, completion, go to definition, references, document highlight, rename, document symbols, and semantic tokens. Operand errors (`add x0, x0, #5000`) arrive with the matcher.
- **Editor features are pure functions** in `crates/lsp/src/ide.rs`, over an `Analysis` and byte offsets, unit-tested without LSP. `server.rs` only converts positions and types.
- **Full text sync**, reanalyzing the whole file on each change. A 10,000 line file parses in under 3 ms; incremental sync waits until a measurement says it's needed.
- **Positions:** UTF-8 when the client offers it, else UTF-16.
- **Hover:** for a mnemonic, its forms in the generic syntax, deduplicated, at most 16, with required features; for a register, its role in Apple's ABI; for a symbol, its kind, use count, and defining line; for a directive, a short description.
- **Semantic token legend:** mnemonics `keyword`, directives and macro calls `macro`, registers `variable` with `defaultLibrary`, constants `variable` with `readonly`, labels and other symbols `function`, register aliases `parameter`, `@` variants `decorator`, plus comments, numbers, and strings. Definitions carry `declaration`.
- **Rename** covers labels, constants, macros, and register aliases. It refuses local numeric labels, invalid names, register and instruction names, and names already defined, saying why.
- **Exit codes** follow the protocol: 0 after `shutdown` then `exit`, 1 for `exit` alone.
- **Tests:** the server's contract is tested by driving the real binary over stdio, with timeouts, including 300 random edits with requests after each. `cargo xtask nvim-smoke` runs armls inside headless Neovim through its own LSP client.

## Consequences

- The owner can use armls now, and operand checking and lints land as upgrades.
- Completion lists every mnemonic; filtering is the client's job.

## Alternatives considered

- **Finish M2 and M3 first.** No feedback from real use until much later.
- **Incremental sync.** More code for no measured gain yet.
