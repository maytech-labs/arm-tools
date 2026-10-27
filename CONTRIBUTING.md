# Contributing

Thanks for helping. Bug reports, especially ones where armtool and clang disagree, are as valuable as code.

## Reporting a problem

[Open an issue](https://github.com/maytech-labs/arm-tools/issues/new/choose). The most useful reports include:

- The smallest assembly that shows it, as text, not a screenshot.
- What armtool or armls said, and what you expected.
- Whether clang accepts it: `clang -c -arch arm64 file.s` on macOS, or `clang -c --target=aarch64-linux-gnu file.s` for Linux. Clang is the source of truth: if armtool reports an error clang doesn't, that's a bug in armtool.
- `armtool --version`, `clang --version`, and your platform.

Security problems go through [SECURITY.md](SECURITY.md), not public issues.

## Building and testing

You need Rust (the version in `rust-toolchain.toml` installs itself) and, for the clang oracle tests, macOS with Xcode's command line tools.

```sh
cargo build
cargo xtask check          # format, clippy, and every test: what CI runs
cargo xtask nvim-smoke     # armls in headless Neovim, if you changed the server
```

To see how a line parses: `echo 'ldr x0, [x1, #8]!' | cargo run -q -p arm-syntax --example tree`.

## Making a change

1. Open an issue first for anything bigger than a fix, so we can agree on the approach.
2. Write the test first where you can. Every new error armtool reports needs a fixture that clang rejects too.
3. Keep `cargo xtask check` green, and update the docs and `CHANGELOG.md` in the same change.
4. Record design decisions as an ADR in [docs/decisions/](docs/decisions/).
5. Keep commits small, with [Conventional Commits](https://www.conventionalcommits.org/) subjects: `fix(lint): ...`.

[AGENTS.md](AGENTS.md) has the full rules: how the crates fit together, the testing layers, the clang survey, and the writing style for docs and diagnostics. Read it before a larger change.

## License

By contributing, you agree that your contributions are licensed under the project's [MIT license](LICENSE), and for `crates/isa`, under its MIT and Apache-2.0 WITH LLVM-exception terms.
