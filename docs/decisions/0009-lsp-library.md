# 0009. lsp-server and lsp-types, synchronous

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

`armls` needs an LSP transport and protocol types. The options in Rust are `tower-lsp` (and its fork `tower-lsp-server`), async on tokio, or `lsp-server`, the synchronous crate rust-analyzer uses.

## Decision

- `lsp-server` for the transport and `lsp-types` for messages.
- A single-threaded main loop over a channel. Requests are cheap (whole-file reparse is milliseconds), so no async runtime.
- If a request becomes slow (workspace-wide references), move it to a worker thread with cancellation, as rust-analyzer does.

## Consequences

- No tokio: a smaller binary, simpler stack traces, deterministic tests.
- We write our own request dispatch, which is a page of code.

## Alternatives considered

- **tower-lsp-server.** Less boilerplate, but async adds a runtime and makes ordering of document changes harder to reason about.
- **Hand-rolled JSON-RPC.** No benefit over `lsp-server`.
