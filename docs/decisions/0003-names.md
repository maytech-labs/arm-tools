# 0003. Names: arm-tools, armls, and armtool

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

The repo, the crates, and the binaries need names that say what they are and don't collide with common tools.

## Decision

- The repo is `arm-tools`.
- Library packages are `arm-<layer>` (`arm-syntax`, `arm-isa`, `arm-sema`, `arm-lint`), in `crates/<layer>`.
- The language server binary is `armls`, from package `arm-lsp`. Short like other servers editors launch by name, such as `zls` and `gopls`.
- The command line is `armtool`, from package `arm-cli`, with subcommands `lint`, `check`, `fmt`. One binary keeps the install simple.

## Consequences

- Short commands to type and configure.
- `arm-` names could be confused with official Arm tools; the README says this project isn't affiliated with Arm.

## Alternatives considered

- **`asm-*` names.** Too generic; this suite is AArch64 specific.
- **Separate `armlint` and `armfmt` binaries.** More to install and document, for no gain.
