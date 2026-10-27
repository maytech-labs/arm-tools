//! Editor features over one analyzed file, in byte offsets. The protocol
//! layer in `server.rs` converts positions and types; everything here can be
//! tested without LSP.

use std::collections::BTreeSet;
use std::ops::Range;

use arm_isa::{Mnemonic, Register, Special, Syntax};
use arm_sema::{Analysis, Symbol, SymbolKind};
use arm_syntax::{NodeOrToken, SyntaxKind, SyntaxToken, TextRange};

/// How many forms a hover lists before saying how many more there are.
const HOVER_FORMS: usize = 16;

pub struct Hover {
    pub range: Range<usize>,
    pub markdown: String,
}

pub fn hover(analysis: &Analysis, offset: usize) -> Option<Hover> {
    let token = word_at(analysis, offset)?;
    let range = token_range(&token);
    let parent = token.parent()?;
    let first_in_parent = parent
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| !t.kind().is_trivia())
        .is_some_and(|t| t == token);
    let markdown = match (parent.kind(), token.kind()) {
        (SyntaxKind::Instruction, SyntaxKind::Ident) if first_in_parent => {
            instruction_hover(analysis, token.text())?
        }
        (SyntaxKind::Directive, SyntaxKind::Ident) if first_in_parent => {
            let text = token.text().to_ascii_lowercase();
            let description =
                arm_isa::directive_description(&text).unwrap_or("Assembler directive.");
            format!("**{text}**\n\n{description}")
        }
        _ => {
            if let Some(symbol) = analysis.symbol_at(offset) {
                symbol_hover(analysis, &analysis.symbols()[symbol])
            } else if token.kind() == SyntaxKind::Ident {
                register_hover(token.text())?
            } else {
                return None;
            }
        }
    };
    Some(Hover { range, markdown })
}

fn instruction_hover(analysis: &Analysis, text: &str) -> Option<String> {
    if let Some(symbol) = analysis
        .symbols()
        .iter()
        .find(|s| s.kind == SymbolKind::Macro && s.name == text)
    {
        return Some(symbol_hover(analysis, symbol));
    }
    let mnemonic = Mnemonic::parse(text);
    match mnemonic.special() {
        Some(Special::SysAlias) => {
            return Some(format!(
                "**{}**\n\nA system operation, written as an alias for `sys`: `{} <op>{{, Xt}}`.",
                mnemonic.head, mnemonic.head
            ));
        }
        Some(Special::SyspAlias) => {
            return Some(format!(
                "**{}**\n\nA 128-bit system operation, an alias for `sysp`.",
                mnemonic.head
            ));
        }
        Some(Special::Bitfield) => {
            return Some(format!(
                "**{}**\n\nA bitfield instruction, an alias of `bfm`, `sbfm`, or `ubfm`: `{} Rd, Rn, #lsb, #width`.",
                mnemonic.head, mnemonic.head
            ));
        }
        None => {}
    }
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for form in arm_isa::forms(&mnemonic.head).filter(|f| f.syntax() == Syntax::Generic) {
        let signature = form.signature();
        if !seen.insert(signature.clone()) {
            continue;
        }
        let features: Vec<_> = form.features().map(|f| f.name()).collect();
        if features.is_empty() {
            lines.push(signature);
        } else {
            lines.push(format!("{signature}    ; needs {}", features.join(", ")));
        }
    }
    if lines.is_empty() {
        return None;
    }
    let total = lines.len();
    lines.truncate(HOVER_FORMS);
    let mut out = format!("**{}**\n\n```asm\n{}\n```", mnemonic.head, lines.join("\n"));
    if total > HOVER_FORMS {
        out.push_str(&format!("\n\nand {} more forms.", total - HOVER_FORMS));
    }
    Some(out)
}

fn symbol_hover(analysis: &Analysis, symbol: &Symbol) -> String {
    let kind = match symbol.kind {
        SymbolKind::Label if symbol.exported => "global label",
        SymbolKind::Label => "label",
        SymbolKind::LocalLabel => "local label",
        SymbolKind::Constant => "constant",
        SymbolKind::RegisterAlias => "register alias",
        SymbolKind::Macro => "macro",
    };
    // The whole source line of the definition: a label's own node is only
    // `_f:`. Symbols from included files show their own file's line.
    let text = symbol
        .origin
        .as_ref()
        .map_or(analysis.text(), |o| o.text.as_str());
    let start = text[..symbol.range.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line = text[start..end].trim();
    let index = analysis.symbols().iter().position(|s| s == symbol);
    let uses = index.map_or(0, |i| analysis.references_to(i).count());
    let mut uses = match uses {
        1 => "1 use".to_string(),
        n => format!("{n} uses"),
    };
    if let Some(name) = symbol.origin.as_ref().and_then(|o| o.path.file_name()) {
        uses.push_str(&format!(", from {}", name.to_string_lossy()));
    }
    format!(
        "**{}** ({kind}, {uses})\n\n```asm\n{line}\n```",
        symbol.name
    )
}

fn register_hover(text: &str) -> Option<String> {
    // `v0.4s` is the register `v0` with an arrangement.
    let base = text.split('.').next().unwrap_or(text);
    let register = Register::lookup(base).next()?;
    Some(register.description())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Instruction,
    Macro,
    Directive,
    Register,
    Label,
    Constant,
    RegisterAlias,
    Condition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
}

/// Completions at a byte offset: instructions, macros, and directives where a
/// statement starts, registers and symbols in operands.
pub fn completions(analysis: &Analysis, offset: usize) -> Vec<Completion> {
    let text = analysis.text();
    let offset = offset.min(text.len());
    if in_comment_or_string(analysis, offset) {
        return Vec::new();
    }
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let before = statement_prefix(&text[line_start..offset]);
    let mut out = Vec::new();
    if !before.contains(char::is_whitespace) {
        // The mnemonic or directive itself.
        if let Some(head) = before.strip_suffix('.').filter(|h| *h == "b" || *h == "bc") {
            for cond in arm_isa::CONDITION_CODES {
                out.push(completion(
                    format!("{head}.{cond}"),
                    CompletionKind::Condition,
                    None,
                ));
            }
            return out;
        }
        if !before.starts_with('.') || before.is_empty() {
            for mnemonic in arm_isa::mnemonics() {
                out.push(completion(
                    mnemonic.to_string(),
                    CompletionKind::Instruction,
                    None,
                ));
            }
            for symbol in analysis
                .symbols()
                .iter()
                .filter(|s| s.kind == SymbolKind::Macro)
            {
                out.push(completion(
                    symbol.name.clone(),
                    CompletionKind::Macro,
                    Some("macro"),
                ));
            }
        }
        for directive in arm_isa::directives() {
            let detail = arm_isa::directive_description(directive);
            out.push(completion(
                directive.to_string(),
                CompletionKind::Directive,
                detail,
            ));
        }
        return out;
    }
    for name in common_registers() {
        out.push(completion(name, CompletionKind::Register, None));
    }
    let mut seen = BTreeSet::new();
    for symbol in analysis.symbols() {
        let kind = match symbol.kind {
            SymbolKind::Label => CompletionKind::Label,
            SymbolKind::Constant => CompletionKind::Constant,
            SymbolKind::RegisterAlias if symbol.scope_end.is_none_or(|end| offset < end) => {
                CompletionKind::RegisterAlias
            }
            _ => continue,
        };
        if seen.insert(symbol.name.clone()) {
            out.push(completion(symbol.name.clone(), kind, None));
        }
    }
    out
}

fn completion(label: String, kind: CompletionKind, detail: Option<&str>) -> Completion {
    Completion {
        label,
        kind,
        detail: detail.map(str::to_string),
    }
}

/// The part of a line after its labels and leading whitespace.
fn statement_prefix(line: &str) -> &str {
    let mut rest = line.trim_start();
    // `%%` starts a new statement.
    if let Some(i) = rest.rfind("%%") {
        rest = rest[i + 2..].trim_start();
    }
    loop {
        let label_end = rest.find(':').filter(|&i| {
            rest[..i]
                .chars()
                .all(|c| c.is_alphanumeric() || "_.$\"".contains(c))
        });
        match label_end {
            Some(i) if i > 0 => rest = rest[i + 1..].trim_start(),
            _ => return rest,
        }
    }
}

fn common_registers() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for prefix in ["x", "w"] {
        names.extend((0..=30).map(|n| format!("{prefix}{n}")));
    }
    names.extend(["sp", "wsp", "xzr", "wzr", "fp", "lr"].map(String::from));
    for prefix in ["v", "q", "d", "s", "h", "b"] {
        names.extend((0..=31).map(|n| format!("{prefix}{n}")));
    }
    names
}

fn in_comment_or_string(analysis: &Analysis, offset: usize) -> bool {
    let Ok(at) = u32::try_from(offset) else {
        return false;
    };
    let tokens = analysis.parse().syntax().token_at_offset(at.into());
    tokens.into_iter().any(|t| {
        let r = token_range(&t);
        matches!(t.kind(), SyntaxKind::Comment | SyntaxKind::StringLit)
            && r.start < offset
            && (offset < r.end || t.kind() == SyntaxKind::Comment && !t.text().starts_with("/*"))
    })
}

/// Where a symbol is defined: a range in this file, or in an included one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub range: Range<usize>,
    /// The included file it's in, with that file's text; None for this file.
    pub origin: Option<std::sync::Arc<arm_sema::Origin>>,
}

/// Where the symbol at `offset` is defined.
pub fn definition(analysis: &Analysis, offset: usize) -> Option<Definition> {
    let symbol = &analysis.symbols()[analysis.symbol_at(offset)?];
    Some(Definition {
        range: symbol.range.clone(),
        origin: symbol.origin.clone(),
    })
}

/// Every use of the symbol at `offset`, and its definition if asked. For a
/// name defined in no file we can see, like `_printf`, every use of the name.
pub fn references(
    analysis: &Analysis,
    offset: usize,
    include_definition: bool,
) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if let Some(symbol) = analysis.symbol_at(offset) {
        // A definition in an included file isn't in this file's ranges.
        if include_definition && analysis.symbols()[symbol].origin.is_none() {
            out.push(analysis.symbols()[symbol].range.clone());
        }
        out.extend(analysis.references_to(symbol).map(|r| r.range.clone()));
    } else if let Some(reference) = analysis.reference_at(offset) {
        if is_register(&reference.name) {
            return out;
        }
        out.extend(
            analysis
                .references()
                .iter()
                .filter(|r| r.symbol.is_none() && r.name == reference.name)
                .map(|r| r.range.clone()),
        );
    }
    out.sort_by_key(|r| r.start);
    out
}

/// The name that rename would change at `offset`.
pub fn prepare_rename(analysis: &Analysis, offset: usize) -> Result<Range<usize>, String> {
    let symbol = analysis.symbol_at(offset).ok_or(
        "only labels, constants, macros, and register aliases defined in this file can be renamed",
    )?;
    if analysis.symbols()[symbol].kind == SymbolKind::LocalLabel {
        return Err("local numeric labels can't be renamed: change the number by hand".into());
    }
    if let Some(origin) = &analysis.symbols()[symbol].origin {
        return Err(format!(
            "this is defined in {}: rename it there",
            origin.path.display()
        ));
    }
    let token = word_at(analysis, offset).ok_or("no name here")?;
    Ok(token_range(&token))
}

/// Edits that rename the symbol at `offset` everywhere in the file.
pub fn rename(
    analysis: &Analysis,
    offset: usize,
    new_name: &str,
) -> Result<Vec<(Range<usize>, String)>, String> {
    prepare_rename(analysis, offset)?;
    let valid = new_name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || "_.$".contains(c))
        && new_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.$".contains(c));
    if !valid {
        return Err(format!(
            "'{new_name}' isn't a valid symbol name: use letters, digits, '_', '.', and '$', not starting with a digit"
        ));
    }
    if is_register(new_name) || arm_isa::is_instruction(new_name) {
        return Err(format!("'{new_name}' is a register or instruction name"));
    }
    let symbol = analysis.symbol_at(offset).ok_or("no symbol here")?;
    let kind = analysis.symbols()[symbol].kind;
    if analysis
        .symbols()
        .iter()
        .any(|s| s.kind == kind && s.name == new_name)
    {
        return Err(format!("'{new_name}' is already defined in this file"));
    }
    Ok(references(analysis, offset, true)
        .into_iter()
        .map(|range| (range, new_name.to_string()))
        .collect())
}

fn is_register(name: &str) -> bool {
    Register::lookup(name.split('.').next().unwrap_or(name))
        .next()
        .is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    pub name: String,
    pub kind: SymbolKind,
    pub exported: bool,
    /// From the definition to just before the next item.
    pub range: Range<usize>,
    pub selection: Range<usize>,
}

/// Labels, constants, aliases, and macros, in order. Local numeric labels are
/// left out: they're noise in an outline.
pub fn outline(analysis: &Analysis) -> Vec<OutlineItem> {
    let symbols: Vec<&Symbol> = analysis
        .symbols()
        .iter()
        .filter(|s| s.kind != SymbolKind::LocalLabel && s.origin.is_none())
        .collect();
    let mut items = Vec::new();
    for (i, symbol) in symbols.iter().enumerate() {
        // A label owns the code up to the next label.
        let end = if symbol.kind == SymbolKind::Label {
            symbols[i + 1..]
                .iter()
                .find(|s| s.kind == SymbolKind::Label)
                .map_or(analysis.text().len(), |next| next.statement.start)
        } else {
            symbol.statement.end
        };
        items.push(OutlineItem {
            name: symbol.name.clone(),
            kind: symbol.kind,
            exported: symbol.exported,
            range: symbol.statement.start..end.max(symbol.statement.end),
            selection: symbol.range.clone(),
        });
    }
    items
}

/// Line ranges that can fold: `.macro`, `.rept`, `.irp`, `.irpc`, and `.if`
/// blocks, the code under each label, and runs of two or more comment lines.
/// Each is (first line, last line), zero-based.
pub fn folding_ranges(analysis: &Analysis) -> Vec<(u32, u32)> {
    let text = analysis.text();
    let line_of = |offset: usize| {
        analysis
            .line_index()
            .line_col(text, offset, arm_sema::Encoding::Utf8)
            .line
    };
    let mut out = Vec::new();
    // Blocks, by matching openers and closers.
    let mut open: Vec<(&'static str, u32)> = Vec::new();
    for statement in analysis.parse().tree().statements() {
        let arm_syntax::ast::Statement::Directive(directive) = statement else {
            continue;
        };
        let Some(name) = directive.name() else {
            continue;
        };
        let line = line_of(usize::from(name.text_range().start()));
        let lower = name.text().to_ascii_lowercase();
        match lower.as_str() {
            ".macro" => open.push((".endm", line)),
            ".rept" | ".rep" | ".irp" | ".irpc" => open.push((".endr", line)),
            l if l.starts_with(".if") => open.push((".endif", line)),
            ".endm" | ".endmacro" | ".endr" | ".endif" => {
                let closer = if lower == ".endmacro" {
                    ".endm"
                } else {
                    lower.as_str()
                };
                if let Some(i) = open.iter().rposition(|(c, _)| *c == closer) {
                    let (_, start) = open.remove(i);
                    if line > start {
                        out.push((start, line));
                    }
                }
            }
            _ => {}
        }
    }
    // The code under each label, up to the line before the next.
    for item in outline(analysis) {
        if item.kind == SymbolKind::Label {
            let start = line_of(item.range.start);
            let end = line_of(item.range.end.saturating_sub(1));
            if end > start {
                out.push((start, end));
            }
        }
    }
    // Runs of full-line comments.
    let mut run: Option<(u32, u32)> = None;
    for (line, content) in text.lines().enumerate() {
        let line = u32::try_from(line).unwrap_or(u32::MAX);
        let trimmed = content.trim_start();
        let comment =
            trimmed.starts_with("//") || trimmed.starts_with(';') || trimmed.starts_with('#');
        match (comment, run) {
            (true, Some((start, _))) => run = Some((start, line)),
            (true, None) => run = Some((line, line)),
            (false, Some((start, end))) => {
                if end > start {
                    out.push((start, end));
                }
                run = None;
            }
            (false, None) => {}
        }
    }
    if let Some((start, end)) = run.filter(|(s, e)| e > s) {
        out.push((start, end));
    }
    out.sort_unstable();
    out
}

/// Kinds of highlighted token, in the order of the legend `armls` sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    /// Mnemonics.
    Keyword,
    /// Directives and macro calls.
    Macro,
    /// Registers and constants.
    Variable,
    /// Labels and other symbols.
    Function,
    /// Register aliases.
    Parameter,
    Comment,
    Number,
    String,
    /// `@PAGE` and other variants.
    Decorator,
}

pub const TOKEN_TYPES: &[TokenType] = &[
    TokenType::Keyword,
    TokenType::Macro,
    TokenType::Variable,
    TokenType::Function,
    TokenType::Parameter,
    TokenType::Comment,
    TokenType::Number,
    TokenType::String,
    TokenType::Decorator,
];

/// Modifier bits, in the order of the legend.
pub const DECLARATION: u32 = 1;
pub const READONLY: u32 = 2;
pub const DEFAULT_LIBRARY: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightToken {
    /// Never spans a line break.
    pub range: Range<usize>,
    pub kind: TokenType,
    pub modifiers: u32,
}

/// Every token worth highlighting, in order. Multi-line comments are split
/// into one token per line, as LSP requires.
pub fn semantic_tokens(analysis: &Analysis) -> Vec<HighlightToken> {
    let text = analysis.text();
    let mut out = Vec::new();
    for element in analysis.parse().syntax().descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        let range = token_range(&token);
        let classified = match token.kind() {
            SyntaxKind::Comment => Some((TokenType::Comment, 0)),
            SyntaxKind::Number | SyntaxKind::CharLit => Some((TokenType::Number, 0)),
            SyntaxKind::LocalLabelRef => Some((TokenType::Function, 0)),
            SyntaxKind::StringLit => Some(string_or_symbol(analysis, &token)),
            SyntaxKind::Ident => classify_ident(analysis, &token),
            _ => None,
        };
        let Some((kind, modifiers)) = classified else {
            continue;
        };
        // Split at line breaks.
        let mut start = range.start;
        for piece in text[range.clone()].split_inclusive('\n') {
            let end = start + piece.trim_end_matches(['\n', '\r']).len();
            if end > start {
                out.push(HighlightToken {
                    range: start..end,
                    kind,
                    modifiers,
                });
            }
            start += piece.len();
        }
    }
    out
}

fn string_or_symbol(analysis: &Analysis, token: &SyntaxToken) -> (TokenType, u32) {
    // A quoted label or symbol, like `"_f g":`, is a name, not a string.
    let start = token_range(token).start;
    match analysis
        .symbols()
        .iter()
        .find(|s| s.origin.is_none() && s.range.start == start)
    {
        Some(_) => (TokenType::Function, DECLARATION),
        None if analysis.reference_at(start).is_some() => (TokenType::Function, 0),
        None => (TokenType::String, 0),
    }
}

fn classify_ident(analysis: &Analysis, token: &SyntaxToken) -> Option<(TokenType, u32)> {
    let parent = token.parent()?;
    let start = token_range(token).start;
    let first = parent
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|t| !t.kind().is_trivia())
        .is_some_and(|t| t == *token);
    match parent.kind() {
        SyntaxKind::Instruction if first => return Some((TokenType::Keyword, 0)),
        SyntaxKind::Directive if first => return Some((TokenType::Macro, 0)),
        SyntaxKind::MacroCall if first => return Some((TokenType::Macro, 0)),
        SyntaxKind::RegisterAlias if !first => return Some((TokenType::Macro, 0)),
        SyntaxKind::Variant => return Some((TokenType::Decorator, 0)),
        _ => {}
    }
    if let Some(symbol) = analysis
        .symbols()
        .iter()
        .find(|s| s.origin.is_none() && s.range.start == start)
    {
        return Some((
            symbol_type(symbol.kind),
            DECLARATION | symbol_modifiers(symbol.kind),
        ));
    }
    if let Some(symbol) = analysis
        .reference_at(start)
        .filter(|r| r.range.start == start)
        .and_then(|r| r.symbol)
    {
        let kind = analysis.symbols()[symbol].kind;
        return Some((symbol_type(kind), symbol_modifiers(kind)));
    }
    if is_register(token.text()) {
        return Some((TokenType::Variable, DEFAULT_LIBRARY));
    }
    if parent.kind() == SyntaxKind::NameRef {
        return Some((TokenType::Function, 0));
    }
    None
}

fn symbol_type(kind: SymbolKind) -> TokenType {
    match kind {
        SymbolKind::Label | SymbolKind::LocalLabel => TokenType::Function,
        SymbolKind::Constant => TokenType::Variable,
        SymbolKind::RegisterAlias => TokenType::Parameter,
        SymbolKind::Macro => TokenType::Macro,
    }
}

fn symbol_modifiers(kind: SymbolKind) -> u32 {
    if kind == SymbolKind::Constant {
        READONLY
    } else {
        0
    }
}

/// The name-like token at or just before `offset`.
fn word_at(analysis: &Analysis, offset: usize) -> Option<SyntaxToken> {
    let at = u32::try_from(offset.min(analysis.text().len())).ok()?;
    let tokens = analysis.parse().syntax().token_at_offset(at.into());
    let is_word = |t: &SyntaxToken| {
        matches!(
            t.kind(),
            SyntaxKind::Ident
                | SyntaxKind::LocalLabelRef
                | SyntaxKind::Number
                | SyntaxKind::StringLit
        )
    };
    tokens
        .clone()
        .find(is_word)
        .or_else(|| tokens.last().filter(is_word))
}

fn token_range(token: &SyntaxToken) -> Range<usize> {
    let r: TextRange = token.text_range();
    usize::from(r.start())..usize::from(r.end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, needle: &str) -> usize {
        text.find(needle).unwrap_or_else(|| panic!("no {needle:?}"))
    }

    fn hover_text(text: &str, needle: &str) -> String {
        let analysis = Analysis::new(text);
        hover(&analysis, at(text, needle) + 1)
            .map(|h| h.markdown)
            .unwrap_or_default()
    }

    #[test]
    fn hover_on_a_mnemonic_lists_its_forms() {
        let text = hover_text("add x0, x0, #1", "add");
        assert!(text.starts_with("**add**"), "{text}");
        assert!(
            text.contains("add Xn|SP, Xn|SP, #imm12{, lsl #12}"),
            "{text}"
        );
    }

    #[test]
    fn hover_shows_required_features() {
        let text = hover_text("abs x0, x1", "abs");
        assert!(text.contains("needs cssc"), "{text}");
    }

    #[test]
    fn hover_on_a_condition_branch() {
        let text = hover_text("b.eq _f", "b.eq");
        assert!(text.contains("b.<cond> <label>"), "{text}");
    }

    #[test]
    fn hover_on_registers() {
        assert!(hover_text("mov x18, #1", "x18").contains("Reserved by Darwin"));
        assert!(hover_text("add v0.4s, v1.4s, v2.4s", "v1").contains("vector"));
    }

    #[test]
    fn hover_on_symbols() {
        let text = "_f: nop\nb _f\nbl _f\n";
        let hover = hover_text(text, "_f\nbl");
        assert!(hover.contains("**_f** (label, 2 uses)"), "{hover}");
        assert!(hover.contains("_f: nop"), "{hover}");
        let local = "1: nop\nb 1b\n";
        assert!(hover_text(local, "1b").contains("local label"));
    }

    #[test]
    fn hover_on_directives_and_macros() {
        assert!(hover_text(".p2align 2", ".p2align").contains("power of two"));
        let text = ".macro zero r\nmov \\r, #0\n.endm\nzero x0\n";
        let hover = hover_text(text, "zero x0");
        assert!(hover.contains("(macro, 1 use)"), "{hover}");
    }

    #[test]
    fn no_hover_on_whitespace_or_comments() {
        let analysis = Analysis::new("nop ; a comment");
        assert!(hover(&analysis, 9).is_none());
    }

    fn labels(text: &str, offset: usize) -> Vec<String> {
        completions(&Analysis::new(text), offset)
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn completes_mnemonics_and_directives_at_statement_start() {
        let all = labels("_f: ", 4);
        assert!(all.contains(&"add".to_string()));
        assert!(all.contains(&".globl".to_string()));
        let dots = labels("  .glo", 6);
        assert!(dots.contains(&".globl".to_string()));
        assert!(!dots.contains(&"add".to_string()));
    }

    #[test]
    fn completes_conditions_after_b_dot() {
        let all = labels("  b.", 4);
        assert!(all.contains(&"b.eq".to_string()));
        assert_eq!(all.len(), arm_isa::CONDITION_CODES.len());
    }

    #[test]
    fn completes_registers_and_symbols_in_operands() {
        let text = "_target: nop\nlimit = 3\nb ";
        let all = labels(text, text.len());
        assert!(all.contains(&"x0".to_string()));
        assert!(all.contains(&"_target".to_string()));
        assert!(all.contains(&"limit".to_string()));
        assert!(!all.contains(&"add".to_string()));
    }

    #[test]
    fn completes_macros_as_instructions() {
        let text = ".macro push2 a, b\n.endm\n";
        assert!(labels(text, text.len()).contains(&"push2".to_string()));
    }

    #[test]
    fn no_completions_in_comments() {
        let text = "nop ; ad";
        assert!(labels(text, text.len()).is_empty());
    }

    #[test]
    fn definition_and_references() {
        let text = "b _f\n_f: nop\nbl _f\nbl _printf\nbl _printf\n";
        let analysis = Analysis::new(text);
        let use_ = at(text, "_f\n");
        assert_eq!(definition(&analysis, use_).map(|d| d.range), Some(5..7));
        assert_eq!(references(&analysis, use_, false).len(), 2);
        assert_eq!(references(&analysis, use_, true).len(), 3);
        // An external symbol's uses are found by name.
        assert_eq!(references(&analysis, at(text, "_printf"), false).len(), 2);
        // Registers aren't symbols.
        assert!(references(&Analysis::new("mov x0, x0"), 5, false).is_empty());
    }

    #[test]
    fn rename_changes_every_use() {
        let text = "_f: nop\nb _f\nbl _f\n";
        let analysis = Analysis::new(text);
        let edits = rename(&analysis, 1, "_g").expect("renames");
        let mut out = text.to_string();
        for (range, new) in edits.iter().rev() {
            out.replace_range(range.clone(), new);
        }
        assert_eq!(out, "_g: nop\nb _g\nbl _g\n");
    }

    #[test]
    fn rename_refuses_bad_names() {
        let analysis = Analysis::new("_f: nop\n_g: nop\nb _f\n1: b 1b\n");
        assert!(rename(&analysis, 1, "9lives").is_err());
        assert!(rename(&analysis, 1, "x0").is_err());
        assert!(rename(&analysis, 1, "_g").is_err());
        assert!(prepare_rename(&analysis, 1).is_ok());
        let local = "_f: nop\n_g: nop\nb _f\n1: b 1b\n".find("1b").expect("1b");
        assert!(prepare_rename(&analysis, local).is_err());
        assert!(prepare_rename(&analysis, 12).is_err());
    }

    #[test]
    fn outline_spans_from_label_to_label() {
        let text = ".globl _a\n_a: nop\n1: ret\n_b: ret\n";
        let items = outline(&Analysis::new(text));
        let names: Vec<_> = items
            .iter()
            .map(|i| (i.name.as_str(), i.exported))
            .collect();
        assert_eq!(names, [("_a", true), ("_b", false)]);
        assert_eq!(items[0].range, at(text, "_a:")..at(text, "_b:"));
    }

    #[test]
    fn semantic_tokens_classify_each_word() {
        let text = "_f: add x0, x1, #4 ; hi\n.p2align 2\nadrp x0, _f@PAGE\n";
        let tokens = semantic_tokens(&Analysis::new(text));
        let kind = |word: &str| {
            let start = at(text, word);
            tokens
                .iter()
                .find(|t| t.range.start == start)
                .map(|t| t.kind)
        };
        assert_eq!(kind("_f:"), Some(TokenType::Function));
        assert_eq!(kind("add"), Some(TokenType::Keyword));
        assert_eq!(kind("x1"), Some(TokenType::Variable));
        assert_eq!(kind("4"), Some(TokenType::Number));
        assert_eq!(kind("; hi"), Some(TokenType::Comment));
        assert_eq!(kind(".p2align"), Some(TokenType::Macro));
        assert_eq!(kind("PAGE"), Some(TokenType::Decorator));
    }

    #[test]
    fn folding_blocks_labels_and_comments() {
        let text = "// one\n// two\n.macro m\n  nop\n.endm\n_f:\n  nop\n  ret\n_g:\n  ret\n";
        let ranges = folding_ranges(&Analysis::new(text));
        assert_eq!(ranges, [(0, 1), (2, 4), (5, 7), (8, 9)]);
    }

    #[test]
    fn semantic_tokens_never_span_lines() {
        let text = "/* one\ntwo */ nop\n";
        let tokens = semantic_tokens(&Analysis::new(text));
        let comments: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenType::Comment)
            .map(|t| &text[t.range.clone()])
            .collect();
        assert_eq!(comments, ["/* one", "two */"]);
    }
}
