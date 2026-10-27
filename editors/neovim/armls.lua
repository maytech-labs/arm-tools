-- armls for Neovim 0.11 and later. Put this file in ~/.config/nvim/plugin/,
-- or paste it into your config.
vim.lsp.config("armls", {
  cmd = { "armls" },
  -- Neovim detects .s and .S files as "asm".
  filetypes = { "asm" },
  root_markers = { "armtool.toml", ".git", "Makefile" },
})
vim.lsp.enable("armls")
