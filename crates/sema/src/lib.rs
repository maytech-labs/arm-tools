//! The semantic model over the syntax tree: symbols, references, block
//! structure, and the errors clang reports for them. See docs/design.md#sema.

mod analysis;
mod diagnostic;
mod line_index;
mod macros;
mod operands;
mod resolver;
mod suggest;

pub use analysis::{Analysis, Instruction, Item, Origin, Reference, Resolver, Symbol, SymbolKind};
pub use diagnostic::{Diagnostic, Fix, Severity};
pub use line_index::{Encoding, LineCol, LineIndex};
pub use resolver::{FsResolver, canonical, normalize};
pub use suggest::suggest;
