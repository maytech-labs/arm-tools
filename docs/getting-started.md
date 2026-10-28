# Getting started

## Install

Prebuilt binaries for macOS (Apple silicon) and Linux (x86_64 and arm64) are on the [releases page](https://github.com/maytech-labs/arm-tools/releases): unpack one and put `armls` and `armtool` on your `PATH`.

With Homebrew, on Apple silicon, from the [maytech-labs tap](https://github.com/maytech-labs/homebrew-tap):

```sh
brew tap maytech-labs/tap
brew trust maytech-labs/tap
brew install arm-tools
```

That installs the latest release's binaries. `brew install --HEAD arm-tools` builds `main` instead, which needs Homebrew's Rust and LLVM and takes a while.

From a clone, with Rust installed:

```sh
make install                    # armls and armtool into /usr/local/bin
make install PREFIX=~/.local    # somewhere else
```

## Set up your editor

armls speaks the Language Server Protocol over stdin and stdout, so any editor with an LSP client can use it. Ready-made setups:

| Editor | Setup |
|---|---|
| Neovim 0.11+ | [editors/neovim](https://github.com/maytech-labs/arm-tools/blob/main/editors/neovim/README.md) |
| Helix | [editors/helix/languages.toml](https://github.com/maytech-labs/arm-tools/blob/main/editors/helix/languages.toml) |
| VS Code | [editors/vscode](https://github.com/maytech-labs/arm-tools/blob/main/editors/vscode/README.md) |

Open a `.s` or `.S` file and errors appear as you type. See [The editor](editor.md) for everything armls does.

## Check from the command line

```sh
armtool check src/          # errors, as clang would report them
armtool lint src/           # errors and lint findings
armtool lint --fix src/     # apply the quick fixes
armtool fmt src/            # align mnemonics, operands, and comments
```

See [The command line](command-line.md).

## Linux projects

Put an `armtool.toml` at the project root:

```toml
[target]
platform = "linux"
```

See [Platforms](platforms.md).

## Try it

The [examples](https://github.com/maytech-labs/arm-tools/blob/main/examples/README.md) are small programs that build and run, each with things to try in the editor, and `mistakes.s`, which shows one of each problem armtool finds.
