# arm-isa's license

`arm-isa` has two parts under two licenses:

- **Our code**, everything but `src/generated.rs`, is licensed under the Reciprocal Ethical AI License (REAL) 1.0, as the rest of arm-tools is: see [LICENSE.md](../../LICENSE.md) at the repository root.
- **`src/generated.rs`**, the instruction, register, and feature tables generated from LLVM 21.1.8, and the operand predicates ported from LLVM's AArch64 assembler, are derived from LLVM and remain under the Apache License 2.0 with LLVM Exceptions: see [LICENSE-LLVM.txt](LICENSE-LLVM.txt). Its terms apply to that LLVM-derived material whatever license covers the rest.

The crate as a whole is distributed under both: REAL 1.0, and Apache-2.0 WITH LLVM-exception for the LLVM-derived parts.
