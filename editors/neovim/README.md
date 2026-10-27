# Neovim

Neovim 0.11 or later. Install `armls` (`make install` from the repo root), then add [armls.lua](armls.lua) to your config: copy it into `~/.config/nvim/plugin/`, or paste its lines into your `init.lua`.

Open a `.s` file and check it's attached with `:checkhealth vim.lsp`.

What you get, with the default keymaps of Neovim 0.11:

| Feature | How |
|---|---|
| Errors as you type | Shown inline; `]d` and `[d` move between them |
| Hover: instruction forms, register roles, symbol definitions | `K` |
| Go to definition, also into other files of the project | `Ctrl-]` or `gd` if you map it |
| Search global labels across the project | `:lua vim.lsp.buf.workspace_symbol()` |
| References | `grr` |
| Rename | `grn` |
| Outline | `gO` |
| Completion | `Ctrl-x Ctrl-o`, or your completion plugin |
| Format | `:lua vim.lsp.buf.format()`, or on save with a `BufWritePre` autocommand |
| Quick fixes | `gra` (code actions) on a diagnostic |
| Folding | `:set foldmethod=expr foldexpr=v:lua.vim.lsp.foldexpr()` |
| Inlay hints (constant values, syscall names) | `:lua vim.lsp.inlay_hint.enable()` |
| Semantic highlighting | Automatic |

`editors/neovim/smoke.lua` is the headless test `cargo xtask nvim-smoke` runs; it's not for your config.
