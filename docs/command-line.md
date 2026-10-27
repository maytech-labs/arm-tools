# The command line

```
armtool check [--json] [--fix] <path>...
armtool lint [--json] [--fix] [--config <file>] <path>...
armtool fmt [--check] <path>...
armtool rules
armtool explain <rule>
```

- **`check`** reports syntax, symbol, and operand errors, as clang would. Nothing it reports assembles.
- **`lint`** is `check` plus the [lint rules](lint.md). It reads the nearest `armtool.toml` above each file, or `--config`.
- **`--fix`** applies the quick fixes findings suggest, in place, and reports what's left.
- **`--json`** prints one JSON object per finding, for other tools.
- **`fmt`** formats files in place; `--check` lists the ones that need it and changes nothing.
- **`rules`** lists every rule with its default level; **`explain`** prints one rule's explanation.

Paths can be files or directories, which are searched for `.s` and `.S` files. `.S` files go through clang's C preprocessor first, for the platform.

Exit codes: 0 when clean, 1 when errors or warnings were found, 2 for a usage or I/O problem. Hints don't fail a run.

In CI:

```sh
armtool lint src/ && armtool fmt --check src/
```
