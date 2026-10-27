//! The semantic model of one file: symbols, references to them, block
//! structure, and the errors clang reports for them. Each check here was
//! probed against clang; see docs/decisions/0015-sema-file-model.md.

use std::collections::HashMap;
use std::ops::Range;

use arm_isa::{CONDITION_CODES, Mnemonic};
use arm_syntax::ast::{self, Statement};
use arm_syntax::{NodeOrToken, Parse, SyntaxKind, SyntaxNode, SyntaxToken};

use crate::diagnostic::{Diagnostic, Severity};
use crate::operands::{self, Scope, Value};
use crate::suggest::suggest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// `_main:`.
    Label,
    /// `1:`, referenced as `1f` or `1b`. Many can share a name.
    LocalLabel,
    /// `.set`, `.equ`, `.equiv`, or `name = expr`.
    Constant,
    /// `name .req x9`, valid until `.unreq name`.
    RegisterAlias,
    /// `.macro name`.
    Macro,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// As written, without quotes. Local labels are their number: `1`.
    pub name: String,
    pub kind: SymbolKind,
    /// The name where it's defined.
    pub range: Range<usize>,
    /// The whole defining statement.
    pub statement: Range<usize>,
    /// Named by `.globl` or `.global`.
    pub exported: bool,
    /// For register aliases: where `.unreq` ends it.
    pub scope_end: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// As written, without quotes, `@` variant, or `f`/`b` suffix.
    pub name: String,
    pub range: Range<usize>,
    /// The symbol it resolves to. `None` for names defined elsewhere, like
    /// `_printf`, and for registers and other operand keywords.
    pub symbol: Option<usize>,
}

/// An instruction, with its operands in clang's model.
#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    /// The whole statement.
    pub range: Range<usize>,
    pub mnemonic: Mnemonic,
    pub mnemonic_range: Range<usize>,
    /// None when the operands can't be judged before assembly: literal pool
    /// loads, SME arrays, parse errors.
    pub operands: Option<Vec<arm_isa::Operand>>,
    /// The source range of each operand.
    pub operand_ranges: Vec<Range<usize>>,
}

/// The file's contents in order, outside macro and repeat bodies: what lint
/// rules and control flow work from.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A label: an index into [`Analysis::symbols`].
    Label(usize),
    Instruction(Instruction),
    /// A call to a macro: code we can't see until it's expanded.
    MacroCall(Range<usize>),
    /// A switch to another section. `text` is true for code sections.
    Section {
        range: Range<usize>,
        text: bool,
    },
    /// Data: `.byte`, `.quad`, `.asciz`, `.space`, and the like.
    Data(Range<usize>),
}

/// One file, analyzed.
#[derive(Debug, Clone)]
pub struct Analysis {
    text: String,
    parse: Parse,
    line_index: crate::LineIndex,
    symbols: Vec<Symbol>,
    references: Vec<Reference>,
    diagnostics: Vec<Diagnostic>,
    items: Vec<Item>,
}

impl Analysis {
    pub fn new(text: &str) -> Analysis {
        let parse = arm_syntax::parse(text);
        let mut builder = Builder {
            symbols: Vec::new(),
            items: Vec::new(),
            pending: Vec::new(),
            local_refs: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            has_include: false,
            exported: Vec::new(),
            skip_local_refs: false,
            constants: HashMap::new(),
            aliases: HashMap::new(),
            later_macros: macro_names(&parse),
        };
        for error in parse.errors() {
            builder.diagnostics.push(Diagnostic::error(
                error.range.clone(),
                "syntax",
                error.message.clone(),
            ));
        }
        for statement in parse.tree().statements() {
            builder.statement(&statement);
        }
        builder.close_blocks();
        let references = builder.resolve();
        let mut diagnostics = std::mem::take(&mut builder.diagnostics);
        diagnostics.sort_by_key(|d| (d.range.start, d.range.end));
        Analysis {
            text: text.to_string(),
            line_index: crate::LineIndex::new(text),
            parse,
            symbols: builder.symbols,
            references,
            diagnostics,
            items: builder.items,
        }
    }

    /// Labels, instructions, sections, data, and macro calls, in order,
    /// outside macro and repeat bodies.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn parse(&self) -> &Parse {
        &self.parse
    }

    pub fn line_index(&self) -> &crate::LineIndex {
        &self.line_index
    }

    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    pub fn references(&self) -> &[Reference] {
        &self.references
    }

    /// Syntax and semantic errors, in source order.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The symbol defined or referenced at a byte offset. A cursor right after
    /// a name counts, so the end of `_main` finds it.
    pub fn symbol_at(&self, offset: usize) -> Option<usize> {
        let touches = |r: &Range<usize>| r.start <= offset && offset <= r.end;
        self.symbols
            .iter()
            .position(|s| touches(&s.range))
            .or_else(|| {
                self.references
                    .iter()
                    .find(|r| touches(&r.range))
                    .and_then(|r| r.symbol)
            })
    }

    /// The reference at a byte offset, resolved or not.
    pub fn reference_at(&self, offset: usize) -> Option<&Reference> {
        self.references
            .iter()
            .find(|r| r.range.start <= offset && offset <= r.range.end)
    }

    /// Every reference to a symbol, in source order.
    pub fn references_to(&self, symbol: usize) -> impl Iterator<Item = &Reference> {
        self.references
            .iter()
            .filter(move |r| r.symbol == Some(symbol))
    }
}

/// Directives Linux code uses that clang rejects on macOS.
const ELF_ONLY: &[&str] = &[
    ".type",
    ".size",
    ".hidden",
    ".local",
    ".protected",
    ".internal",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Macro,
    Repeat,
    Conditional,
}

struct Block {
    kind: BlockKind,
    directive: String,
    range: Range<usize>,
}

/// A name reference waiting for every definition to be known.
struct Pending {
    name: String,
    range: Range<usize>,
    /// Resolve only to a macro: the name of a macro call.
    macro_only: bool,
}

struct Builder {
    symbols: Vec<Symbol>,
    items: Vec<Item>,
    pending: Vec<Pending>,
    /// `1f` and `1b`: (number, range, forward).
    local_refs: Vec<(String, Range<usize>, bool)>,
    diagnostics: Vec<Diagnostic>,
    blocks: Vec<Block>,
    has_include: bool,
    /// Names given to `.globl` or `.global`.
    exported: Vec<String>,
    /// Inside a macro or repeat body, where `1f` resolves per expansion.
    skip_local_refs: bool,
    /// Constants defined so far with known values. Clang folds a constant
    /// into an operand only if it's defined above the use.
    constants: HashMap<String, i64>,
    /// Register aliases in scope: lowercase alias to register text.
    aliases: HashMap<String, String>,
    /// Every macro defined anywhere in the file, for a better message when one
    /// is called before its definition.
    later_macros: Vec<String>,
}

impl Builder {
    fn in_body(&self) -> bool {
        self.blocks.iter().any(|b| b.kind != BlockKind::Conditional)
    }

    fn statement(&mut self, statement: &Statement) {
        let in_body = self.in_body();
        self.skip_local_refs = in_body;
        match statement {
            Statement::Label(label) => {
                let before = self.symbols.len();
                self.label(label, in_body);
                if self.symbols.len() > before {
                    self.items.push(Item::Label(before));
                }
            }
            Statement::Directive(directive) => {
                if !in_body {
                    self.directive_item(directive);
                }
                self.directive(directive, in_body);
            }
            Statement::Assignment(assignment) => {
                if let Some(name) = assignment.name() {
                    self.define_constant(&name, assignment.syntax(), false);
                    self.record_value(&name, assignment.value());
                }
                if let Some(value) = assignment.value() {
                    self.collect_refs(&value);
                }
            }
            Statement::RegisterAlias(alias) => {
                if let (Some(name), Some(target)) = (alias.name(), alias.target()) {
                    let target = target.text().to_string();
                    self.aliases
                        .insert(name.text().to_ascii_lowercase(), target.trim().to_string());
                }
                if let Some(name) = alias.name() {
                    self.symbols.push(Symbol {
                        name: name.text().to_ascii_lowercase(),
                        kind: SymbolKind::RegisterAlias,
                        range: range(&name),
                        statement: node_range(alias.syntax()),
                        exported: false,
                        scope_end: None,
                    });
                }
            }
            Statement::MacroCall(call) => {
                if !in_body {
                    self.items.push(Item::MacroCall(node_range(call.syntax())));
                }
                if let Some(name) = call.name() {
                    self.pending.push(Pending {
                        name: name.text().to_string(),
                        range: range(&name),
                        macro_only: true,
                    });
                }
                self.collect_refs(call.syntax());
            }
            Statement::Instruction(insn) => {
                if let Some(mnemonic) = insn.mnemonic().filter(|_| !in_body) {
                    let checkable = self.check_mnemonic(&mnemonic);
                    let flat = if checkable {
                        self.check_operands(insn, &mnemonic)
                    } else {
                        None
                    };
                    let (operands, operand_ranges) = match flat {
                        Some(f) => (Some(f.operands), f.ranges),
                        None => (None, Vec::new()),
                    };
                    self.items.push(Item::Instruction(Instruction {
                        range: node_range(insn.syntax()),
                        mnemonic: Mnemonic::parse(mnemonic.text()),
                        mnemonic_range: range(&mnemonic),
                        operands,
                        operand_ranges,
                    }));
                }
                self.collect_refs(insn.syntax());
            }
        }
    }

    fn label(&mut self, label: &ast::Label, in_body: bool) {
        // Labels in macro and repeat bodies are defined once per expansion.
        let Some(token) = label.name().filter(|_| !in_body) else {
            return;
        };
        let name = unquote(token.text());
        let kind = if label.is_local() {
            SymbolKind::LocalLabel
        } else {
            SymbolKind::Label
        };
        if kind == SymbolKind::Label && self.find(&name, SymbolKind::Label).is_some() {
            self.diagnostics.push(Diagnostic::error(
                range(&token),
                "duplicate-symbol",
                format!("symbol '{name}' is already defined: rename one of the labels"),
            ));
        }
        self.symbols.push(Symbol {
            name,
            kind,
            range: range(&token),
            statement: node_range(label.syntax()),
            exported: false,
            scope_end: None,
        });
    }

    fn directive(&mut self, directive: &ast::Directive, in_body: bool) {
        let Some(name_token) = directive.name() else {
            return;
        };
        let name = name_token.text().to_ascii_lowercase();
        let args: Vec<SyntaxNode> = directive.args().collect();
        let first_name = args
            .first()
            .and_then(|a| ast::NameRef::cast(a.clone()))
            .and_then(|n| n.name());
        match name.as_str() {
            ".macro" => {
                if let Some(macro_name) = &first_name {
                    if self.find(macro_name.text(), SymbolKind::Macro).is_some() {
                        self.diagnostics.push(Diagnostic::error(
                            range(macro_name),
                            "duplicate-macro",
                            format!(
                                "macro '{}' is already defined: rename it, or remove one definition",
                                macro_name.text()
                            ),
                        ));
                    }
                    self.symbols.push(Symbol {
                        name: macro_name.text().to_string(),
                        kind: SymbolKind::Macro,
                        range: range(macro_name),
                        statement: node_range(directive.syntax()),
                        exported: false,
                        scope_end: None,
                    });
                }
                self.open(BlockKind::Macro, &name, &name_token);
                return;
            }
            ".rept" | ".rep" | ".irp" | ".irpc" => {
                self.collect_refs(directive.syntax());
                self.open(BlockKind::Repeat, &name, &name_token);
                return;
            }
            ".endm" | ".endmacro" => return self.close(BlockKind::Macro, &name, &name_token),
            ".endr" => return self.close(BlockKind::Repeat, &name, &name_token),
            ".endif" => return self.close(BlockKind::Conditional, &name, &name_token),
            ".else" | ".elseif" => {
                if self.blocks.last().map(|b| b.kind) != Some(BlockKind::Conditional) {
                    self.diagnostics.push(Diagnostic::error(
                        range(&name_token),
                        "unbalanced-block",
                        format!("{name} without an open .if: add the .if, or remove this line"),
                    ));
                }
                self.collect_refs(directive.syntax());
                return;
            }
            _ => {}
        }
        if name.starts_with(".if") && arm_isa::is_directive(&name) {
            self.collect_refs(directive.syntax());
            self.open(BlockKind::Conditional, &name, &name_token);
            return;
        }
        if !in_body && !arm_isa::is_directive(&name) {
            let mut message = format!("unknown directive '{}'", name_token.text());
            if ELF_ONLY.contains(&name.as_str()) {
                message.push_str(": it's ELF only, and macOS uses Mach-O. Remove it");
            } else if let Some(better) = suggest(&name, arm_isa::directives()) {
                message.push_str(&format!(": did you mean '{better}'?"));
            }
            self.diagnostics.push(Diagnostic::error(
                range(&name_token),
                "unknown-directive",
                message,
            ));
        }
        match name.as_str() {
            ".include" => self.has_include = true,
            ".set" | ".equ" | ".equiv" => {
                if let Some(constant) = &first_name {
                    self.define_constant(constant, directive.syntax(), name == ".equiv");
                    self.record_value(constant, args.get(1).cloned());
                }
                for arg in args.iter().skip(1) {
                    self.collect_refs(arg);
                }
                return;
            }
            ".unreq" => {
                if let Some(alias) = &first_name {
                    let lower = alias.text().to_ascii_lowercase();
                    self.aliases.remove(&lower);
                    // The alias stays in scope for its own `.unreq`.
                    let at = range(alias).end;
                    if let Some(symbol) = self.symbols.iter_mut().rev().find(|s| {
                        s.kind == SymbolKind::RegisterAlias
                            && s.name == lower
                            && s.scope_end.is_none()
                    }) {
                        symbol.scope_end = Some(at);
                    }
                }
            }
            ".globl" | ".global" => {
                for arg in &args {
                    if let Some(n) = ast::NameRef::cast(arg.clone()).and_then(|n| n.name()) {
                        let exported = unquote(n.text());
                        self.pending.push(Pending {
                            name: exported.clone(),
                            range: range(&n),
                            macro_only: false,
                        });
                        self.exported.push(exported);
                    }
                }
                return;
            }
            _ => {}
        }
        self.collect_refs(directive.syntax());
    }

    fn define_constant(&mut self, name: &SyntaxToken, statement: &SyntaxNode, equiv: bool) {
        let text = unquote(name.text());
        let clash = self.find(&text, SymbolKind::Label).is_some()
            || (equiv && self.find(&text, SymbolKind::Constant).is_some());
        if clash {
            self.diagnostics.push(Diagnostic::error(
                range(name),
                "duplicate-symbol",
                format!("redefinition of '{text}': it's already a label or .equiv constant"),
            ));
        }
        self.symbols.push(Symbol {
            name: text,
            kind: SymbolKind::Constant,
            range: range(name),
            statement: node_range(statement),
            exported: false,
            scope_end: None,
        });
    }

    /// Remembers a constant's value for operands below it, or forgets it if
    /// the new value isn't a known integer.
    fn record_value(&mut self, name: &SyntaxToken, value: Option<SyntaxNode>) {
        let scope = Scope {
            constants: &self.constants,
            aliases: &self.aliases,
        };
        let value = value.map(|v| operands::eval(&v, &scope));
        let name = unquote(name.text());
        match value {
            Some(Value::Int(v)) => {
                self.constants.insert(name, v);
            }
            _ => {
                self.constants.remove(&name);
            }
        }
    }

    /// Checks operands against the instruction's forms, as clang's matcher does.
    /// Returns the flattened operands, for the lint rules.
    fn check_operands(
        &mut self,
        insn: &ast::Instruction,
        mnemonic: &SyntaxToken,
    ) -> Option<operands::Flattened> {
        let scope = Scope {
            constants: &self.constants,
            aliases: &self.aliases,
        };
        let flat = operands::flatten(insn, &scope)?;
        let parsed = Mnemonic::parse(mnemonic.text());
        let arm_isa::MatchResult::Mismatch(mismatch) =
            arm_isa::match_instruction(&parsed, &flat.operands)
        else {
            return Some(flat);
        };
        let at = match mismatch.operand {
            Some(i) => flat.ranges[i].clone(),
            None => {
                let end = flat.ranges.last().map_or(range(mnemonic).end, |r| r.end);
                range(mnemonic).start..end
            }
        };
        let mut message = mismatch.message.clone();
        if let Some(form) = mismatch
            .closest
            .filter(|_| message.ends_with("for instruction"))
        {
            message.push_str(&format!(": the closest form is `{}`", form.signature()));
        }
        self.diagnostics
            .push(Diagnostic::error(at, "invalid-operand", message));
        Some(flat)
    }

    /// Records section switches and data directives as items.
    fn directive_item(&mut self, directive: &ast::Directive) {
        let Some(name) = directive.name() else {
            return;
        };
        let lower = name.text().to_ascii_lowercase();
        let whole = node_range(directive.syntax());
        let text_section =
            |args: &str| args.starts_with("__TEXT,__text") || args.contains("pure_instructions");
        match lower.as_str() {
            ".text" => self.items.push(Item::Section {
                range: whole,
                text: true,
            }),
            ".data" | ".bss" | ".const" | ".const_data" | ".cstring" | ".literal4"
            | ".literal8" | ".literal16" | ".tdata" | ".tbss" | ".static_data" => {
                self.items.push(Item::Section {
                    range: whole,
                    text: false,
                });
            }
            ".section" => {
                let args: String = directive
                    .syntax()
                    .text()
                    .to_string()
                    .split_once(char::is_whitespace)
                    .map(|(_, rest)| rest.chars().filter(|c| !c.is_whitespace()).collect())
                    .unwrap_or_default();
                let text = text_section(&args);
                self.items.push(Item::Section { range: whole, text });
            }
            ".byte" | ".short" | ".hword" | ".2byte" | ".long" | ".int" | ".word" | ".4byte"
            | ".quad" | ".xword" | ".8byte" | ".dword" | ".octa" | ".float" | ".single"
            | ".double" | ".ascii" | ".asciz" | ".string" | ".space" | ".skip" | ".zero"
            | ".fill" | ".inst" | ".zerofill" | ".ltorg" | ".pool" => {
                self.items.push(Item::Data(whole));
            }
            _ => {}
        }
    }

    /// Reports an unknown mnemonic or condition. Returns whether it's an
    /// instruction whose operands can be checked.
    fn check_mnemonic(&mut self, token: &SyntaxToken) -> bool {
        let text = token.text();
        let mnemonic = Mnemonic::parse(text);
        if let Some(condition) = &mnemonic.condition {
            if CONDITION_CODES.contains(&condition.as_str()) {
                return true;
            }
            let mut message = format!("invalid condition code '{condition}'");
            if let Some(better) = suggest(condition, CONDITION_CODES.iter().copied()) {
                message.push_str(&format!(": did you mean '{better}'?"));
            }
            self.diagnostics.push(Diagnostic::error(
                range(token),
                "invalid-condition",
                message,
            ));
            return false;
        }
        if self.find(text, SymbolKind::Macro).is_some() {
            return false;
        }
        if arm_isa::is_instruction(text) {
            return true;
        }
        if self.later_macros.iter().any(|m| m == text) {
            self.diagnostics.push(Diagnostic::error(
                range(token),
                "macro-before-definition",
                format!(
                    "'{text}' is a macro defined later in the file: clang needs a macro defined before its first use, so move the .macro above this line"
                ),
            ));
            return false;
        }
        let mut message = format!("unknown instruction '{text}'");
        if let Some(better) = suggest(&mnemonic.head, arm_isa::mnemonics()) {
            message.push_str(&format!(": did you mean '{better}'?"));
        }
        if self.has_include {
            message.push_str(" If a file included above defines it as a macro, ignore this.");
            self.diagnostics.push(Diagnostic {
                range: range(token),
                severity: Severity::Warning,
                code: "unknown-instruction",
                message,
            });
        } else {
            self.diagnostics.push(Diagnostic::error(
                range(token),
                "unknown-instruction",
                message,
            ));
        }
        false
    }

    /// Names and local label references inside a node.
    fn collect_refs(&mut self, node: &SyntaxNode) {
        for element in node.descendants_with_tokens() {
            match element {
                NodeOrToken::Node(n) => {
                    if let Some(name) = ast::NameRef::cast(n).and_then(|n| n.name()) {
                        self.pending.push(Pending {
                            name: unquote(name.text()),
                            range: range(&name),
                            macro_only: false,
                        });
                    }
                }
                NodeOrToken::Token(t)
                    if t.kind() == SyntaxKind::LocalLabelRef && !self.skip_local_refs =>
                {
                    let text = t.text();
                    let number = text[..text.len() - 1].to_string();
                    self.local_refs
                        .push((number, range(&t), text.ends_with('f')));
                }
                NodeOrToken::Token(_) => {}
            }
        }
    }

    fn open(&mut self, kind: BlockKind, directive: &str, token: &SyntaxToken) {
        self.blocks.push(Block {
            kind,
            directive: directive.to_string(),
            range: range(token),
        });
    }

    fn close(&mut self, kind: BlockKind, directive: &str, token: &SyntaxToken) {
        if self.blocks.last().map(|b| b.kind) == Some(kind) {
            self.blocks.pop();
            return;
        }
        let opener = match kind {
            BlockKind::Macro => ".macro",
            BlockKind::Repeat => ".rept, .irp, or .irpc",
            BlockKind::Conditional => ".if",
        };
        self.diagnostics.push(Diagnostic::error(
            range(token),
            "unbalanced-block",
            format!("{directive} without an open {opener}: add the {opener}, or remove this line"),
        ));
    }

    fn close_blocks(&mut self) {
        for block in std::mem::take(&mut self.blocks) {
            let closer = match block.kind {
                BlockKind::Macro => ".endm",
                BlockKind::Repeat => ".endr",
                BlockKind::Conditional => ".endif",
            };
            self.diagnostics.push(Diagnostic::error(
                block.range,
                "unbalanced-block",
                format!("this {} is never closed: add {closer}", block.directive),
            ));
        }
    }

    fn find(&self, name: &str, kind: SymbolKind) -> Option<usize> {
        self.symbols
            .iter()
            .position(|s| s.kind == kind && s.name == name)
    }

    /// Resolves every reference now that all definitions are known.
    fn resolve(&mut self) -> Vec<Reference> {
        for name in std::mem::take(&mut self.exported) {
            for symbol in self.symbols.iter_mut().filter(|s| s.name == name) {
                symbol.exported = true;
            }
        }
        let mut references = Vec::new();
        for pending in std::mem::take(&mut self.pending) {
            let symbol = if pending.macro_only {
                self.find(&pending.name, SymbolKind::Macro)
            } else {
                self.resolve_name(&pending.name, pending.range.start)
            };
            references.push(Reference {
                name: pending.name,
                range: pending.range,
                symbol,
            });
        }
        for (number, range, forward) in std::mem::take(&mut self.local_refs) {
            let candidates = self
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| s.kind == SymbolKind::LocalLabel && s.name == number);
            let symbol = if forward {
                candidates
                    .filter(|(_, s)| s.range.start > range.start)
                    .map(|(i, _)| i)
                    .next()
            } else {
                candidates
                    .filter(|(_, s)| s.range.start < range.start)
                    .map(|(i, _)| i)
                    .next_back()
            };
            if symbol.is_none() {
                let direction = if forward { "after" } else { "before" };
                self.diagnostics.push(Diagnostic::error(
                    range.clone(),
                    "undefined-local-label",
                    format!(
                        "directional label undefined: there's no '{number}:' {direction} this line"
                    ),
                ));
            }
            references.push(Reference {
                name: number,
                range,
                symbol,
            });
        }
        references.sort_by_key(|r| r.range.start);
        references
    }

    fn resolve_name(&self, name: &str, at: usize) -> Option<usize> {
        let lower = name.to_ascii_lowercase();
        let alias = self.symbols.iter().rposition(|s| {
            s.kind == SymbolKind::RegisterAlias
                && s.name == lower
                && s.range.start < at
                && s.scope_end.is_none_or(|end| at < end)
        });
        alias.or_else(|| {
            self.symbols.iter().position(|s| {
                matches!(
                    s.kind,
                    SymbolKind::Label | SymbolKind::Constant | SymbolKind::Macro
                ) && s.name == name
            })
        })
    }
}

/// Names of every `.macro` in the file.
fn macro_names(parse: &Parse) -> Vec<String> {
    parse
        .tree()
        .statements()
        .filter_map(|s| match s {
            Statement::Directive(d)
                if d.name()
                    .is_some_and(|n| n.text().eq_ignore_ascii_case(".macro")) =>
            {
                d.args()
                    .next()
                    .and_then(ast::NameRef::cast)
                    .and_then(|n| n.name())
                    .map(|n| n.text().to_string())
            }
            _ => None,
        })
        .collect()
}

fn unquote(text: &str) -> String {
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text)
        .to_string()
}

fn range(token: &SyntaxToken) -> Range<usize> {
    let r = token.text_range();
    usize::from(r.start())..usize::from(r.end())
}

fn node_range(node: &SyntaxNode) -> Range<usize> {
    let r = node.text_range();
    usize::from(r.start())..usize::from(r.end())
}
