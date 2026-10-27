# arm-tools

Tools for writing AArch64 assembly on macOS: a language server, a linter, and a formatter, built on one parser and one model of the instruction set.

> **Status: pre-release.** The workspace and design are in place; the lexer is being built. See the [roadmap](docs/roadmap.md).

It targets the assembly Apple's toolchain accepts: `clang -c -arch arm64`, Mach-O, the Darwin ABI. That means `_`-prefixed symbols, `@PAGE` and `@PAGEOFF`, `;` comments, `%%` separators, and x18 reserved for the OS.

| Tool | What it does |
|---|---|
| `armls` | Language server: diagnostics, hover docs for instructions and registers, completion, go to definition, references, rename, document symbols, semantic highlighting |
| `armtool lint` | The linter, for the terminal and CI. The same rules show up in your editor through `armls`. |
| `armtool check` | Syntax and operand errors without assembling |
| `armtool fmt` | Formatter (later) |

The linter goes past syntax: ABI rules (callee-saved registers, x18, stack alignment), `adrp` without its `@PAGEOFF`, unbalanced prologues and epilogues, and dead or unreachable code. The [design](docs/design.md#lint) has the full list.

## Build

```sh
cargo build --release
cargo xtask check   # fmt, clippy, tests: what CI runs
```

Needs Rust 1.98 (pinned in `rust-toolchain.toml`). The oracle tests need Xcode's clang and run only on macOS.

## Docs

- [Design](docs/design.md): architecture and how each part works
- [Roadmap](docs/roadmap.md): milestones and what's done
- [Decisions](docs/README.md#decisions): every decision, as an ADR
- [AGENTS.md](AGENTS.md): how to work in this repo

## License

[MIT](LICENSE). Not affiliated with Arm Limited.
