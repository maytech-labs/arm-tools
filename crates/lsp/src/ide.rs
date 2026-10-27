//! Editor features over one analyzed file, in byte offsets. The protocol
//! layer in `server.rs` converts positions and types; everything here can be
//! tested without LSP.

use std::collections::BTreeSet;
use std::ops::Range;

use arm_isa::{ClassKind, Form, Mnemonic, OperandClass, Register, RegisterKind, Special, Syntax};
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
/// statement starts, and in operands what the instruction's forms accept
/// there (docs/decisions/0030-operand-aware-editing.md). `globals` are labels
/// other files in the project define.
pub fn completions(analysis: &Analysis, offset: usize, globals: &[String]) -> Vec<Completion> {
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
                let detail = arm_isa::forms(mnemonic).next().map(Form::signature);
                out.push(completion(
                    mnemonic.to_string(),
                    CompletionKind::Instruction,
                    detail.as_deref(),
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
    if let Some((mnemonic, operands)) = before.split_once(char::is_whitespace) {
        let candidates = candidates(analysis, mnemonic, operands);
        let out = operand_completions(analysis, offset, &candidates, globals);
        if !out.is_empty() {
            return out;
        }
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

/// The forms the operands typed so far could begin, each with the index of
/// the class the operand at the cursor goes in.
fn candidates(analysis: &Analysis, mnemonic: &str, operands: &str) -> Vec<(Form, usize)> {
    if !arm_isa::is_instruction(mnemonic) {
        return Vec::new();
    }
    let Some(typed) = analysis.partial_operands(operands) else {
        return Vec::new();
    };
    arm_isa::match_prefix(&Mnemonic::parse(mnemonic), &typed)
}

/// The class the operand at the cursor must fit: the one after `[` if the
/// operand starts with it.
fn expected_class(form: Form, next: usize) -> Option<OperandClass> {
    let mut classes = form.operands().skip(next);
    let class = classes.next()?;
    if class.kind() == ClassKind::Token && class.name() == "[" {
        return classes.next();
    }
    Some(class)
}

/// What fits the operand at the cursor, in each candidate form.
fn operand_completions(
    analysis: &Analysis,
    offset: usize,
    candidates: &[(Form, usize)],
    globals: &[String],
) -> Vec<Completion> {
    let mut classes: Vec<OperandClass> = Vec::new();
    for &(form, next) in candidates {
        if let Some(class) = expected_class(form, next)
            && !classes.contains(&class)
        {
            classes.push(class);
        }
    }
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut push = |out: &mut Vec<Completion>, label: String, kind, detail: Option<String>| {
        if seen.insert(label.clone()) {
            out.push(Completion {
                label,
                kind,
                detail,
            });
        }
    };
    let mut registers = false;
    for class in classes {
        let expects = expects(class);
        for name in &expects.registers {
            push(&mut out, name.clone(), CompletionKind::Register, None);
        }
        registers |= !expects.registers.is_empty();
        for keyword in expects.keywords {
            push(&mut out, keyword, CompletionKind::Condition, None);
        }
        if let Some(suffix) = expects.labels {
            let locals = analysis
                .symbols()
                .iter()
                .filter(|s| s.kind == SymbolKind::Label)
                .map(|s| s.name.clone());
            for name in locals.chain(globals.iter().cloned()) {
                push(
                    &mut out,
                    format!("{name}{suffix}"),
                    CompletionKind::Label,
                    None,
                );
            }
        }
        if expects.constants {
            for symbol in analysis
                .symbols()
                .iter()
                .filter(|s| s.kind == SymbolKind::Constant)
            {
                let detail = analysis
                    .constant_value(&symbol.name)
                    .map(|v| format!("= {v}"));
                push(
                    &mut out,
                    symbol.name.clone(),
                    CompletionKind::Constant,
                    detail,
                );
            }
        }
    }
    if registers {
        for symbol in analysis.symbols().iter().filter(|s| {
            s.kind == SymbolKind::RegisterAlias && s.scope_end.is_none_or(|end| offset < end)
        }) {
            push(
                &mut out,
                symbol.name.clone(),
                CompletionKind::RegisterAlias,
                Some("register alias".to_string()),
            );
        }
    }
    out
}

/// What an operand class accepts, for completion.
#[derive(Debug, Default)]
struct Expects {
    registers: Vec<String>,
    /// Condition codes, barrier options, shifts, and other names.
    keywords: Vec<String>,
    /// Labels, with what to write after them: `@PAGE`.
    labels: Option<&'static str>,
    constants: bool,
}

fn expects(class: OperandClass) -> Expects {
    let mut out = Expects::default();
    let words = |list: &[&str]| list.iter().map(|w| w.to_string()).collect();
    match class.kind() {
        ClassKind::Register => out.registers = registers_in(class),
        ClassKind::Token => {
            let name = class.name();
            if name.chars().all(|c| c.is_ascii_alphabetic()) {
                out.keywords = vec![name.to_string()];
            }
        }
        _ => {}
    }
    if class.kind() != ClassKind::Custom {
        return out;
    }
    let predicate = class.predicate();
    let (family, args) = predicate.split_once('<').unwrap_or((predicate, ""));
    // `isGPR64<AArch64::FPR128RegClassID>`: a register class by name.
    let named = args.split([',', '>']).find_map(|a| {
        a.trim()
            .strip_prefix("AArch64::")?
            .strip_suffix("RegClassID")
    });
    match family {
        "isGPR64" | "isGPR64WithShiftExtend" => {
            if let Some(class) = named.and_then(arm_isa::operand_class) {
                out.registers = registers_in(class);
            }
        }
        "isGPR32as64" => out.registers = registers_named("GPR32"),
        "isGPR64as32" => out.registers = registers_named("GPR64"),
        "isNeonVectorReg" => out.registers = numbered("v", 0..32),
        "isNeonVectorRegLo" => out.registers = numbered("v", 0..16),
        "isNeonVectorReg0to7" => out.registers = numbered("v", 0..8),
        "isSVEDataVectorRegOfWidth" | "isSVEDataVectorRegWithShiftExtend" | "isFPRasZPR" => {
            out.registers = numbered("z", 0..32);
        }
        "isSVEPredicateVectorRegOfWidth" | "isSVEPredicateOrPredicateAsCounterRegOfWidth" => {
            out.registers = if named == Some("PPR_3b") {
                numbered("p", 0..8)
            } else {
                numbered("p", 0..16)
            };
        }
        "isSVEPredicateAsCounterRegOfWidth" => {
            out.registers = if named == Some("PNR_p8to15") {
                numbered("pn", 8..16)
            } else {
                numbered("pn", 0..16)
            };
        }
        "isCondCode" => out.keywords = words(arm_isa::CONDITION_CODES),
        "isBarrier" => out.keywords = words(arm_isa::BARRIER_OPTIONS),
        "isPrefetch" => out.keywords = arm_isa::prefetch_operations(),
        "isSVEPattern" => out.keywords = words(arm_isa::SVE_PATTERNS),
        "isArithmeticShifter" => out.keywords = words(&["lsl", "lsr", "asr"]),
        "isLogicalShifter" => out.keywords = words(&["lsl", "lsr", "asr", "ror"]),
        "isMovImm32Shifter"
        | "isMovImm64Shifter"
        | "isLogicalVecShifter"
        | "isLogicalVecHalfWordShifter" => out.keywords = words(&["lsl"]),
        "isMoveVecShifter" => out.keywords = words(&["msl"]),
        "isExtend" | "isExtend64" | "isExtendLSL64" => {
            out.keywords = words(&[
                "uxtb", "uxth", "uxtw", "uxtx", "sxtb", "sxth", "sxtw", "sxtx", "lsl",
            ]);
        }
        "isMemWExtend" => out.keywords = words(&["uxtw", "sxtw"]),
        "isMemXExtend" => out.keywords = words(&["lsl", "sxtx"]),
        "isBranchTarget" | "isAdrLabel" | "isPAuthPCRelLabel16Operand" => out.labels = Some(""),
        "isAdrpLabel" => out.labels = Some("@PAGE"),
        "isUImm12Offset" | "isAddSubImm" => {
            // `ldr x0, [x0, _msg@PAGEOFF]` and `add x0, x0, _msg@PAGEOFF`.
            out.labels = Some("@PAGEOFF");
            out.constants = true;
        }
        _ => {
            out.constants = family.contains("Imm") || family == "isImm";
        }
    }
    out
}

/// Registers that fit a class, by their usual names: no `x31` or `w31`.
fn registers_in(class: OperandClass) -> Vec<String> {
    let mut names: Vec<&str> = Register::all()
        .filter(|r| r.kind() == RegisterKind::Scalar && r.fits(class))
        .map(Register::name)
        .filter(|n| !n.ends_with("31") || n.starts_with(['b', 'h', 's', 'd', 'q']))
        .collect();
    // x0, x1, ..., x30, then sp and the like.
    names.sort_by_key(|n| {
        let digits = n.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        match digits.parse::<u32>() {
            Ok(number) if !digits.is_empty() => (0, n.len() - digits.len(), number, *n),
            _ => (1, 0, 0, *n),
        }
    });
    names.into_iter().map(String::from).collect()
}

fn registers_named(class: &str) -> Vec<String> {
    arm_isa::operand_class(class).map_or_else(Vec::new, registers_in)
}

fn numbered(prefix: &str, range: Range<u32>) -> Vec<String> {
    range.map(|n| format!("{prefix}{n}")).collect()
}

/// The signatures of the instruction at the cursor, for signature help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelp {
    /// Each signature, with the operand the cursor is in, if it's one of them.
    pub signatures: Vec<(arm_isa::Signature, Option<usize>)>,
}

/// Most forms a signature help shows; `mov` alone has dozens.
const MAX_SIGNATURES: usize = 30;

/// Signature help in an instruction's operands: the forms that the operands
/// typed so far could begin, with the operand at the cursor. All forms if
/// none fit, so a mistake doesn't hide what's expected.
pub fn signature_help(analysis: &Analysis, offset: usize) -> Option<SignatureHelp> {
    let text = analysis.text();
    let offset = offset.min(text.len());
    if in_comment_or_string(analysis, offset) {
        return None;
    }
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let before = statement_prefix(&text[line_start..offset]);
    let (mnemonic, operands) = before.split_once(char::is_whitespace)?;
    if !arm_isa::is_instruction(mnemonic) {
        return None;
    }
    let mut found: Vec<(arm_isa::Signature, Option<usize>)> =
        candidates(analysis, mnemonic, operands)
            .into_iter()
            .map(|(form, next)| {
                let parts = form.signature_parts();
                let active = parts.param_of.get(next).copied().flatten();
                (parts, active)
            })
            .collect();
    if found.is_empty() {
        // Count the commas outside `{ }` lists.
        let mut depth = 0i32;
        let mut commas = 0;
        for c in operands.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth <= 0 => commas += 1,
                _ => {}
            }
        }
        let head = Mnemonic::parse(mnemonic).head;
        found = arm_isa::forms(&head)
            .filter(|f| f.syntax() == Syntax::Generic)
            .map(|form| {
                let parts = form.signature_parts();
                let active = (commas < parts.params.len()).then_some(commas);
                (parts, active)
            })
            .collect();
    }
    let mut seen = BTreeSet::new();
    found.retain(|(parts, _)| seen.insert(parts.label()));
    found.truncate(MAX_SIGNATURES);
    (!found.is_empty()).then_some(SignatureHelp { signatures: found })
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

/// Inlay hints as (byte offset, label): the value after each use of a
/// constant, and the system call a `mov x16, #n` sets up when an `svc`
/// follows.
pub fn inlay_hints(analysis: &Analysis) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for reference in analysis.references() {
        let Some(symbol) = reference.symbol.map(|s| &analysis.symbols()[s]) else {
            continue;
        };
        if symbol.kind != SymbolKind::Constant {
            continue;
        }
        if let Some(value) = analysis.constant_value(&symbol.name) {
            let label = if (0..16).contains(&value) {
                format!("= {value}")
            } else {
                format!("= {value} ({value:#x})")
            };
            out.push((reference.range.end, label));
        }
    }
    let items = analysis.items();
    for (i, item) in items.iter().enumerate() {
        let arm_sema::Item::Instruction(insn) = item else {
            continue;
        };
        let Some(ops) = &insn.operands else {
            continue;
        };
        let is_x16 = matches!(ops.first(), Some(arm_isa::Operand::Register(r))
            if matches!(r.register.llvm_name(), "X16" | "W16"));
        let number = match ops.get(1) {
            Some(arm_isa::Operand::Imm(arm_isa::Imm::Constant(n))) => Some(*n),
            _ => None,
        };
        let svc_follows = items[i + 1..].iter().take(8).any(
            |item| matches!(item, arm_sema::Item::Instruction(next) if next.mnemonic.head == "svc"),
        );
        let name = number.and_then(arm_isa::syscall_name);
        if let (true, "mov" | "movz", true, Some(name)) =
            (is_x16, insn.mnemonic.head.as_str(), svc_follows, name)
        {
            out.push((insn.range.end, format!("syscall {name}")));
        }
    }
    out.sort_by_key(|(offset, _)| *offset);
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
        completions(&Analysis::new(text), offset, &[])
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
        let text = "_target: nop\nlimit = 3\nmov x0, ";
        let all = labels(text, text.len());
        assert!(all.contains(&"x1".to_string()));
        assert!(all.contains(&"limit".to_string()));
        assert!(!all.contains(&"add".to_string()));
        // A branch takes a label, not a register.
        let text = "_target: nop\nb ";
        let all = labels(text, text.len());
        assert!(all.contains(&"_target".to_string()));
        assert!(!all.contains(&"x0".to_string()));
    }

    fn operand_labels(text: &str, globals: &[&str]) -> Vec<String> {
        let globals: Vec<String> = globals.iter().map(|g| g.to_string()).collect();
        completions(&Analysis::new(text), text.len(), &globals)
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    fn has(all: &[String], wanted: &[&str]) -> bool {
        wanted.iter().all(|w| all.iter().any(|a| a == w))
    }

    fn lacks(all: &[String], unwanted: &[&str]) -> bool {
        unwanted.iter().all(|w| !all.iter().any(|a| a == w))
    }

    #[test]
    fn operand_completion_follows_the_forms() {
        // A W destination means W sources.
        let w = operand_labels("  add w0, ", &[]);
        assert!(has(&w, &["w1", "wsp"]) && lacks(&w, &["x1", "v0"]), "{w:?}");
        // Registers come in numeric order.
        let x = operand_labels("  ret ", &[]);
        let at = |n: &str| x.iter().position(|l| l == n);
        assert!(at("x2") < at("x10"), "{x:?}");
        assert!(lacks(&x, &["x31"]));
        // The base of an address can be sp, and is never a W register.
        let base = operand_labels("  ldr x0, [", &[]);
        assert!(
            has(&base, &["x1", "sp"]) && lacks(&base, &["w1", "_x@PAGEOFF"]),
            "{base:?}"
        );
        // After the base: an offset, a page offset, or an index register.
        let text = "size = 8\n_msg: nop\n  ldr x0, [x1, ";
        let offset = operand_labels(text, &[]);
        assert!(has(&offset, &["size", "_msg@PAGEOFF", "x2"]), "{offset:?}");
        let conds = operand_labels("  csel x0, x1, x2, ", &[]);
        assert!(
            has(&conds, &["eq", "ne", "hs"]) && lacks(&conds, &["x0"]),
            "{conds:?}"
        );
        let shifts = operand_labels("  add x0, x1, x2, ", &[]);
        assert!(
            has(&shifts, &["lsl", "asr"]) && lacks(&shifts, &["ror"]),
            "{shifts:?}"
        );
        let extends = operand_labels("  ldr x0, [x1, w2, ", &[]);
        assert!(
            has(&extends, &["uxtw", "sxtw"]) && lacks(&extends, &["lsl"]),
            "{extends:?}"
        );
    }

    #[test]
    fn operand_completion_for_names_and_vectors() {
        let barriers = operand_labels("  dmb ", &[]);
        assert!(
            has(&barriers, &["ish", "sy"]) && lacks(&barriers, &["x0"]),
            "{barriers:?}"
        );
        let prefetch = operand_labels("  prfm ", &[]);
        assert!(has(&prefetch, &["pldl1keep", "pstl2strm"]), "{prefetch:?}");
        let vectors = operand_labels("  fadd v0.4s, ", &[]);
        assert!(
            has(&vectors, &["v1", "v31"]) && lacks(&vectors, &["x0"]),
            "{vectors:?}"
        );
        let scalars = operand_labels("  fadd d0, ", &[]);
        assert!(
            has(&scalars, &["d1", "d31"]) && lacks(&scalars, &["v1"]),
            "{scalars:?}"
        );
        let sve = operand_labels("  ptrue p0.s, ", &[]);
        assert!(has(&sve, &["all", "vl4"]), "{sve:?}");
        let preds = operand_labels("  add z0.s, ", &[]);
        assert!(has(&preds, &["z1", "p0"]), "{preds:?}");
    }

    #[test]
    fn operand_completion_for_labels() {
        let text = "_local: nop\nlimit = 3\n  bl ";
        let calls = operand_labels(text, &["_helper"]);
        assert!(has(&calls, &["_local", "_helper"]), "{calls:?}");
        assert!(lacks(&calls, &["x0", "limit"]), "{calls:?}");
        let page = operand_labels("_msg: nop\n  adrp x0, ", &[]);
        assert!(has(&page, &["_msg@PAGE"]), "{page:?}");
        let imm = operand_labels("limit = 3\n  mov x0, ", &[]);
        assert!(has(&imm, &["limit", "x1"]), "{imm:?}");
        let detail = completions(&Analysis::new("limit = 3\n  cmp x0, "), 21, &[])
            .into_iter()
            .find(|c| c.label == "limit")
            .and_then(|c| c.detail);
        assert_eq!(detail.as_deref(), Some("= 3"));
        // Aliases go where registers do.
        let alias = operand_labels("tmp .req x9\n  add x0, ", &[]);
        assert!(has(&alias, &["tmp", "x1"]), "{alias:?}");
    }

    #[test]
    fn operand_completion_falls_back() {
        // Nothing fits: every register and symbol, as before.
        let bad = operand_labels("_f: nop\n  add x0, v1.4b, ", &[]);
        assert!(has(&bad, &["x0", "v0", "_f"]), "{bad:?}");
        // Macros aren't instructions.
        let text = ".macro m a\n.endm\n  m ";
        assert!(has(&operand_labels(text, &[]), &["x0"]));
    }

    #[test]
    fn mnemonics_show_a_signature() {
        let all = completions(&Analysis::new("  "), 2, &[]);
        let ldp = all
            .iter()
            .find(|c| c.label == "ldp")
            .and_then(|c| c.detail.clone());
        assert!(ldp.is_some_and(|d| d.starts_with("ldp ")));
    }

    fn signature(text: &str) -> Option<(Vec<String>, Vec<Option<usize>>)> {
        let help = signature_help(&Analysis::new(text), text.len())?;
        Some(
            help.signatures
                .into_iter()
                .map(|(parts, active)| (parts.label(), active))
                .unzip(),
        )
    }

    #[test]
    fn signature_help_tracks_the_operand() {
        let (labels, active) = signature("  ldp x0, x1, [").expect("help");
        let pre = labels
            .iter()
            .position(|l| l == "ldp Xn, Xn, [Xn|SP, #-512..504 (×8)]!")
            .expect("pre-index form");
        assert_eq!(active[pre], Some(2));
        let (labels, active) = signature("  ldp x0, x1, [sp, ").expect("help");
        let at = labels
            .iter()
            .position(|l| l.ends_with("]!"))
            .expect("pre-index form");
        assert_eq!(active[at], Some(3));
        // W operands narrow add to its W forms.
        let (labels, active) = signature("  add w0, ").expect("help");
        assert!(labels.iter().all(|l| !l.contains("Xn")), "{labels:?}");
        assert!(active.iter().all(|a| *a == Some(1)));
        let (labels, active) = signature("  b.eq ").expect("help");
        assert_eq!(
            (labels, active),
            (vec!["b.<cond> <label>".to_string()], vec![Some(0)])
        );
        // `ret` alone has no operand at the cursor.
        let (labels, active) = signature("  ret x1").expect("help");
        assert_eq!(labels, ["ret", "ret Xn"]);
        assert_eq!(active, [None, Some(0)]);
    }

    #[test]
    fn signature_help_falls_back_to_every_form() {
        // Nothing fits: all of add's forms, by comma count.
        let (labels, active) = signature("  add x0, v1.4b, ").expect("help");
        assert!(
            labels.len() > 3 && labels.len() <= MAX_SIGNATURES,
            "{}",
            labels.len()
        );
        assert!(active.iter().all(|a| a.is_none_or(|a| a == 2)));
        // SYS aliases are C++ in clang, so there are no forms to show.
        assert!(signature("  dc ").is_none());
        assert!(signature("  ldr x0, [x1] ; a comment ").is_none());
        assert!(signature("  .quad ").is_none());
        assert!(signature("  ldr").is_none());
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
    fn inlay_hints_show_values_and_syscalls() {
        let text = "LIMIT = 4095\nsmall = 3\n_f:\n    add x0, x0, #LIMIT\n    mov x1, #small\n    mov x16, #4\n    svc #0x80\n";
        let hints = inlay_hints(&Analysis::new(text));
        let labels: Vec<(&str, &str)> = hints
            .iter()
            .map(|(at, label)| (&text[..*at], label.as_str()))
            .map(|(before, label)| (before.lines().last().unwrap_or(""), label))
            .collect();
        assert_eq!(
            labels,
            [
                ("    add x0, x0, #LIMIT", "= 4095 (0xfff)"),
                ("    mov x1, #small", "= 3"),
                ("    mov x16, #4", "syscall write"),
            ]
        );
        // Redefined constants have no single value; x16 without an svc isn't a syscall.
        let text = ".set a, 1\n.set a, 2\nmov x0, #a\nmov x16, #4\nret\n";
        assert!(inlay_hints(&Analysis::new(text)).is_empty());
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
