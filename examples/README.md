# Examples

Small programs that build and run on Apple silicon, to read and to try armls on. Build and run them all:

```sh
make run          # from this directory
make fib          # or one at a time, into build/
./build/fib
```

| Program | Shows | Try in the editor |
|---|---|---|
| [hello](hello/hello.s) | Raw Darwin system calls: `write` and `exit` through `svc #0x80` with the number in x16, and `adrp` + `@PAGEOFF` to load an address | Inlay hints show `syscall write`; hover `x16`; on a new line, the `write` and `exit` snippets write these for you |
| [sum](sum/sum.s) | Calling `printf`. On Apple silicon, variadic arguments go on the stack, not in x1 | Hover `_printf`'s format, `str`, and `sp`; go to definition on `sum` |
| [fib](fib/fib.s) | Recursion: frame records, and callee-saved x19 and x20 kept across calls | Hover `x19` for its ABI role; delete the `stp x19, x20` to see `abi/callee-saved` |
| [neon](neon/vecadd.s) | NEON: `ld1`, `fadd` on four floats at once, `st1` | Hover `fadd` for what it does and its forms; retype `fadd v0.4s, ` to see signature help and only vector registers offered; change `.4s` to `.8h` in one operand |
| [macros](macros/main.s) | `.include`, macros with default arguments, `.irp`, `.rept` | Go to definition on `print` jumps to [syscalls.s](macros/syscalls.s); break the macro body and the error appears at the call |
| [preprocessor](preprocessor/strlen.S) | A `.S` file: `#include`, `#define`, a function-like macro, `#if` | Errors map back through the preprocessor; set `VERBOSE` to 0 |
| [linked](linked/main.s) | A program in two files, assembled separately and linked: calls go both ways between them | `gd` on `_print_line` jumps to [text.s](linked/text.s); `grr` lists uses in both files; `grn` renames it in both; `:lua vim.lsp.buf.incoming_calls()` shows its callers |
| [strings](strings/upper.s) | Byte loads and stores (`ldrb`, `strb`), editing a string in place, writable data in `__DATA` | Hover `ldrb` and `strb` |
| [sort](sort/sort.s) | Insertion sort: nested loops, signed comparisons, `[x0, x2, lsl #3]` indexing | Hover `b.le`; references on `sort` |
| [jumptable](jumptable/switch.s) | A `switch` as a jump table of offsets and `br`; strings kept near code for `adr` | Labels named `s0` to `s6` look like registers: `adr` reads them as labels, a branch wouldn't |
| [bits](bits/bits.s) | `movz`/`movk` to build a constant, `clz`, `rbit`, and popcount with NEON `cnt` and `addv` | Hover `cnt` and `addv`; add `ubfx x1, x0, #60, #8` to see clang's bitfield check |
| [atomics](atomics/atomics.s) | LSE atomics: `ldaddal` and compare-and-swap (`casal`) | Hover shows they need `lse`; add `.arch armv8-a` at the top to see the error when it's off |
| [c-interop](c-interop/dot.s) | C calling assembly and assembly calling C: arguments in x0 to x2, a double result in d0, d8 kept across calls | `gd` on `_report` jumps to `report` in [main.c](c-interop/main.c); `K` shows its C signature |
| [echo](echo/echo.s) | stdin to stdout with the `read` and `write` syscalls; Darwin's carry flag on error | Inlay hints name `read` and `write`; try `echo hi \| ./build/echo` |
| [mistakes](mistakes/mistakes.s) | Doesn't build: one of each problem armls catches, labeled | `]d` through them; `gra` on the ones marked quick fix |

Things these examples ran into while being written, which armls now explains:

- `limit = 15` below a `cmp x19, #limit` isn't a number to clang yet; it's a symbol, and `cmp` can't take one. The error says to move the definition up.
- A string without a zero byte (`.ascii`) in `__TEXT,__cstring` gets merged with its neighbors by the linker, garbling output. Strings with explicit lengths go in `__TEXT,__const`.

The tests in `crates/cli/tests/examples.rs` keep these honest: the valid examples must lint clean and build and print what's expected, and `mistakes.s` must keep showing each problem it lists.

## Linux

[linux/](linux/) has programs for AArch64 Linux. Its `armtool.toml` sets `platform = "linux"`, so armls reads them with Linux's conventions: `;` separates statements, addresses use `:lo12:`, and system calls take their number in x8. Build and run them on an Arm Linux machine with `make -C linux run`; on a Mac, `make -C linux check` assembles them for Linux. CI runs them on GitHub's Arm Linux runner.

| Program | Shows | Try in the editor |
|---|---|---|
| [hello](linux/hello.s) | Linux system calls with no C library: `write` and `exit` through `svc #0`, `_start` as the entry point | Inlay hints show `syscall write`; hover `x8` and `x18` for their roles on Linux |
| [printf](linux/printf.s) | Calling `printf`: on Linux variadic arguments go in registers, unlike Darwin | Type `adrp x0, ` and completion offers labels without `@PAGE`; after `add x0, x0, ` it offers `:lo12:` |
| [atomics](linux/atomics.s) | LSE atomics need `.arch armv8.1-a` on Linux, whose default CPU is generic Armv8-A | Delete the `.arch` line to see the error |
| [count](linux/count.S) | A `.S` file preprocessed for Linux: `__linux__` is defined | The `#error` stays quiet; set `platform = "darwin"` in armtool.toml and it fires |
