# Configuration

`armtool.toml` sets rule levels, include paths, the target, and formatting for every file in its directory and below. armls and armtool read the nearest one above each file. Every section is optional.

```toml
[rules]
"abi/x18-reserved" = "error"     # error, warning, info, hint, or off
"style/unused-label" = "off"

[include]
paths = ["include", "../shared"] # for .include, relative to this file

[target]
platform = "linux"               # or "darwin"; else each file's own syntax decides
cpu = "apple-m4"                 # like -mcpu
arch = "armv8.6-a"               # like -march, added to the CPU's features
extensions = ["sve", "nolse"]    # like +sve and +nolse

[format]
indent = 4                       # column for mnemonics and directives
mnemonic_width = 8               # width the mnemonic is padded to
comment_column = 40              # where trailing comments start
max_blank_lines = 1
```

- **Rules** are listed in [Lint rules](lint.md). An unknown rule ID is an error that suggests the closest one.
- **Includes** are searched in the listed paths, then the project root (this file's directory, or the nearest with `.git`), then the including file's directory.
- **Target**: without a CPU, it's clang's default for the platform, apple-m1 on Darwin and generic Armv8-A on Linux. `.arch`, `.cpu`, and `.arch_extension` in a file change it from there, as they do for clang.
- **Format** settings are used by `armtool fmt` and editor formatting alike.

To silence one finding, put `; armtool: allow(rule-id)` on its line or the line above (`//` on Linux).
