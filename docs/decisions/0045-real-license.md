# 0045. License under REAL 1.0

- **Status:** Accepted
- **Date:** 2026-10-27
- **Supersedes:** [0005](0005-mit-license.md)

## Context

[0005](0005-mit-license.md) chose MIT. Before the first release, maytech-labs wrote its own license, the [Reciprocal Ethical AI License (REAL) 1.0](https://github.com/maytech-labs/real-license), and wants its projects under it.

REAL isn't OSI-approved and has no SPDX identifier. `crates/isa` contains tables generated from LLVM and predicates ported from LLVM's C++, which are Apache-2.0 WITH LLVM-exception ([0014](0014-isa-from-asm-matcher.md)). That license can't be removed from those parts, but Apache-2.0 section 4 lets a derivative work as a whole carry other terms as long as the notices stay.

## Decision

- `LICENSE.md` holds REAL 1.0, copyright maytech-labs. It replaces `LICENSE`.
- `crates/isa/LICENSE.md` says our code there is REAL 1.0, `src/generated.rs` and the ported predicates are also Apache-2.0 WITH LLVM-exception (`LICENSE-LLVM.txt`), and the crate as a whole is under both.
- Cargo manifests use `license-file`, since there's no SPDX expression to put in `license`. The Homebrew formula says `license :cannot_represent`. The VS Code extension says `SEE LICENSE IN LICENSE.md` and copies the file in when packaging.
- `cargo xtask notices` writes `THIRD-PARTY-NOTICES.md` from `cargo metadata`: each crate the binaries are built from, its license, and the text of its license files. `cargo xtask check` fails when it's stale.
- Release archives carry `LICENSE.md`, `LICENSE-LLVM.txt`, and `THIRD-PARTY-NOTICES.md`.
- The docs call the project source-available, not open source.

## Consequences

- Distributors and package indexes that need an SPDX identifier or an OSI license (crates.io accepts `license-file`; some Linux distributions don't accept non-OSI licenses) may not carry it.
- Contributors agree to REAL 1.0 for their contributions (CONTRIBUTING.md).
- Adding or updating a dependency means regenerating the notices.

## Alternatives considered

- **Keep MIT.** The owner chose REAL for its terms.
- **Dual license, MIT or REAL.** MIT alone would let anyone ignore REAL's terms, which defeats the purpose.
- **Hand-maintained notices.** They go stale; generating them from `cargo metadata` and checking in CI doesn't.
