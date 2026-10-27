# 0035. Keeping armls fast on large files and projects

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

The design budgets a full reparse of a 10,000 line file under 5 ms and assumes whole-file reanalysis per edit is fine. Nothing measured the rest. On a generated 10,000 line file (`crates/lsp/src/perf.rs`), each edit cost about 145 ms before semantic tokens, which took another 100 ms, and indexing a 400 file project held up startup for 1.15 s.

## Decision

- **Offset lookups by binary search.** `Analysis::symbol_at`, `reference_at`, and the new `definition_at` search ranges sorted by start, instead of scanning every symbol for each token. Semantic tokens and lint's control flow, which call them per token and per branch, went from quadratic to near linear.
- **Names resolve through a map.** Resolving references looks symbols up by name, not by scanning.
- **One analysis per edit.** The workspace index takes the analysis armls already made (`Index::update_from`) instead of analyzing the text again.
- **Edits coalesce.** With full document sync each change carries the whole text, so when several changes to a document are already queued, armls analyzes only the last. Typing fast, or a slow `.S` preprocessor run, doesn't build a backlog.
- **Indexing runs on every core** at startup, a file per task.
- Incremental parsing and analysis stay out: whole-file work is now well within budget.

## Consequences

Measured on an M-series Mac, release build, 10,000 lines: parse 6 ms, analysis 29 ms, analysis and lint 34 ms, semantic tokens 8 ms, formatting 8 ms; completion, hover, and signature help under 1 ms. Indexing 400 files of 1,000 lines takes 0.19 s. `ARMLS_PERF=<file>` and `ARMLS_PERF_SCAN=<dir>` with `cargo test -p arm-lsp --release perf -- --nocapture` rerun the measurements.

## Alternatives considered

- **A timer-based debounce.** Adds latency to every edit; coalescing only skips work that's already stale.
- **Index in a background thread.** Needs shared state across threads; parallel scanning was enough.
