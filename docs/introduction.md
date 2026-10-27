# arm-tools

Tools for writing AArch64 assembly on macOS and Linux, built on one parser and one model of the instruction set, generated from LLVM's:

- **armls**, a language server: errors as you type in clang's words, hover that says what an instruction does and what each register is for, completion of only what fits each operand, signature help, navigation and rename across files (into C too), call hierarchy, inlay hints, formatting, and quick fixes.
- **armtool**, for the terminal and CI: `check`, `lint` with 25 rules, `fmt`, and `explain`.
- **Optional types**: a few comments, and the linter checks pointers, widths, signedness, struct fields, calls, and printf formats.

Apple silicon comes first: the default is the assembly `clang -arch arm64` accepts, Mach-O, and Apple's ABI. AArch64 Linux is a second platform, with its own syntax, directives, and ABI. Clang is the source of truth for both: armtool reports nothing on LLVM's 2,289 AArch64 assembler test files that clang accepts.

Start with [Getting started](getting-started.md).
