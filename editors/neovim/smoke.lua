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

local function sync(method, params)
  local responses = vim.lsp.buf_request_sync(buf, method, params, 5000) or {}
  for _, response in pairs(responses) do
    if response.error then
      fail(method .. " failed: " .. vim.inspect(response.error))
    end
    return response.result
  end
  fail(method .. " got no response")
end

local function document()
  return { textDocument = vim.lsp.util.make_text_document_params(buf) }
end

-- smoke.s, 1-based rows:
--  1 .include "defs.s"     5 frob x0           9 ldp x29, x30, [sp], #16
--  2 .globl _main          6 mov x0, #LIMIT   10 ret (trailing spaces)
--  3 _main:                7 bl _helper       11 (blank)
--  4 stp ...               8 csel ..., eq     12 ; end

local hover = request("textDocument/hover", 4, 5)
if not (hover and hover.contents.value:find("%*%*stp%*%*") and hover.contents.value:find("Store a pair")) then
  fail("hover on stp: " .. vim.inspect(hover))
end
pass("hover on stp describes it and lists its forms")

local definition = request("textDocument/definition", 7, 8)
local location = definition and (definition.range and definition or definition[1])
if not (location and location.uri:find("util%.s$") and location.range.start.line == 1) then
  fail("definition of _helper: " .. vim.inspect(definition))
end
pass("go to definition jumps into util.s")

local function labels(items)
  local out = {}
  for _, item in ipairs(items and (items.items or items) or {}) do
    out[item.label] = item
  end
  return out
end

local conditions = labels(request("textDocument/completion", 8, 21))
if not conditions["eq"] or conditions["x0"] then
  fail("completion after csel's registers: " .. vim.inspect(vim.tbl_keys(conditions)))
end
pass("completion after csel x0, x1, x2, offers condition codes only")

local starts = labels(request("textDocument/completion", 11, 0))
if not (starts["ldp"] and starts["func"] and starts["func"].insertTextFormat == 2) then
  fail("completion at a statement start: no ldp or func snippet")
end
pass("completion at a statement start offers mnemonics and snippets")

local help = request("textDocument/signatureHelp", 9, 19)
local active
for _, signature in ipairs(help and help.signatures or {}) do
  if signature.label:find("%]!$") == nil and signature.label:find("%], ") then
    active = signature.activeParameter
  end
end
if active ~= 2 then
  fail("signature help in ldp's address: " .. vim.inspect(help))
end
pass("signature help marks ldp's third operand")

local items = request("textDocument/prepareCallHierarchy", 3, 1)
local outgoing = items and items[1] and sync("callHierarchy/outgoingCalls", { item = items[1] })
if not (outgoing and outgoing[1] and outgoing[1].to.name == "_helper") then
  fail("outgoing calls of _main: " .. vim.inspect(outgoing))
end
pass("call hierarchy: _main calls _helper")

local whole = document()
whole.range = { start = { line = 0, character = 0 }, ["end"] = { line = 12, character = 0 } }
local hints = sync("textDocument/inlayHint", whole) or {}
local hinted = false
for _, hint in ipairs(hints) do
  local label = type(hint.label) == "string" and hint.label or ""
  hinted = hinted or label:find("= 4") ~= nil
end
if not hinted then
  fail("inlay hint for LIMIT: " .. vim.inspect(hints))
end
pass("inlay hint shows LIMIT's value from defs.s")

local links = sync("textDocument/documentLink", document()) or {}
if not (links[1] and links[1].target:find("defs%.s$")) then
  fail("document link for .include: " .. vim.inspect(links))
end
pass(".include links to defs.s")

local options = document()
options.options = { tabSize = 4, insertSpaces = true }
local edits = sync("textDocument/formatting", options)
if not edits or #edits == 0 then
  fail("formatting changed nothing in a file with trailing spaces")
end
vim.lsp.util.apply_text_edits(edits, buf, client.offset_encoding)
for i, line in ipairs(vim.api.nvim_buf_get_lines(buf, 0, -1, false)) do
  if line:find("%s$") then
    fail("line " .. i .. " still ends in whitespace after formatting")
  end
end
local stable = vim.wait(1000, function()
  local again = vim.lsp.buf_request_sync(buf, "textDocument/formatting", options, 5000) or {}
  for _, response in pairs(again) do
    return response.result == nil or #response.result == 0
  end
  return false
end)
if not stable then
  fail("formatting a formatted file still changes it")
end
pass("formatting applies, and formatting again changes nothing")

-- Rename last: it edits util.s too, through Neovim's own workspace edit.
local after_format = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
local call_row
for i, line in ipairs(after_format) do
  if line:find("bl%s+_helper") then
    call_row = i
  end
end
local column = after_format[call_row]:find("_helper") - 1
vim.api.nvim_win_set_cursor(0, { call_row, column })
local params = vim.lsp.util.make_position_params(0, client.offset_encoding)
params.newName = "_assist"
local edit = sync("textDocument/rename", params)
vim.lsp.util.apply_workspace_edit(edit, client.offset_encoding)
local util = vim.fn.bufnr(vim.fn.fnamemodify(file, ":h") .. "/util.s")
local util_text = table.concat(vim.api.nvim_buf_get_lines(util, 0, -1, false), "\n")
local here = table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), "\n")
if not (util_text:find("_assist:") and util_text:find(".globl _assist", 1, true) and here:find("bl%s+_assist")) then
  fail("rename across files: util.s is " .. vim.inspect(util_text))
end
pass("rename _helper changes smoke.s and util.s")

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
