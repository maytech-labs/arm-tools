# Examples

Small programs that build and run on Apple silicon, to read and to try armls on. Build and run them all:

```sh
make run          # from this directory
make fib          # or one at a time, into build/
./build/fib
```

| Program | Shows | Try in the editor |
|---|---|---|
| [hello](hello/hello.s) | Raw Darwin system calls: `write` and `exit` through `svc #0x80` with the number in x16, and `adrp` + `@PAGEOFF` to load an address | Inlay hints show `syscall write`; hover `x16` |
| [sum](sum/sum.s) | Calling `printf`. On Apple silicon, variadic arguments go on the stack, not in x1 | Hover `_printf`'s format, `str`, and `sp`; go to definition on `sum` |
| [fib](fib/fib.s) | Recursion: frame records, and callee-saved x19 and x20 kept across calls | Hover `x19` for its ABI role; delete the `stp x19, x20` to see `abi/callee-saved` |
| [neon](neon/vecadd.s) | NEON: `ld1`, `fadd` on four floats at once, `st1` | Hover `fadd` for its forms; change `.4s` to `.8h` in one operand |
| [macros](macros/main.s) | `.include`, macros with default arguments, `.irp`, `.rept` | Go to definition on `print` jumps to [syscalls.s](macros/syscalls.s); break the macro body and the error appears at the call |
| [preprocessor](preprocessor/strlen.S) | A `.S` file: `#include`, `#define`, a function-like macro, `#if` | Errors map back through the preprocessor; set `VERBOSE` to 0 |
| [linked](linked/main.s) | A program in two files, assembled separately and linked: calls go both ways between them | `gd` on `_print_line` jumps to [text.s](linked/text.s); `grr` lists uses in both files; hover shows where it's defined |
| [mistakes](mistakes/mistakes.s) | Doesn't build: one of each problem armls catches, labeled | `]d` through them; `gra` on the ones marked quick fix |

Things these examples ran into while being written, which armls now explains:

- `limit = 15` below a `cmp x19, #limit` isn't a number to clang yet; it's a symbol, and `cmp` can't take one. The error says to move the definition up.
- A string without a zero byte (`.ascii`) in `__TEXT,__cstring` gets merged with its neighbors by the linker, garbling output. Strings with explicit lengths go in `__TEXT,__const`.

The tests in `crates/cli/tests/examples.rs` keep these honest: the valid examples must lint clean and build and print what's expected, and `mistakes.s` must keep showing each problem it lists.
