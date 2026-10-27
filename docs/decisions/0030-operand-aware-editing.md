# 0030. Signature help and completion from the operand forms

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Completion in operands offered every register and every symbol, whatever the instruction took there. And nothing showed what an instruction expects while you type it; hover only works on a finished mnemonic.

The instruction tables already have every form's operand classes ([0014](0014-isa-from-asm-matcher.md)), and the matcher knows how clang reads each operand ([0017](0017-operand-matching.md)).

## Decision

- **Prefix matching.** `arm_isa::match_prefix` runs the matcher in a prefix mode: the operands typed so far only need to begin a form, and each candidate comes back with the class the next operand goes in. Forms whose next class attaches to the operand before (`]`, `!`, `.4s`) are dropped, since the cursor is after a comma.
- **What's typed.** `Analysis::partial_operands` flattens the operand text before the cursor, leaving out the operand under it. An open `[` is closed for parsing and the closer dropped. Inside an open `{ }` list it gives up. Names resolve as at the end of the file.
- **Signature help** shows the candidate forms' signatures, split into operands at commas (`[` starts an operand; `]`, `!`, and suffixes attach), with the cursor's operand active. When nothing fits, it shows every generic form and counts commas, so a mistake doesn't hide what's expected. At most 30 signatures, deduplicated. Parameter offsets are in the negotiated position encoding, since labels contain `×`.
- **Completion** offers what fits the expected class of each candidate: registers that fit a register class, in numeric order, without `x31`/`w31`; condition codes, barrier options, prefetch operations, SVE patterns, shifts, and extends by predicate; labels for branches and `adr` (with other files' global labels), `label@PAGE` for `adrp`, `label@PAGEOFF` and constants for 12-bit offsets and add immediates; constants for other immediates. Register aliases come with registers. When nothing fits, it falls back to every register and symbol, as before.
- Mnemonic completions show the first form's signature as detail. Items carry a `sortText` so editors keep our order.

## Consequences

- `csel x0, x1, x2, ` offers only condition codes, and `ldr x0, [` only base registers.
- Classes we don't map (system registers, SME tiles, vector lists) offer nothing specific and fall back.
- Constants and aliases resolve as at the end of the file, not at the cursor; a constant redefined below the cursor can mislead. Rare in practice.

## Alternatives considered

- **Count commas only.** Simpler, but can't narrow `add w0, ` to its W forms, and miscounts inside `[ ]` for completion.
- **Hard-coded completion lists per mnemonic.** Duplicates the tables and drifts from clang.
