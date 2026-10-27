# 0033. Call hierarchy from the control-flow graph's functions

- **Status:** Accepted
- **Date:** 2026-10-27

## Context

Editors can show who calls a function and what it calls. Assembly has no function syntax, so something has to decide what a function and a call are.

## Decision

- **Functions** are the control-flow graph's: labels that are exported or called with `bl` in the file ([0018](0018-lint.md)). `arm_lint::call_graph` lists them with their extent, label to last instruction.
- **Calls** are `bl` to a name, and `b` (unconditional) to a function or to a name the file doesn't define, which is a tail call. Conditional branches and branches to labels inside the function aren't calls. Indirect calls (`blr`) have no name and aren't shown.
- The workspace index ([0029](0029-workspace-index.md)) keeps each file's call graph. Incoming calls to an exported function come from every file that doesn't define its own function of that name; calls to a file-local function come from its file only.
- Outgoing calls to names the project doesn't define, like `_printf`, still appear, pointing at the call, with the detail "not defined in the project".
- Calls before the first function label have no caller and aren't listed as incoming.

## Consequences

- Call hierarchy, incoming and outgoing, works across separately assembled files.
- Calls from C into assembly aren't seen.
