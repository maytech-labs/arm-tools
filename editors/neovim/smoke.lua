-- End-to-end check, run by `cargo xtask nvim-smoke`: start armls through
-- Neovim's own LSP client, headless, and check what an editor user sees.
local armls = assert(os.getenv("ARMLS"), "set ARMLS to the armls binary")
local file = assert(os.getenv("SMOKE_FILE"), "set SMOKE_FILE to a .s file")

local function fail(message)
  io.stderr:write("nvim-smoke: " .. message .. "\n")
  vim.cmd("cquit 1")
end

local function pass(message)
  io.stdout:write("ok: " .. message .. "\n")
end

vim.lsp.config("armls", { cmd = { armls }, filetypes = { "asm" }, root_markers = { ".git" } })
vim.lsp.enable("armls")
vim.cmd("edit " .. vim.fn.fnameescape(file))
local buf = vim.api.nvim_get_current_buf()

if vim.bo[buf].filetype ~= "asm" then
  fail("Neovim detected filetype '" .. vim.bo[buf].filetype .. "', not asm")
end
pass("filetype is asm")

local attached = vim.wait(5000, function()
  return #vim.lsp.get_clients({ bufnr = buf, name = "armls" }) > 0
end)
if not attached then
  fail("armls didn't attach")
end
local client = vim.lsp.get_clients({ bufnr = buf, name = "armls" })[1]
pass("armls attached, position encoding " .. client.offset_encoding)

vim.wait(5000, function()
  return #vim.diagnostic.get(buf) > 0
end)
local diagnostics = vim.diagnostic.get(buf)
if #diagnostics ~= 1 or not diagnostics[1].message:find("unknown instruction 'frob'") then
  fail("expected one unknown-instruction diagnostic, got " .. vim.inspect(diagnostics))
end
pass("diagnostic on line " .. (diagnostics[1].lnum + 1) .. ": " .. diagnostics[1].message)

local function request(method, row, col)
  vim.api.nvim_win_set_cursor(0, { row, col })
  local params = vim.lsp.util.make_position_params(0, client.offset_encoding)
  local responses = vim.lsp.buf_request_sync(buf, method, params, 5000) or {}
  for _, response in pairs(responses) do
    if response.error then
      fail(method .. " failed: " .. vim.inspect(response.error))
    end
    return response.result
  end
  fail(method .. " got no response")
end

local hover = request("textDocument/hover", 2, 5)
if not (hover and hover.contents.value:find("%*%*stp%*%*")) then
  fail("hover on stp: " .. vim.inspect(hover))
end
pass("hover on stp lists its forms")

local definition = request("textDocument/definition", 4, 7)
local location = definition and (definition.range and definition or definition[1])
if not (location and location.range.start.line == 0) then
  fail("definition of _main: " .. vim.inspect(definition))
end
pass("go to definition jumps to line 1")

local tokens = vim.lsp.buf_request_sync(buf, "textDocument/semanticTokens/full",
  { textDocument = vim.lsp.util.make_text_document_params(buf) }, 5000) or {}
local data
for _, response in pairs(tokens) do
  data = response.result and response.result.data
end
if not data or #data == 0 or #data % 5 ~= 0 then
  fail("semantic tokens: " .. vim.inspect(tokens))
end
pass(#data / 5 .. " semantic tokens")

vim.cmd("qall!")
