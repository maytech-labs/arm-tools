//! Typed views over the tree. Each type wraps a node of one kind. Accessors
//! return `Option` because the tree can be incomplete while the user types.

use crate::{SyntaxKind, SyntaxNode, SyntaxToken};

macro_rules! ast_node {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(SyntaxNode);

        impl $name {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                (node.kind() == SyntaxKind::$name).then_some(Self(node))
            }

            pub fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

ast_node!(
    /// The whole file.
    SourceFile
);
ast_node!(
    /// `name:`, `"quoted":`, or `1:`.
    Label
);
ast_node!(
    /// A mnemonic and its operands, or a macro call.
    Instruction
);
ast_node!(
    /// A call to a macro defined earlier in the file.
    MacroCall
);
ast_node!(
    /// `.name args`.
    Directive
);
ast_node!(
    /// `name = expr`.
    Assignment
);
ast_node!(
    /// `name .req register`.
    RegisterAlias
);
ast_node!(
    /// A register, symbol, or other name in an operand or expression.
    NameRef
);

/// One statement in a file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Statement {
    Label(Label),
    Instruction(Instruction),
    MacroCall(MacroCall),
    Directive(Directive),
    Assignment(Assignment),
    RegisterAlias(RegisterAlias),
}

impl Statement {
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        Some(match node.kind() {
            SyntaxKind::Label => Statement::Label(Label(node)),
            SyntaxKind::Instruction => Statement::Instruction(Instruction(node)),
            SyntaxKind::MacroCall => Statement::MacroCall(MacroCall(node)),
            SyntaxKind::Directive => Statement::Directive(Directive(node)),
            SyntaxKind::Assignment => Statement::Assignment(Assignment(node)),
            SyntaxKind::RegisterAlias => Statement::RegisterAlias(RegisterAlias(node)),
            _ => return None,
        })
    }

    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Statement::Label(n) => n.syntax(),
            Statement::Instruction(n) => n.syntax(),
            Statement::MacroCall(n) => n.syntax(),
            Statement::Directive(n) => n.syntax(),
            Statement::Assignment(n) => n.syntax(),
            Statement::RegisterAlias(n) => n.syntax(),
        }
    }
}

impl SourceFile {
    pub(crate) fn new(node: SyntaxNode) -> Self {
        Self(node)
    }

    /// Statements in source order. Error nodes between statements are skipped.
    pub fn statements(&self) -> impl Iterator<Item = Statement> + use<> {
        self.0.children().filter_map(Statement::cast)
    }
}

impl Label {
    /// The name: an identifier, a quoted string, or a number for local labels.
    pub fn name(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// True for local numeric labels like `1:`, referenced as `1f` and `1b`.
    pub fn is_local(&self) -> bool {
        self.name().is_some_and(|t| t.kind() == SyntaxKind::Number)
    }
}

impl Instruction {
    pub fn mnemonic(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// Operands in order. An operand the parser couldn't read is an `Error` node.
    /// Empty inside macro and repeat bodies, where operands aren't parsed.
    pub fn operands(&self) -> impl Iterator<Item = SyntaxNode> + use<> {
        child(&self.0, SyntaxKind::OperandList)
            .into_iter()
            .flat_map(|list| list.children())
    }
}

impl MacroCall {
    pub fn name(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// Arguments that parsed as expressions, like [`Directive::args`].
    pub fn args(&self) -> impl Iterator<Item = SyntaxNode> + use<> {
        child(&self.0, SyntaxKind::ArgList)
            .into_iter()
            .flat_map(|list| list.children())
    }
}

impl Directive {
    /// The directive's name, with its leading `.`.
    pub fn name(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// Arguments that parsed as expressions. Raw tokens between them, like the
    /// `=` in `.macro m b=1`, are in `syntax()` but not here.
    pub fn args(&self) -> impl Iterator<Item = SyntaxNode> + use<> {
        child(&self.0, SyntaxKind::ArgList)
            .into_iter()
            .flat_map(|list| list.children())
    }
}

impl Assignment {
    pub fn name(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    pub fn value(&self) -> Option<SyntaxNode> {
        self.0.children().next()
    }
}

impl RegisterAlias {
    pub fn name(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// The register the alias stands for.
    pub fn target(&self) -> Option<SyntaxNode> {
        self.0.children().next()
    }
}

impl NameRef {
    pub fn name(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// The variant after `@`, like `PAGEOFF` in `_g@PAGEOFF`.
    pub fn variant(&self) -> Option<SyntaxToken> {
        child(&self.0, SyntaxKind::Variant)?
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| t.kind() == SyntaxKind::Ident)
    }
}

fn first_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
}

fn child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|n| n.kind() == kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn text(token: Option<SyntaxToken>) -> Option<String> {
        token.map(|t| t.text().to_string())
    }

    #[test]
    fn statements_in_order() {
        let parse = parse("_f: .globl _f\nmov x0, #1\na = 2\n");
        let kinds: Vec<_> = parse
            .tree()
            .statements()
            .map(|s| s.syntax().kind())
            .collect();
        assert_eq!(
            kinds,
            [
                SyntaxKind::Label,
                SyntaxKind::Directive,
                SyntaxKind::Instruction,
                SyntaxKind::Assignment
            ]
        );
    }

    #[test]
    fn accessors() {
        let parse = parse("1: add x0, x0, _g@PAGEOFF\n.p2align 2\n");
        let mut statements = parse.tree().statements();

        let Some(Statement::Label(label)) = statements.next() else {
            panic!("expected a label");
        };
        assert_eq!(text(label.name()).as_deref(), Some("1"));
        assert!(label.is_local());

        let Some(Statement::Instruction(insn)) = statements.next() else {
            panic!("expected an instruction");
        };
        assert_eq!(text(insn.mnemonic()).as_deref(), Some("add"));
        let operands: Vec<_> = insn.operands().collect();
        assert_eq!(operands.len(), 3);
        let symbol = NameRef::cast(operands[2].clone()).expect("a name");
        assert_eq!(text(symbol.name()).as_deref(), Some("_g"));
        assert_eq!(text(symbol.variant()).as_deref(), Some("PAGEOFF"));

        let Some(Statement::Directive(directive)) = statements.next() else {
            panic!("expected a directive");
        };
        assert_eq!(text(directive.name()).as_deref(), Some(".p2align"));
        assert_eq!(directive.args().count(), 1);
    }
}
