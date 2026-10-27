# 0021. Quick fixes travel with diagnostics

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

Many diagnostics already say exactly what to change: "did you mean 'mov'?", "use 32", "add x0, x0, _msg@PAGEOFF". The editor and the command line should both be able to apply that change.

## Decision

- `arm_sema::Diagnostic` has an optional `Fix`: a title and text replacements by byte range. The layer that finds the problem builds the fix, so armls and armtool share one implementation.
- Fixes today: the suggested instruction, directive, or condition code; removing an ELF-only directive; rounding `add`/`sub sp, sp, #n` up to a multiple of 16; replacing a mismatched page-offset variant (only when the operand is exactly `sym@VARIANT`, so an addend is never lost); inserting the missing `add xN, xN, sym@PAGEOFF` after an `adrp`, indented to match.
- A fix is only offered when it's certain. There's none for x18, callee-saved registers, or unbalanced stacks, where the right change depends on intent.
- armls serves them as `quickfix` code actions for diagnostics that overlap the requested range. `armtool check --fix` and `lint --fix` apply every fix that doesn't overlap an earlier one, rewrite the file, then report what's left.
- armls also offers folding ranges: `.macro`, `.rept`, `.irp`, `.irpc`, and `.if` blocks, the code under each label, and runs of comment lines.

## Consequences

- Typos and the common address and alignment mistakes are one keystroke, or one command in CI, to fix.
- Each fix is tested by applying it and comparing text, in arm-sema, arm-lint, and over the real binaries.

## Alternatives considered

- **Fixes computed in armls from the message.** Parsing our own messages is fragile, and armtool couldn't share it.
- **Offer best-guess fixes for every rule.** A wrong automatic edit to assembly is worse than none.
