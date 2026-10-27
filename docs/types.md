# Types for assembly

Assembly has no types, so armtool can't know that x0 is a pointer to 32-bit integers, or that x1 is unsigned, unless you say so. Say it in comments, and the linter checks that the code uses values the way you declared ([0039](decisions/0039-types.md)). Clang ignores the comments, so files assemble unchanged.

```asm
; armtool: fn _sum(x0: ptr<i32>, x1: u64) -> i64
        .globl  _sum
_sum:
        mov     x2, #0
1:      ldr     w3, [x0], #4    ; 4 bytes through ptr<i32>: fine
        sxtw    x3, w3
        add     x2, x2, x3
        subs    x1, x1, #1
        b.gt    1b              ; types/signedness: x1 is unsigned, use b.hi
        mov     x0, x2
        ret
```

## Annotations

An annotation is a comment containing `armtool:`. Write it with `;` or `//` on Darwin, and `//` on Linux.

- **Signatures:** `armtool: fn NAME(REGISTER: TYPE, ...) -> TYPE`. Put one anywhere in the file. For a function the file defines, the parameters are its registers' types at entry and the return type is checked at each `ret`. For any function, calls to it are checked, so a signature for a function defined elsewhere works as a declaration: `; armtool: fn _strlen(x0: ptr<u8>) -> u64`. End the list with `...` for variadic functions like printf, and write `-> void` for no result. Leave the arrow off if you don't want the result checked.
- **Registers:** `armtool: x9: ptr<u8>`, or several: `armtool: x9: ptr<u8>, x10: u64`. On the same line as an instruction, it gives the type after that instruction (`bl _malloc ; armtool: x0: ptr<u64>`). On its own line, it applies from the next instruction.

Name registers as you'd write them: `x0` and `w0` are the same register, as are `d0`, `s0`, and `v0`. Results come back in x0, or v0 for f32 and f64.

## Types

| Type | Meaning |
|---|---|
| `i8`, `i16`, `i32`, `i64` | Signed integers |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers |
| `f32`, `f64` | Floating point |
| `ptr<T>` | A pointer to T, like `ptr<u8>` or `ptr<ptr<i64>>` |
| `ptr` | A pointer to anything |
| a struct name | A struct declared with `armtool: struct` |

## How types flow

From annotations, types follow the code through branches and loops:

- `mov x1, x0` copies x0's type; a W register reads the low 32 bits, so `mov w1, w0` of an i64 gives i32.
- A load through `ptr<T>` gives T; `ldrsw` from `ptr<i32>` gives i64.
- `add` and `sub` of an integer keep a pointer's type; subtracting two pointers gives i64.
- `sxtw` gives i64, `uxtb` and `uxth` give u32, `scvtf` gives a float, and `fcvtzs` gives i64.
- A call to an annotated function sets x0 or v0 to its result. Every call forgets the types of the registers it may change (x0 to x18, v0 to v7, v16 to v31) and keeps x19 to x28 and v8 to v15.
- Anything else makes the register's type unknown. Where paths with different types meet, it's unknown too.

Nothing is reported about an unknown type, so a file without annotations gets no `types/` findings, and you can annotate one function at a time. Hover a register in the editor to see its type at that point.

## Structs

Declare a struct on one line, and use its name as a type ([0041](decisions/0041-structs.md)):

```asm
; armtool: struct point { x: i32, y: i32 }
; armtool: struct node { pos: point, next: ptr<node>, id: u64, tag: u8[4] }
; armtool: fn _walk(x0: ptr<node>) -> u64
_walk:
        ldr     w1, [x0, #4]      ; node.pos.y, an i32
        ldr     x0, [x0, #8]      ; node.next: x0 is a ptr<node> again
        ldr     x2, [x0, #12]     ; types/memory: offset 12 isn't at a field
```

The layout is C's: each field at the next multiple of its alignment (a scalar's size, or a struct's largest field), and the size rounded up to the struct's alignment. So `node` is 32 bytes: `pos` at 0, `next` at 8, `id` at 16, `tag` at 24.

Through a `ptr<node>`, loads and stores must land on a field and match its width; offsets past the first node count as the next ones. `add x1, x0, #8` gives a `ptr<ptr<node>>`, a pointer to the `next` field, and `add x1, x0, #32` a `ptr<node>` to the next node.

Structs defined in the project's C headers and sources count too, so a struct shared with C needs no annotation: `typedef struct node { ... } node_t;` makes `node` and `node_t` types, and a prototype's `node_t *` a `ptr<node>` ([0042](decisions/0042-c-structs.md)). Unions, bit-fields, and nested definitions are skipped.

Name constants for offsets as `struct_field`, and they're checked against the layout:

```asm
node_next = 8
node_id   = 12        ; types/layout: node.id is at offset 16
node_size = 32
```

## Signatures from C

You don't have to annotate functions written in C ([0040](decisions/0040-c-signatures.md)). Calls are checked against:

- prototypes and definitions in the project's `.h`, `.c`, and `.m` files, where the project is the directory with `armtool.toml`, or the nearest one with `.git`;
- about 40 common C library functions: printf and friends, the string and memory functions, malloc and free, and some math.

`long area(long w, long h);` in a header becomes `_area(x0: i64, x1: i64) -> i64` (`area` on Linux). An annotation in the file wins over both.

## printf

At a call to printf, fprintf, sprintf, snprintf, or dprintf, if the format register holds the address of a string in the file (`adrp` and `@PAGEOFF` or `:lo12:`, or `adr`), each `%` conversion is checked against its argument. On Linux, arguments are in x1 and up (after the fixed arguments), and doubles in v0 and up. On Darwin, they're on the stack, 8 bytes each, and passing them in registers is the classic mistake:

```asm
        adrp    x0, fmt@PAGE
        add     x0, x0, fmt@PAGEOFF
        mov     x1, #42           ; types/format: on Darwin, store it at [sp]
        bl      _printf
```

## What's checked

| Rule | Finds |
|---|---|
| `types/call` | Arguments that don't fit the callee's signature, and returns that don't fit the function's own |
| `types/signedness` | `b.gt` and friends after comparing unsigned values, `b.hi` and friends after signed ones, `sdiv` and `udiv` on the wrong kind |
| `types/memory` | Loads and stores of the wrong size for the pointee, offsets and pointer arithmetic that land mid-element, and non-pointers used as addresses |
| `types/format` | printf arguments that don't match the format string |
| `types/layout` | `struct_field` and `struct_size` constants that disagree with the layout |
| `types/pointer-width` | A pointer read through a W register |
| `types/annotation` | An annotation that can't be read |

An integer fits where a wider one is wanted if it's unsigned, since writing a W register zeroes the top half; a signed one needs `sxtw` first. Signed and unsigned integers of the same width fit each other.
