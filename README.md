# arm-tools

Tools for writing AArch64 assembly on macOS and Linux: a language server, a linter, and a formatter, built on one parser and one model of the instruction set.

> **Status: pre-release.** The language server is feature complete for its first release, and `armtool` works: errors as you type in clang's words, operand checking, 16 lint rules, and everything below. See the [roadmap](docs/roadmap.md).

Apple silicon comes first. By default it targets the assembly Apple's toolchain accepts: `clang -c -arch arm64`, Mach-O, the Darwin ABI. That means `_`-prefixed symbols, `@PAGE` and `@PAGEOFF`, `;` comments, `%%` separators, and x18 reserved for the OS.

It also handles AArch64 Linux, as `clang --target=aarch64-linux-gnu` and GNU tools accept it: ELF sections and directives, `:lo12:` and `:got:` relocations, `;` as a statement separator, GNU operator precedence, syscalls in x8 with `svc #0`, and the generic CPU's features. Set it per project in `armtool.toml`, or let a file's own syntax say which it is:

```toml
[target]
platform = "linux"
```

| Tool | What it does |
|---|---|
| `armls` | Language server: errors and lint findings as you type; hover that says what an instruction does, its forms, and each register's role in Apple's ABI; completion of only what fits each operand; signature help; snippets; go to definition, references, rename, and call hierarchy across files, including into C; inlay hints for constants and syscalls; formatting, quick fixes, folding, document links, and semantic highlighting |
| `armtool lint` | The linter, for the terminal and CI. The same rules show up in your editor through `armls`. |
| `armtool check` | Syntax, symbol, and operand errors without assembling, as clang reports them |
| `armtool fmt` | Formatter: aligns mnemonics, operands, and comments, changing only whitespace |

The linter goes past syntax: ABI rules (callee-saved registers, x18, stack alignment, frame records), `adrp` without its `@PAGEOFF`, stacks left unbalanced on some path, and unreachable code. [docs/lint.md](docs/lint.md) has every rule and how to configure them.

## Install

With Homebrew, once the repo is public (builds the latest `main`):

```sh
brew tap trammel-may/arm-tools https://github.com/trammel-may/arm-tools
brew install --HEAD arm-tools
```

From a clone:

```sh
make install                    # armls and armtool into /usr/local/bin
make install PREFIX=~/.local    # somewhere else
```

Then set up your editor: [editors/](editors/README.md) has Neovim and Helix configs.

## Build

```sh
cargo build --release
cargo xtask check        # fmt, clippy, tests: what CI runs
cargo xtask nvim-smoke   # armls inside headless Neovim
```

Needs Rust 1.98 (pinned in `rust-toolchain.toml`). The oracle tests need Xcode's clang and run only on macOS.

## Examples

[examples/](examples/README.md) has small programs that build and run, a tour of what to try in the editor, and a file of deliberate mistakes.

## Docs

- [Design](docs/design.md): architecture and how each part works
- [Roadmap](docs/roadmap.md): milestones and what's done
- [Decisions](docs/README.md#decisions): every decision, as an ADR
- [AGENTS.md](AGENTS.md): how to work in this repo

## License

[MIT](LICENSE). Not affiliated with Arm Limited.
