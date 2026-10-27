# The editor

What armls does in any editor. Key bindings are Neovim's defaults; other editors have their own.

| Feature | What you see |
|---|---|
| Diagnostics | Clang's errors as you type: unknown instructions, operands out of range, missing CPU features, undefined labels. Lint findings too, each with its rule ID. |
| Hover | For an instruction, what it does and every form it takes, with the CPU features each needs. For a register, its role in the platform's ABI, and its type if [types](types.md) are declared. For a label or constant, where it's defined and its value. |
| Completion | Mnemonics, directives, and macros where a statement starts. In operands, only what fits: condition codes after `csel x0, x1, x2, `, base registers after `[`, labels with `@PAGE` after `adrp`. |
| Signature help | The forms the operands typed so far could begin, with the operand you're on. `Ctrl-s` in insert mode. |
| Snippets | `func`, `main`, `prologue`, `epilogue`, `addr`, `load`, `got`, `syscall`, `write`, `exit`, `loop`, `cstring`, in the platform's conventions. |
| Go to definition | Labels, constants, macros, and register aliases; across `.include`; into other files of the project for global labels; into C files for C functions. |
| References and rename | Within a file, and across the project for global labels. |
| Call hierarchy | Who calls a function and what it calls, across files. |
| Inlay hints | The value of each constant where it's used, and the name of the system call a `mov` sets up. |
| Quick fixes | From diagnostics and lint findings: a misspelled mnemonic, `@PAGEOFF` after `adrp`, a rounded stack adjustment, `sxtw` where a type needs it, and more. |
| Formatting | Aligned mnemonics, operands, and comments, changing only whitespace. |
| Also | Document symbols and workspace symbol search, highlight of a symbol's uses, semantic highlighting, folding, and links for `.include` and `#include`. |

## Platform

armls reads each file as Darwin or Linux assembly: from `[target] platform` in the nearest `armtool.toml`, else a comment `armtool: platform=linux` in the file, else what the file's syntax gives away (`@PAGE` or `:lo12:`), else Darwin. Hover `x18` to see which: on Darwin it's reserved, on Linux it's free. See [Platforms](platforms.md).
