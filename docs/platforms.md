# Platforms

AArch64 assembly is written two ways, and armtool knows both ([0038](decisions/0038-linux.md)):

| | Darwin (default) | Linux |
|---|---|---|
| Assembled by | `clang -arch arm64`, Mach-O | `clang --target=aarch64-linux-gnu` or GNU as, ELF |
| `;` | Starts a comment | Separates statements |
| Address of a symbol | `adrp x0, _sym@PAGE` and `add x0, x0, _sym@PAGEOFF` | `adrp x0, sym` and `add x0, x0, :lo12:sym` |
| Through the GOT | `@GOTPAGE`, `@GOTPAGEOFF` | `:got:`, `:got_lo12:` |
| C names | `_printf` | `printf` |
| System calls | Number in x16, `svc #0x80`; errors set the carry flag | Number in x8, `svc #0`; errors come back as -errno |
| x18 | Reserved by the OS | Free to use |
| Frame pointer | Required | Optional |
| Default CPU | apple-m1 | Generic Armv8-A: LSE, CRC, and AES need `.arch` |
| Operator precedence | Apple's: `1 << 2 + 1` is 8 | GNU's: `1 << 2 + 1` is 5 |
| Directives | `.subsections_via_symbols`, `.zerofill`, `.loh` | `.type`, `.size`, `.hidden`, `.weak`, `.note.GNU-stack` |

## Choosing

Each file is read as one or the other, the first that applies:

1. `[target] platform = "linux"` (or `"darwin"`) in the nearest `armtool.toml`.
2. A comment `armtool: platform=linux` in the file.
3. What the file gives away: `@PAGE`, `__TEXT`, or `.subsections_via_symbols` mean Darwin; `:lo12:`, `.type`, or `.section .text` mean Linux. Signs of both, or neither, decide nothing.
4. Darwin.

The default is Darwin rather than the machine you're on, so a project lints the same on a Mac and in Linux CI. A Linux project should say so in `armtool.toml`: a small file with no Linux-only syntax would otherwise be read as Darwin.

Using one platform's syntax in the other's file is an error that says which platform it belongs to.

## What differs in the linter

- `abi/x18-reserved` and `abi/frame-pointer` are Darwin's.
- `addr/adrp-pageoff` and `addr/variant-mismatch` pair `adrp` with `:lo12:` and the ELF GOT and TLS specifiers on Linux.
- `abi/syscall-number` wants x16 on Darwin and x8 on Linux.
- `elf/gnu-stack` and `elf/function-symbol` are Linux's.
