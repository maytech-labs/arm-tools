# 0042. Struct definitions from C

- **Status:** Accepted
- **Date:** 2026-10-27
- **Extends:** [0040](0040-c-signatures.md), [0041](0041-structs.md)

## Context

A struct shared by C and assembly had to be declared twice: in C, and in an `armtool: struct` annotation ([0041](0041-structs.md)). The two drift.

## Decision

- The project scan for C prototypes ([0040](0040-c-signatures.md)) also reads `struct name { ... };` and `typedef struct [name] { ... } alias;` at the top level, and lays them out with 0041's rules. A typedef names the struct for prototypes and annotations alike: `node_t *` is `ptr<node>`.
- Structs that can't be laid out exactly are skipped, never guessed: unions, bit-fields, nested definitions, multi-dimensional arrays, and fields of types we don't know. A struct whose fields name another struct waits until that one is laid out.
- Prototypes now type `struct x *` as `ptr<x>`. A struct passed or returned by value drops the prototype, since it isn't in x0.
- An annotation in the file wins over C. Struct names in annotations are checked after C's structs are known, so `ptr<node>` with `node` defined only in a header is fine.

## Consequences

- One definition, in C, types the assembly too.
- Hover, which doesn't scan the project, knows only the file's own structs.
