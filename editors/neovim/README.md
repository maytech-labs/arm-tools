# Neovim

Neovim 0.11 or later. Install `armls` (`make install` from the repo root), then add [armls.lua](armls.lua) to your config: copy it into `~/.config/nvim/plugin/`, or paste its lines into your `init.lua`.

Open a `.s` file and check it's attached with `:checkhealth vim.lsp`.

What you get, with the default keymaps of Neovim 0.11:

| Feature | How |
|---|---|
| Errors as you type | Shown inline; `]d` and `[d` move between them |
| Hover: instruction forms, register roles, symbol definitions | `K` |
| Go to definition | `Ctrl-]` or `gd` if you map it |
| References | `grr` |
| Rename | `grn` |
| Outline | `gO` |
| Completion | `Ctrl-x Ctrl-o`, or your completion plugin |
| Semantic highlighting | Automatic |

`editors/neovim/smoke.lua` is the headless test `cargo xtask nvim-smoke` runs; it's not for your config.
