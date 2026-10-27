# 0041. Struct layouts in type annotations

- **Status:** Accepted
- **Date:** 2026-10-27
- **Extends:** [0039](0039-types.md)

## Context

Assembly that works with C data reads fields at hand-counted offsets: `ldr w1, [x0, #12]`. When a field is added or reordered, every offset silently goes stale, and padding makes them easy to get wrong in the first place.

## Decision

- `armtool: struct name { field: T, field: T[N], ... }` declares a struct, on one line. Fields can be any type, another struct declared earlier, or an array.
- Layout follows C on AArch64 (AAPCS64): scalars align to their size, a struct to its largest field, each field at the next multiple of its alignment, and the size rounded up to the struct's alignment. Packed and bit-field structs aren't supported.
- Struct names are types: `ptr<node>` in signatures and register annotations. An unknown name is a `types/annotation` finding.
- Through a `ptr<S>`, a load or store at a constant offset finds the scalar field there, through nested structs and arrays (`pos.y`, `n[2]`), and offsets past the first struct count as later elements. An offset that isn't at a field, or a width that doesn't match it, is a `types/memory` finding; a load gives its register the field's type, so `ldr x2, [x0, #8]` of a `ptr<node>` field makes x2 a `ptr<node>`.
- `add` and `sub` of a `ptr<S>` by a multiple of S's size keep the type; by a field's offset they give a pointer to the field; anything else is a finding.
- New rule `types/layout`: constants named `S_field` and `S_size` are checked against the layout, so offsets written as named constants can't go stale unnoticed.

## Consequences

- A struct used by C and assembly is declared twice, in C and in the annotation; `types/layout` and the field checks catch a mismatch at its uses. Reading struct definitions from C headers is a possible next step.
