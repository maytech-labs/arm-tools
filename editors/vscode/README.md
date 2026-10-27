# AArch64 Assembly for VS Code

Errors as you type in clang's words, operand checking, lint rules for Apple's arm64 ABI, hover, completion, navigation, rename, formatting, and quick fixes, from [armls](../../README.md).

## Install

1. Install armls: `make install` from the repo root (puts it in `/usr/local/bin`).
2. Build the extension: `npm ci && npm run compile && npm run package` in this directory.
3. In VS Code: Extensions, `...`, Install from VSIX, and pick `arm-tools-0.0.1.vsix`.

If armls isn't on your PATH, set `armTools.serverPath` in settings.
