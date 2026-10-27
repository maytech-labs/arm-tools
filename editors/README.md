# Editor setup

`armls` speaks LSP over stdin and stdout, so any editor with an LSP client can use it. Install it first:

```sh
make install                    # into /usr/local/bin
make install PREFIX=~/.local    # somewhere else
```

| Editor | Setup |
|---|---|
| Neovim 0.11+ | [neovim/](neovim/README.md) |
| Helix | Merge [helix/languages.toml](helix/languages.toml) into `~/.config/helix/languages.toml` |
| VS Code | [vscode/](vscode/README.md): build the `.vsix` and install it |
