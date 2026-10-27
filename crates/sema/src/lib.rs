//! The semantic model over the syntax tree: symbols, references, block
//! structure, and the errors clang reports for them. See docs/design.md#sema.

mod analysis;
mod diagnostic;
mod line_index;
mod operands;
mod suggest;

pub use analysis::{Analysis, Instruction, Item, Reference, Symbol, SymbolKind};
pub use diagnostic::{Diagnostic, Severity};
pub use line_index::{Encoding, LineCol, LineIndex};
pub use suggest::suggest;
