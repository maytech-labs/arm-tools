# AGENTS.md

How to work in this repo, for AI agents and people. Read it before writing code.

## What this is

arm-tools is a Cargo workspace of tools for AArch64 assembly on macOS: a language server, a linter, and the shared core both are built on.

| Crate | Package | What it is |
|---|---|---|
| `crates/syntax` | `arm-syntax` | Lexer and lossless, error-tolerant parser. Every byte of the input is in the tree. |
| `crates/isa` | `arm-isa` | AArch64 instruction tables generated from LLVM: mnemonics, operand forms, registers, features. `src/generated.rs` is written by `cargo xtask gen-isa`; never edit it by hand. Licensed MIT and Apache-2.0 WITH LLVM-exception ([0014](docs/decisions/0014-isa-from-asm-matcher.md)). |
| `crates/sema` | `arm-sema` | The semantic model: symbols, labels, macros, `.include`, sections, conditional assembly |
| `crates/lint` | `arm-lint` | The lint engine and its rules. Adding a rule: add it to `RULES` in `rules.rs` with tests, then regenerate docs/lint.md (`crates/lint/tests/docs.rs` fails until you do) |
| `crates/lsp` | `arm-lsp` | The `armls` binary: the language server |
| `crates/cli` | `arm-cli` | The `armtool` binary: `check`, `lint`, `rules`, `explain` from the command line and CI |
| `xtask` | (not published) | Repo automation: `cargo xtask check`, `survey`, `gen-isa`, `nvim-smoke` |
| `editors/` | | Editor setup; `editors/neovim/smoke.lua` is the headless Neovim test |

Dependencies point one way: `syntax` and `isa` at the bottom, then `sema`, then `lint`, then `lsp` and `cli`. Nothing lower depends on anything higher.

The design is in [docs/design.md](docs/design.md), the milestones in [docs/roadmap.md](docs/roadmap.md), and decisions in [docs/decisions/](docs/decisions/).

## Working loop

1. Read the section of `docs/design.md` for what you're building, its milestone in `docs/roadmap.md`, and any ADRs they link.
2. Write the test first where practical (see [Testing](#testing)).
3. Run `cargo xtask check` (fmt, clippy with warnings as errors, all tests). CI runs the same on macOS and Linux.
4. Update docs in the same change: the design's status lines, the roadmap's checkboxes, and `CHANGELOG.md` under **Unreleased**.
5. Record every decision you made as an ADR (below).

## Rules

1. **Rust only**, built with Cargo ([0002](docs/decisions/0002-rust-and-cargo.md)). The one exception is `editors/vscode`, TypeScript glue that starts armls ([0025](docs/decisions/0025-distribution.md)); check it with `npm run check` there. Prefer small dependencies; say why in the commit or an ADR when you add one.
2. **Apple's assembler is the source of truth** ([0004](docs/decisions/0004-apple-aarch64-first.md)). If clang (`clang -c -arch arm64`) accepts it, we accept it; if clang rejects it, we report it. When unsure, probe clang and write the result down as a test.
3. **Never lose a byte.** The syntax tree is lossless ([0008](docs/decisions/0008-lossless-syntax-tree.md)). The parser never fails: bad input becomes error nodes plus diagnostics, and the rest of the file still parses.
4. **Never crash the server.** A panic in `armls` kills the user's editor session. Same rule as below: no `unwrap`/`expect` outside tests unless a comment proves it can't fail.
5. **Diagnostics that teach.** Every diagnostic says what's wrong and what to do, and lint diagnostics name their rule ID.
6. **Nothing leaves the machine.** No telemetry, no network calls.
7. **No `unsafe`** (forbidden workspace-wide).

## Testing

Two kinds of test, plus an oracle ([0007](docs/decisions/0007-testing.md)).

- **Unit tests** sit beside the code in `#[cfg(test)] mod tests`. They drive the code test-first and cover edge cases. Rewrite them freely in a refactor.
- **Integration tests** in `crates/<crate>/tests/` pin the contract: the public API for library crates, and for `armls` and `armtool`, the real binary over stdio or the command line. Change one only when the contract changes.
- **Oracle tests** run `.s` fixtures through Apple's clang and check we agree on what's valid. They run on macOS and skip elsewhere.
- **armls tests** drive the real binary over stdio with timeouts (`crates/lsp/tests/lsp.rs`). Editor features live in `crates/lsp/src/ide.rs` as pure functions with unit tests. After changing the server, also run `cargo xtask nvim-smoke`.
- **Fuzzed edit session** (`crates/lsp/src/fuzz.rs`): the real server, in memory, takes random edits to the examples and corpus and every request after each; it must answer all of them without a panic. Formatting mutated files must change only whitespace and be stable. Before a release, soak it: `ARMLS_FUZZ=300 cargo test -p arm-lsp --release soak`. A failure names its seed.
- **Snapshot tests** for parser output live in `crates/syntax/tests/data/parser/`: a `.s` input beside its `.tree`. After a deliberate change, run `UPDATE_SNAPSHOTS=1 cargo test -p arm-syntax --test parse` and read the diff before committing.
- **Corpus:** every `.s` in `crates/syntax/tests/data/corpus/` must parse without errors and assemble with clang. Lines in `tests/data/invalid.s` must be reported by us and rejected by clang.

- **Survey:** after any parser change, run `cargo xtask survey` on real assembly. The bar is zero disagreements: every line we report must be one clang rejects. The reference corpus isn't checked in; fetch it once:

  ```sh
  git clone --depth 1 --filter=blob:none --sparse https://github.com/llvm/llvm-project /tmp/llvm-project
  git -C /tmp/llvm-project sparse-checkout set llvm/test/MC/AArch64
  cargo xtask survey /tmp/llvm-project/llvm/test/MC/AArch64 crates/syntax/tests/data
  ```

- **Instruction tables:** to regenerate, check out the LLVM tag matching `llvm-tblgen --version` (21.1.8 today) and run `cargo xtask gen-isa --llvm <checkout> --tblgen $(brew --prefix llvm)/bin/llvm-tblgen`. Run `cargo xtask gen-isa` alone for the sparse checkout command. The output must be identical when nothing in LLVM changed.

To see how something parses: `echo 'ldr x0, [x1, #8]!' | cargo run -q -p arm-syntax --example tree`.

## Recording decisions

Every time you make a decision, document it in `docs/decisions/`. The next agent has no memory of your session; the ADR is how it learns what you chose and why.

A decision needs an ADR when you chose between reasonable options and a future reader could ask "why is it like this?": a dependency, a data format, an algorithm, a lint rule's default, an interpretation of Apple's assembler behavior, or a tradeoff. Following an existing pattern doesn't need one.

Copy `docs/decisions/0000-template.md` to the next number, and add it to the list in `docs/README.md`. Keep it short. ADRs are append-only: to change a decision, write a new one that supersedes the old, and change only the old one's status line.

## Writing

Plain, direct, and concrete, in docs, comments, diagnostics, and commit messages. Short sentences, active voice, contractions. No em dashes, no hype, no exclamation points. Sentence case for headings. Name the file, the command, the instruction.

## Commits and pull requests

Commits follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) and stay terse ([0006](docs/decisions/0006-commit-messages.md)):

- **Subject only**, lowercase, imperative, no period, under about 50 characters: `feat(syntax): lex local label references`.
- **Types:** `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`. **Scopes** are crate directory names (`syntax`, `isa`, `sema`, `lint`, `lsp`, `cli`, `xtask`) or left off.
- A body only when the why isn't obvious, and then a line or two.
- Breaking changes: `!` after the type.
- **No tool attribution of any kind**: no `Co-Authored-By` trailers for AI tools, no "Generated with" lines, in commits or PR descriptions.
- One logical change per commit. Work on a branch; `main` stays green.
