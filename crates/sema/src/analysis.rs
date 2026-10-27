//! The semantic model of one file: symbols, references to them, block
//! structure, and the errors clang reports for them. Each check here was
//! probed against clang; see docs/decisions/0015-sema-file-model.md.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use arm_isa::{CONDITION_CODES, FeatureSet, Mnemonic};
use arm_syntax::ast::{self, Statement};
use arm_syntax::{NodeOrToken, Parse, SyntaxKind, SyntaxNode, SyntaxToken};

use crate::diagnostic::{Diagnostic, Severity};
use crate::macros::{self, MacroDef};
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
    /// Named by `.globl`, `.global`, or `.private_extern`: other object files
    /// can link to it.
    pub exported: bool,
    /// For register aliases: where `.unreq` ends it.
    pub scope_end: Option<usize>,
    /// For symbols from an included file: that file. `range` and `statement`
    /// are offsets in its text. None for symbols defined in this file.
    pub origin: Option<Arc<Origin>>,
}

/// A file that symbols were imported from with `.include`.
#[derive(Debug, PartialEq, Eq)]
pub struct Origin {
    pub path: PathBuf,
    pub text: String,
}

/// Finds the files that `.include` names.
pub trait Resolver {
    /// The path and text of an included file, from its name as written.
    fn resolve(&self, name: &str) -> Option<(PathBuf, String)>;
}

/// How deep includes may nest before the analysis stops following them.
const MAX_INCLUDE_DEPTH: usize = 16;

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
    inactive: Vec<Range<usize>>,
    /// Macro definitions, for files that include this one.
    macro_defs: HashMap<String, MacroDef>,
    /// Constants with known values at the end of the file, for files that
    /// include this one.
    constants: HashMap<String, i64>,
    /// Register aliases in scope at the end of the file.
    aliases: HashMap<String, String>,
    includes: Vec<(Range<usize>, PathBuf)>,
}

impl Analysis {
    /// Analyzes a file on its own: `.include` isn't followed, and CPU
    /// features start as clang's default for `-arch arm64`.
    pub fn new(text: &str) -> Analysis {
        Analysis::with_options(text, None, None, Some(FeatureSet::apple_default()))
    }

    /// Analyzes a file, following `.include` through `resolver` and importing
    /// what included files define: macros, constants, register aliases, and
    /// labels. `path` is this file's, to detect include cycles.
    pub fn with_includes(text: &str, path: Option<PathBuf>, resolver: &dyn Resolver) -> Analysis {
        Analysis::with_options(
            text,
            path,
            Some(resolver),
            Some(FeatureSet::apple_default()),
        )
    }

    /// The general form: an optional resolver for `.include`, and the CPU
    /// features enabled at the top of the file, as `-mcpu` or `-march` would
    /// set them. `None` turns feature checks off.
    pub fn with_options(
        text: &str,
        path: Option<PathBuf>,
        resolver: Option<&dyn Resolver>,
        features: Option<FeatureSet>,
    ) -> Analysis {
        Analysis::build(text, resolver, path.into_iter().collect(), features, None)
    }

    fn build(
        text: &str,
        resolver: Option<&dyn Resolver>,
        stack: Vec<PathBuf>,
        features: Option<FeatureSet>,
        seed: Option<Seed>,
    ) -> Analysis {
        let parse = arm_syntax::parse(text);
        let seed = seed.unwrap_or_default();
        let mut builder = Builder {
            text,
            macro_defs: seed.macro_defs,
            depth: seed.depth,
            counter: 0,
            resolver,
            inactive: Vec::new(),
            undecided: Vec::new(),
            stack,
            features,
            symbols: seed.symbols,
            items: Vec::new(),
            pending: Vec::new(),
            local_refs: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            has_include: false,
            includes: Vec::new(),
            exported: Vec::new(),
            skip_local_refs: false,
            constants: seed.constants,
            aliases: seed.aliases,
            later_macros: macro_names(&parse),
            later_constants: constant_names(&parse),
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
        // Nothing is reported from lines clang skips or that we can't decide,
        // including parse errors: clang doesn't parse skipped lines.
        let quiet = |d: &Diagnostic| {
            builder
                .inactive
                .iter()
                .chain(&builder.undecided)
                .any(|r| r.start <= d.range.start && d.range.start < r.end)
        };
        diagnostics.retain(|d| !quiet(d));
        diagnostics.sort_by_key(|d| (d.range.start, d.range.end));
        Analysis {
            text: text.to_string(),
            line_index: crate::LineIndex::new(text),
            parse,
            symbols: builder.symbols,
            references,
            diagnostics,
            items: builder.items,
            inactive: builder.inactive,
            macro_defs: builder.macro_defs,
            constants: builder.constants,
            aliases: builder.aliases,
            includes: builder.includes,
        }
    }

    /// The operands of an instruction being typed, for the matcher:
    /// `before` is the operand text up to the cursor. The operand under the
    /// cursor is left out, except a `[` it starts with. Names resolve as at
    /// the end of the file. None inside a `{` list, or if an operand can't be
    /// judged.
    pub fn partial_operands(&self, before: &str) -> Option<Vec<arm_isa::Operand>> {
        let mut depth = 0i32;
        let mut split = None;
        for (i, c) in before.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => split = Some(i),
                _ => {}
            }
        }
        if depth > 0 {
            return None;
        }
        let (done, current) = match split {
            Some(i) => (&before[..i], &before[i + 1..]),
            None => ("", before),
        };
        let mut out = Vec::new();
        if !done.trim().is_empty() {
            let open = done
                .matches('[')
                .count()
                .saturating_sub(done.matches(']').count());
            let line = format!("nop {done}{}", "]".repeat(open));
            let parse = arm_syntax::parse(&line);
            let insn = parse.tree().statements().find_map(|s| match s {
                Statement::Instruction(i) => Some(i),
                _ => None,
            })?;
            let scope = Scope {
                constants: &self.constants,
                aliases: &self.aliases,
            };
            out = operands::flatten(&insn, &scope)?.operands;
            for _ in 0..open {
                if matches!(out.last(), Some(arm_isa::Operand::Token(t)) if t == "]") {
                    out.pop();
                }
            }
        }
        if current.trim_start().starts_with('[') {
            out.push(arm_isa::Operand::Token("[".into()));
        }
        Some(out)
    }

    /// The value of a constant defined exactly once in this file (or once in
    /// what it includes), if it's known. Redefined constants (`.set` twice)
    /// have no single value.
    pub fn constant_value(&self, name: &str) -> Option<i64> {
        let definitions = self
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Constant && s.name == name)
            .count();
        (definitions == 1)
            .then(|| self.constants.get(name).copied())
            .flatten()
    }

    /// The files this one `.include`s that were found: the range of each
    /// file name, inside its quotes, and the path. Empty without a resolver.
    pub fn includes(&self) -> &[(Range<usize>, PathBuf)] {
        &self.includes
    }

    /// Byte ranges of `.if` branches clang skips because their condition is
    /// false. Editors grey them out.
    pub fn inactive(&self) -> &[Range<usize>] {
        &self.inactive
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
            .position(|s| s.origin.is_none() && touches(&s.range))
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
    /// For conditionals: whether the current branch is assembled. None when
    /// the condition can't be known here.
    branch: Option<bool>,
    /// For conditionals: whether an earlier branch was taken.
    taken: Option<bool>,
    /// Where the current branch's lines start; for macros and repeats,
    /// where the body starts.
    branch_start: usize,
    /// The text after the opening directive: `name a, b=1` for `.macro`,
    /// `r, x0, x1` for `.irp`.
    spec: String,
}

/// A name reference waiting for every definition to be known.
struct Pending {
    name: String,
    range: Range<usize>,
    /// Resolve only to a macro: the name of a macro call.
    macro_only: bool,
}

/// What an expansion starts with: the context at the call.
#[derive(Default)]
struct Seed {
    symbols: Vec<Symbol>,
    constants: HashMap<String, i64>,
    aliases: HashMap<String, String>,
    macro_defs: HashMap<String, MacroDef>,
    depth: usize,
}

/// How deep macro expansions may nest before we stop following them.
const MAX_EXPANSION_DEPTH: usize = 8;

struct Builder<'r> {
    text: &'r str,
    /// Macros defined so far, with their bodies.
    macro_defs: HashMap<String, MacroDef>,
    /// How many expansions deep this analysis is.
    depth: usize,
    /// For `\@`: one more for each expansion.
    counter: usize,
    resolver: Option<&'r dyn Resolver>,
    /// Lines in branches clang skips.
    inactive: Vec<Range<usize>>,
    /// Lines in branches whose condition we can't decide: analyzed, but
    /// their errors aren't reported.
    undecided: Vec<Range<usize>>,
    /// CPU features enabled here, changed by `.arch`, `.cpu`, and
    /// `.arch_extension`. None when we can't tell: no checks then.
    features: Option<FeatureSet>,
    /// Paths of the files including this one, outermost first, and this one.
    stack: Vec<PathBuf>,
    symbols: Vec<Symbol>,
    items: Vec<Item>,
    pending: Vec<Pending>,
    /// `1f` and `1b`: (number, range, forward).
    local_refs: Vec<(String, Range<usize>, bool)>,
    diagnostics: Vec<Diagnostic>,
    blocks: Vec<Block>,
    has_include: bool,
    /// Each `.include` found: its file name's range and the path.
    includes: Vec<(Range<usize>, PathBuf)>,
    /// Names given to `.globl` or `.global`.
    exported: Vec<String>,
    /// Inside a macro or repeat body, where `1f` resolves per expansion.
    skip_local_refs: bool,
    /// Constants defined so far with known values. Clang folds a constant
    /// into an operand only if it's defined above the use.
    constants: HashMap<String, i64>,
    /// Register aliases in scope: lowercase alias to register text.
    aliases: HashMap<String, String>,
    /// Every constant defined anywhere in the file, for a better message when
    /// one is used above its definition.
    later_constants: Vec<String>,
    /// Every macro defined anywhere in the file, for a better message when one
    /// is called before its definition.
    later_macros: Vec<String>,
}

impl Builder<'_> {
    fn in_body(&self) -> bool {
        self.blocks.iter().any(|b| b.kind != BlockKind::Conditional)
    }

    fn statement(&mut self, statement: &Statement) {
        // In a branch clang skips, only the conditionals' own nesting counts.
        if self.active() == Some(false) {
            if let Statement::Directive(directive) = statement {
                let name = directive
                    .name()
                    .map(|n| n.text().to_ascii_lowercase())
                    .unwrap_or_default();
                if name.starts_with(".if") && arm_isa::is_directive(&name) {
                    if let Some(token) = directive.name() {
                        self.open(BlockKind::Conditional, &name, &token);
                        if let Some(block) = self.blocks.last_mut() {
                            block.branch = Some(false);
                            block.taken = Some(true);
                        }
                    }
                } else if matches!(name.as_str(), ".else" | ".elseif" | ".endif") {
                    self.directive(directive, self.in_body());
                }
            }
            return;
        }
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
                        origin: None,
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
                    if !in_body {
                        self.expand_call(name.text(), call.syntax(), &name);
                    }
                }
                self.collect_refs(call.syntax());
            }
            Statement::Instruction(insn) => {
                // A call to a macro from an included file: the parser couldn't
                // know it was a macro.
                let imported_macro = insn.mnemonic().filter(|m| {
                    self.symbols.iter().any(|s| {
                        s.kind == SymbolKind::Macro && s.origin.is_some() && s.name == m.text()
                    })
                });
                if let Some(name) = imported_macro {
                    if !in_body {
                        self.items.push(Item::MacroCall(node_range(insn.syntax())));
                    }
                    self.pending.push(Pending {
                        name: name.text().to_string(),
                        range: range(&name),
                        macro_only: true,
                    });
                    if !in_body {
                        self.expand_call(name.text(), insn.syntax(), &name);
                    }
                    self.collect_refs(insn.syntax());
                    return;
                }
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
            origin: None,
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
                        origin: None,
                    });
                }
                self.open(BlockKind::Macro, &name, &name_token);
                self.set_spec(directive);
                return;
            }
            ".rept" | ".rep" | ".irp" | ".irpc" => {
                self.collect_refs(directive.syntax());
                self.open(BlockKind::Repeat, &name, &name_token);
                self.set_spec(directive);
                return;
            }
            ".endm" | ".endmacro" => {
                self.end_macro(node_range(directive.syntax()).start);
                return self.close(BlockKind::Macro, &name, &name_token);
            }
            ".endr" => {
                self.end_repeat(node_range(directive.syntax()).start);
                return self.close(BlockKind::Repeat, &name, &name_token);
            }
            ".endif" => {
                if self.blocks.last().map(|b| b.kind) == Some(BlockKind::Conditional) {
                    self.end_branch(node_range(directive.syntax()).start);
                }
                return self.close(BlockKind::Conditional, &name, &name_token);
            }
            ".else" | ".elseif" => {
                if self.blocks.last().map(|b| b.kind) != Some(BlockKind::Conditional) {
                    self.diagnostics.push(Diagnostic::error(
                        range(&name_token),
                        "unbalanced-block",
                        format!("{name} without an open .if: add the .if, or remove this line"),
                    ));
                    return;
                }
                self.end_branch(node_range(directive.syntax()).start);
                let taken = self.blocks.last().and_then(|b| b.taken);
                let branch = match (name.as_str(), taken) {
                    (_, Some(true)) => Some(false),
                    (".else", Some(false)) => Some(true),
                    (".elseif", Some(false)) if !in_body => self.condition(".if", directive, &args),
                    _ => None,
                };
                if let Some(block) = self.blocks.last_mut() {
                    block.taken = match (block.taken, branch) {
                        (Some(true), _) | (_, Some(true)) => Some(true),
                        (Some(false), Some(false)) => Some(false),
                        _ => None,
                    };
                    block.branch = branch;
                    block.branch_start = node_range(directive.syntax()).end;
                }
                self.collect_refs(directive.syntax());
                return;
            }
            _ => {}
        }
        if name.starts_with(".if") && arm_isa::is_directive(&name) {
            self.collect_refs(directive.syntax());
            // Inside a macro body, conditions depend on the arguments.
            let branch = if in_body {
                Some(true)
            } else {
                self.condition(&name, directive, &args)
            };
            self.open(BlockKind::Conditional, &name, &name_token);
            if let Some(block) = self.blocks.last_mut() {
                block.branch = branch;
                block.taken = branch;
                block.branch_start = node_range(directive.syntax()).end;
            }
            return;
        }
        if !in_body && !arm_isa::is_directive(&name) {
            let mut message = format!("unknown directive '{}'", name_token.text());
            let mut fix = None;
            if ELF_ONLY.contains(&name.as_str()) {
                message.push_str(": it's ELF only, and macOS uses Mach-O. Remove it");
                fix = Some(crate::Fix {
                    title: format!("Remove the {name} line"),
                    edits: vec![(node_range(directive.syntax()), String::new())],
                });
            } else if let Some(better) = suggest(&name, arm_isa::directives()) {
                message.push_str(&format!(": did you mean '{better}'?"));
                fix = Some(crate::Fix {
                    title: format!("Change to '{better}'"),
                    edits: vec![(range(&name_token), better.to_string())],
                });
            }
            let mut diagnostic =
                Diagnostic::error(range(&name_token), "unknown-directive", message);
            diagnostic.fix = fix;
            self.diagnostics.push(diagnostic);
        }
        match name.as_str() {
            ".include" => self.include(directive, &args, &name_token),
            ".arch" | ".cpu" => {
                // The spec as written, like `armv8.5-a+sve`.
                let spec: String = directive
                    .syntax()
                    .text()
                    .to_string()
                    .split_once(char::is_whitespace)
                    .map(|(_, rest)| {
                        rest.split([';', '/'])
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string()
                    })
                    .unwrap_or_default();
                let set = if name == ".arch" {
                    FeatureSet::for_arch(&spec)
                } else {
                    FeatureSet::for_cpu(&spec)
                };
                self.features = set.ok();
            }
            ".arch_extension" => {
                let mut ok = true;
                for arg in &args {
                    let text = arg.text().to_string();
                    if let Some(features) = self.features.as_mut() {
                        ok &= features.modify(text.trim()).is_ok();
                    }
                }
                if !ok {
                    self.features = None;
                }
            }
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
            ".globl" | ".global" | ".private_extern" => {
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

    /// Follows `.include "file"`, importing what the file defines.
    fn include(&mut self, directive: &ast::Directive, args: &[SyntaxNode], name: &SyntaxToken) {
        let literal = args.first().and_then(|a| {
            a.children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind() == SyntaxKind::StringLit)
        });
        let target = literal.as_ref().map(|t| unquote(t.text()));
        let (Some(resolver), Some(target)) = (self.resolver, target) else {
            self.has_include = true;
            return;
        };
        let at = node_range(directive.syntax()).start..range(name).end;
        let Some((path, text)) = resolver.resolve(&target) else {
            self.has_include = true;
            self.diagnostics.push(Diagnostic {
                range: at,
                severity: Severity::Warning,
                code: "include-not-found",
                fix: None,
                message: format!(
                    "can't find '{target}' from the project root, the [include] paths in armtool.toml, or this file's directory. Clang looks relative to the directory it runs in and -I paths: add the directory to [include] paths"
                ),
            });
            return;
        };
        if let Some(literal) = &literal {
            let r = range(literal);
            let closed = literal.text().len() >= 2 && literal.text().ends_with('"');
            let end = if closed { r.end - 1 } else { r.end };
            self.includes.push((r.start + 1..end, path.clone()));
        }
        let key = crate::canonical(&path);
        if let Some(start) = self.stack.iter().position(|p| crate::canonical(p) == key) {
            let mut chain: Vec<String> = self.stack[start..]
                .iter()
                .map(|p| {
                    p.file_name().map_or_else(
                        || p.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    )
                })
                .collect();
            chain.push(chain[0].clone());
            self.diagnostics.push(Diagnostic::error(
                at,
                "include-cycle",
                format!(
                    "include cycle: {}. Clang loops forever on this: remove one of the includes",
                    chain.join(" includes ")
                ),
            ));
            return;
        }
        if self.stack.len() >= MAX_INCLUDE_DEPTH {
            return;
        }
        let mut stack = self.stack.clone();
        stack.push(path.clone());
        let included = Analysis::build(&text, self.resolver, stack, self.features.clone(), None);
        let origin = Arc::new(Origin { path, text });
        for symbol in included.symbols {
            let importable = match symbol.kind {
                SymbolKind::LocalLabel => false,
                SymbolKind::RegisterAlias => symbol.scope_end.is_none(),
                _ => true,
            };
            if importable {
                let origin = symbol.origin.clone().or_else(|| Some(origin.clone()));
                self.symbols.push(Symbol { origin, ..symbol });
            }
        }
        self.constants.extend(included.constants);
        self.macro_defs.extend(included.macro_defs);
        self.aliases.extend(included.aliases);
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
            origin: None,
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
        let mismatch = match arm_isa::match_for(&parsed, &flat.operands, self.features.as_ref()) {
            arm_isa::MatchResult::Mismatch(mismatch) => mismatch,
            arm_isa::MatchResult::MissingFeatures { needs, .. } => {
                let wanted: Vec<String> = needs.iter().map(|any| any.join(" or ")).collect();
                let first = needs
                    .first()
                    .and_then(|any| any.first())
                    .copied()
                    .unwrap_or("");
                let end = flat.ranges.last().map_or(range(mnemonic).end, |r| r.end);
                self.diagnostics.push(Diagnostic::error(
                    range(mnemonic).start..end,
                    "missing-feature",
                    format!(
                        "instruction requires: {}. Enable it with .arch_extension {first} above this line, or set the target in armtool.toml if your build uses -mcpu or -march",
                        wanted.join(", ")
                    ),
                ));
                return Some(flat);
            }
            _ => return Some(flat),
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
        // A constant used above its definition is a symbol to clang, not a
        // number: say so, since clang's message doesn't.
        let late = mismatch.operand.and_then(|i| match &flat.operands[i] {
            arm_isa::Operand::Imm(arm_isa::Imm::Symbol {
                name,
                variant: None,
                ..
            }) => self.later_constants.contains(name).then(|| name.clone()),
            _ => None,
        });
        if let Some(name) = late {
            message.push_str(&format!(
                ". '{name}' is defined below this line, and clang only uses a constant's value when it's defined above: move its definition up"
            ));
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
            let mut diagnostic = Diagnostic::error(range(token), "invalid-condition", "");
            if let Some(better) = suggest(condition, CONDITION_CODES.iter().copied()) {
                message.push_str(&format!(": did you mean '{better}'?"));
                let fixed = format!("{}.{better}", mnemonic.head);
                diagnostic = diagnostic.replace_with(format!("Change to '{fixed}'"), fixed);
            }
            diagnostic.message = message;
            self.diagnostics.push(diagnostic);
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
        let better = suggest(&mnemonic.head, arm_isa::mnemonics());
        if let Some(better) = better {
            message.push_str(&format!(": did you mean '{better}'?"));
        }
        let severity = if self.has_include {
            message.push_str(" If a file included above defines it as a macro, ignore this.");
            Severity::Warning
        } else {
            Severity::Error
        };
        let mut diagnostic = Diagnostic {
            severity,
            ..Diagnostic::error(range(token), "unknown-instruction", message)
        };
        if let Some(better) = better {
            diagnostic = diagnostic.replace_with(format!("Change to '{better}'"), better);
        }
        self.diagnostics.push(diagnostic);
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
            branch: Some(true),
            taken: Some(true),
            branch_start: range(token).end,
            spec: String::new(),
        });
    }

    /// Records the text after a block's opening directive, and where its
    /// body starts.
    fn set_spec(&mut self, directive: &ast::Directive) {
        let whole = node_range(directive.syntax());
        let spec = directive
            .syntax()
            .text()
            .to_string()
            .split_once(char::is_whitespace)
            .map(|(_, rest)| strip_comment(rest).trim().to_string())
            .unwrap_or_default();
        if let Some(block) = self.blocks.last_mut() {
            block.spec = spec;
            block.branch_start = whole.end;
        }
    }

    /// Whether only one macro or repeat body is open: the one on top.
    fn outermost_body(&self) -> bool {
        self.blocks
            .iter()
            .filter(|b| b.kind != BlockKind::Conditional)
            .count()
            == 1
    }

    /// Analyzes expanded text as ordinary code, in the context here, and
    /// reports its problems at `at`, where the call or repeat is.
    fn check_expansion(&mut self, at: Range<usize>, what: &str, text: &str) {
        if self.depth >= MAX_EXPANSION_DEPTH {
            return;
        }
        let seed = Seed {
            symbols: self
                .symbols
                .iter()
                .filter(|s| matches!(s.kind, SymbolKind::Macro | SymbolKind::Constant))
                .cloned()
                .collect(),
            constants: self.constants.clone(),
            aliases: self.aliases.clone(),
            macro_defs: self.macro_defs.clone(),
            depth: self.depth + 1,
        };
        let expanded = Analysis::build(text, None, Vec::new(), self.features.clone(), Some(seed));
        for d in expanded.diagnostics {
            let diagnostic = Diagnostic {
                range: at.clone(),
                severity: d.severity,
                code: d.code,
                message: format!("{} (in the expansion of {what})", d.message),
                fix: None,
            };
            // The same problem from each value of an .irp is reported once.
            if d.severity != Severity::Hint && !self.diagnostics.contains(&diagnostic) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    /// At `.endm`: records the macro's definition, if this is a top-level
    /// macro (one inside another is defined only when the outer expands).
    fn end_macro(&mut self, end: usize) {
        let Some(block) = self.blocks.last() else {
            return;
        };
        if block.kind != BlockKind::Macro || !self.outermost_body() {
            return;
        }
        let spec = block.spec.clone();
        let body = self
            .text
            .get(block.branch_start..end)
            .unwrap_or_default()
            .to_string();
        let (name, params) = spec
            .split_once(|c: char| c == ',' || c.is_whitespace())
            .unwrap_or((spec.as_str(), ""));
        if !name.is_empty() {
            let def = MacroDef {
                params: macros::parse_params(params),
                body,
            };
            self.macro_defs.insert(name.to_string(), def);
        }
    }

    /// At `.endr`: checks the repeated lines as clang expands them, once for
    /// `.rept`, once per value for `.irp` and per character for `.irpc`.
    fn end_repeat(&mut self, end: usize) {
        let Some(block) = self.blocks.last() else {
            return;
        };
        if block.kind != BlockKind::Repeat || !self.outermost_body() {
            return;
        }
        // An enclosing branch we skip or can't decide: nothing to check.
        let outer_active = self
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Conditional)
            .all(|b| b.branch == Some(true));
        if !outer_active {
            return;
        }
        let at = block.range.start..end;
        let body = self
            .text
            .get(block.branch_start..end)
            .unwrap_or_default()
            .to_string();
        let spec = block.spec.clone();
        let kind = block.directive.clone();
        let expansions: Vec<String> = match kind.as_str() {
            ".rept" | ".rep" => vec![body],
            ".irp" | ".irpc" => {
                let (symbol, values) = spec.split_once(',').unwrap_or((spec.as_str(), ""));
                let symbol = symbol.trim().to_string();
                let values: Vec<String> = if kind == ".irp" {
                    macros::split_args(values)
                } else {
                    values.trim().chars().map(String::from).collect()
                };
                let values = if values.is_empty() {
                    vec![String::new()]
                } else {
                    values
                };
                values
                    .into_iter()
                    .map(|v| macros::substitute(&body, &[(symbol.clone(), v)], 0))
                    .collect()
            }
            _ => Vec::new(),
        };
        for text in expansions {
            self.check_expansion(at.clone(), &kind, &text);
        }
    }

    /// Binds a call's arguments and checks the expansion.
    fn expand_call(&mut self, name: &str, call: &SyntaxNode, name_token: &SyntaxToken) {
        let Some(def) = self.macro_defs.get(name).cloned() else {
            return;
        };
        let call_range = node_range(call);
        let args_text =
            strip_comment(&self.text[range(name_token).end..call_range.end]).to_string();
        let args = macros::split_args(&args_text);
        match macros::bind(name, &def, &args) {
            Ok(values) => {
                self.counter += 1;
                let text = macros::substitute(&def.body, &values, self.counter);
                self.check_expansion(call_range, &format!("'{name}'"), &text);
            }
            Err(message) => {
                self.diagnostics
                    .push(Diagnostic::error(call_range, "macro-arguments", message))
            }
        }
    }

    /// Whether clang assembles the current line: false inside a branch whose
    /// condition is false, None inside one we can't decide.
    fn active(&self) -> Option<bool> {
        let mut active = Some(true);
        for block in self
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Conditional)
        {
            match block.branch {
                Some(false) => return Some(false),
                None => active = None,
                Some(true) => {}
            }
        }
        active
    }

    /// Ends the top conditional's current branch at `end`, recording it as
    /// inactive or undecided if it was.
    fn end_branch(&mut self, end: usize) {
        let Some(block) = self.blocks.last() else {
            return;
        };
        let span = block.branch_start..end;
        match block.branch {
            Some(false) => self.inactive.push(span),
            None => self.undecided.push(span),
            Some(true) => {}
        }
    }

    /// The value of an `.if`-family condition, as clang decides it with what's
    /// defined above. None when it can't be known here.
    fn condition(
        &mut self,
        name: &str,
        directive: &ast::Directive,
        args: &[SyntaxNode],
    ) -> Option<bool> {
        let raw: String = directive
            .syntax()
            .text()
            .to_string()
            .split_once(char::is_whitespace)
            .map(|(_, rest)| rest.trim().to_string())
            .unwrap_or_default();
        let value = |builder: &mut Self| -> Option<i64> {
            let node = args.first()?;
            let scope = Scope {
                constants: &builder.constants,
                aliases: &builder.aliases,
            };
            match operands::eval(node, &scope) {
                Value::Int(v) => Some(v),
                Value::Symbol { name, .. } => {
                    builder.diagnostics.push(Diagnostic::error(
                        node_range(node),
                        "non-constant-condition",
                        format!(
                            "expected absolute expression: '{name}' isn't a constant defined above this line"
                        ),
                    ));
                    None
                }
                _ => None,
            }
        };
        let defined = |builder: &Self| {
            let symbol = args
                .first()
                .and_then(|a| ast::NameRef::cast(a.clone()))
                .and_then(|n| n.name())?;
            let text = unquote(symbol.text());
            Some(builder.symbols.iter().any(|s| {
                s.name == text && matches!(s.kind, SymbolKind::Label | SymbolKind::Constant)
            }))
        };
        let strings = || -> Option<(String, String)> {
            let (a, b) = raw.split_once(',')?;
            Some((a.trim().to_string(), b.trim().to_string()))
        };
        let unquoted = |s: String| -> Option<String> {
            Some(s.strip_prefix('"')?.strip_suffix('"')?.to_string())
        };
        match name {
            ".if" | ".ifne" => value(self).map(|v| v != 0),
            ".ifeq" => value(self).map(|v| v == 0),
            ".iflt" => value(self).map(|v| v < 0),
            ".ifle" => value(self).map(|v| v <= 0),
            ".ifgt" => value(self).map(|v| v > 0),
            ".ifge" => value(self).map(|v| v >= 0),
            ".ifdef" => defined(self),
            ".ifndef" | ".ifnotdef" => defined(self).map(|d| !d),
            ".ifc" => strings().map(|(a, b)| a == b),
            ".ifnc" => strings().map(|(a, b)| a != b),
            ".ifeqs" => strings().and_then(|(a, b)| Some(unquoted(a)? == unquoted(b)?)),
            ".ifnes" => strings().and_then(|(a, b)| Some(unquoted(a)? != unquoted(b)?)),
            ".ifb" => Some(raw.is_empty()),
            ".ifnb" => Some(!raw.is_empty()),
            _ => None,
        }
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
/// Names of every `.set`, `.equ`, `.equiv`, and `name = value` in the file.
fn constant_names(parse: &Parse) -> Vec<String> {
    parse
        .tree()
        .statements()
        .filter_map(|s| match s {
            Statement::Assignment(a) => a.name().map(|n| unquote(n.text())),
            Statement::Directive(d)
                if d.name().is_some_and(|n| {
                    matches!(
                        n.text().to_ascii_lowercase().as_str(),
                        ".set" | ".equ" | ".equiv"
                    )
                }) =>
            {
                d.args()
                    .next()
                    .and_then(ast::NameRef::cast)
                    .and_then(|n| n.name())
                    .map(|n| unquote(n.text()))
            }
            _ => None,
        })
        .collect()
}

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

/// Text before a trailing `;` or `//` comment, outside strings.
fn strip_comment(text: &str) -> &str {
    let mut in_string = false;
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_string = !in_string,
            b';' if !in_string => return &text[..i],
            b'/' if !in_string && bytes.get(i + 1) == Some(&b'/') => return &text[..i],
            _ => {}
        }
    }
    text
}
