//! Turning an instruction's operands into clang's operand model, so the
//! matcher in `arm-isa` can check them (docs/decisions/0017-operand-matching.md).

use std::collections::HashMap;
use std::ops::Range;

use arm_isa::{Imm, Operand, Register, RegisterKind, RegisterOperand};
use arm_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, ast};

/// What names mean at a point in the file.
pub(crate) struct Scope<'a> {
    /// Constants defined above, with their values.
    pub constants: &'a HashMap<String, i64>,
    /// Register aliases in scope: lowercase alias to register text.
    pub aliases: &'a HashMap<String, String>,
}

/// The operands, each with the byte range it came from.
pub(crate) struct Flattened {
    pub operands: Vec<Operand>,
    pub ranges: Vec<Range<usize>>,
}

/// Flattens an instruction's operands, or None if any can't be judged
/// (macro arguments, literal pool loads, SME arrays, parse errors).
pub(crate) fn flatten(insn: &ast::Instruction, scope: &Scope) -> Option<Flattened> {
    let mut out = Flattened {
        operands: Vec::new(),
        ranges: Vec::new(),
    };
    for operand in insn.operands() {
        flatten_operand(&operand, scope, &mut out)?;
    }
    Some(out)
}

/// Whether an operand is where `b`, `bl`, `b.cond`, `adr`, or `adrp` takes
/// its label: the first operand of a branch, the second of adr and adrp.
/// Clang reads names there as symbols, even `lsl` or `uxtw`. (`cbz x0, uxtw`
/// stays an error, as it is for clang.)
fn label_position(node: &SyntaxNode) -> bool {
    let Some(list) = node
        .parent()
        .filter(|p| p.kind() == SyntaxKind::OperandList)
    else {
        return false;
    };
    let Some(mnemonic) = list
        .parent()
        .and_then(|insn| insn.first_token())
        .map(|t| arm_isa::Mnemonic::parse(t.text()).head)
    else {
        return false;
    };
    let index = list.children().position(|c| c == *node);
    match mnemonic.as_str() {
        "b" | "bl" => index == Some(0),
        "adr" | "adrp" => index == Some(1),
        _ => false,
    }
}

/// An SME array or tile slice: `za.d[w8, 0, vgx2]` or `za0h.s[w12, 3]`.
/// Clang reads the matrix, `[`, the index register, the offset, and `]`;
/// it drops `vgx2` and `vgx4`, which the instruction already implies.
/// Offset ranges (`0:1`) aren't modeled, so they aren't judged.
fn matrix_index(
    node: &SyntaxNode,
    matrix: RegisterOperand,
    scope: &Scope,
    out: &mut Flattened,
) -> Option<()> {
    let parts: Vec<SyntaxNode> = node
        .children()
        .filter(|c| {
            !(c.kind() == SyntaxKind::NameRef
                && first_token(c).is_some_and(|t| {
                    matches!(t.text().to_ascii_lowercase().as_str(), "vgx2" | "vgx4")
                }))
        })
        .collect();
    // `0:3` is a range: the colon sits between the two numbers, which clang
    // wants written as integers.
    let range = node
        .children_with_tokens()
        .any(|t| t.kind() == SyntaxKind::Colon);
    let offset = match (range, parts.as_slice()) {
        (false, [_, offset]) => match eval(offset, scope) {
            Value::Int(v) => Operand::Imm(Imm::Constant(v)),
            _ => return None,
        },
        (true, [_, first, last])
            if first.kind() == SyntaxKind::Literal && last.kind() == SyntaxKind::Literal =>
        {
            match (eval(first, scope), eval(last, scope)) {
                (Value::Int(first), Value::Int(last)) => Operand::ImmRange { first, last },
                _ => return None,
            }
        }
        _ => return None,
    };
    let index = register(&first_token(&parts[0])?.text().to_ascii_lowercase())?;
    push(out, Operand::Register(matrix), node);
    push(out, Operand::Token("[".into()), node);
    push(out, Operand::Register(index), node);
    push(out, offset, node);
    push(out, Operand::Token("]".into()), node);
    Some(())
}

fn push(out: &mut Flattened, operand: Operand, node: &SyntaxNode) {
    out.operands.push(operand);
    out.ranges.push(node_range(node));
}

fn flatten_operand(node: &SyntaxNode, scope: &Scope, out: &mut Flattened) -> Option<()> {
    match node.kind() {
        SyntaxKind::NameRef => {
            let name_ref = ast::NameRef::cast(node.clone())?;
            let name = name_ref.name()?;
            if name_ref.variant().is_some() || name.kind() == SyntaxKind::StringLit {
                push(out, Operand::Imm(imm(eval(node, scope))), node);
                return Some(());
            }
            let text = name.text();
            // A shift or extend without an amount: `[x0, w1, sxtw]`. Where a
            // branch or adr takes a label, it's a symbol named that way
            // (`bl lsl`), as clang parses it.
            let lower = text.to_ascii_lowercase();
            if SHIFT_EXTENDS.contains(&lower.as_str()) && !label_position(node) {
                let operand = Operand::ShiftExtend {
                    kind: lower,
                    amount: None,
                    explicit: false,
                };
                push(out, operand, node);
                return Some(());
            }
            let target = scope
                .aliases
                .get(&text.to_ascii_lowercase())
                .map_or(text, String::as_str);
            if let Some(register) = register(target) {
                let neon_suffix = neon_suffix(&register);
                push(out, Operand::Register(register), node);
                if let Some(suffix) = neon_suffix {
                    push(out, Operand::Token(suffix), node);
                }
            } else if let Some(value) = scope.constants.get(text) {
                push(out, Operand::Imm(Imm::Constant(*value)), node);
            } else {
                push(out, Operand::Ident(text.to_string()), node);
            }
        }
        SyntaxKind::Immediate
        | SyntaxKind::Literal
        | SyntaxKind::PrefixExpr
        | SyntaxKind::ParenExpr
        | SyntaxKind::Specifier => {
            let value = match node.kind() {
                SyntaxKind::Immediate => eval(&node.first_child()?, scope),
                _ => eval(node, scope),
            };
            push(out, value_operand(value), node);
        }
        SyntaxKind::BinExpr => {
            // `p0/z` is a predicate register with a qualifier, not a division.
            if let Some((register, qualifier)) = predicate_qualifier(node) {
                push(out, Operand::Register(register), node);
                push(out, Operand::Token("/".into()), node);
                push(out, Operand::Token(qualifier), node);
            } else {
                push(out, value_operand(eval(node, scope)), node);
            }
        }
        SyntaxKind::Memory => {
            push(out, Operand::Token("[".into()), node);
            for child in node.children() {
                flatten_operand(&child, scope, out)?;
            }
            if has_token(node, SyntaxKind::RBracket) {
                push(out, Operand::Token("]".into()), node);
            }
            if has_token(node, SyntaxKind::Bang) {
                push(out, Operand::Token("!".into()), node);
            }
        }
        SyntaxKind::Shift => {
            let kind = first_token(node)?.text().to_ascii_lowercase();
            let amount_node = node.first_child();
            if kind == "mul" {
                push(out, Operand::Token("mul".into()), node);
                match amount_node {
                    Some(n) if n.kind() == SyntaxKind::NameRef => {
                        let word = first_token(&n)?.text().to_ascii_lowercase();
                        push(out, Operand::Token(word), node);
                    }
                    Some(n) => flatten_operand(&n, scope, out)?,
                    None => {}
                }
                return Some(());
            }
            let amount = match amount_node {
                None => None,
                Some(n) => {
                    let value = match n.kind() {
                        SyntaxKind::Immediate => eval(&n.first_child()?, scope),
                        _ => eval(&n, scope),
                    };
                    match value {
                        Value::Int(v) => Some(v),
                        _ => return None,
                    }
                }
            };
            let explicit = amount.is_some();
            push(
                out,
                Operand::ShiftExtend {
                    kind,
                    amount,
                    explicit,
                },
                node,
            );
        }
        SyntaxKind::RegisterList => register_list(node, scope, out)?,
        SyntaxKind::Indexed => {
            let base = first_token(node)?;
            let register = register(base.text())?;
            if register.register.kind() == RegisterKind::Matrix {
                return matrix_index(node, register, scope, out);
            }
            if !matches!(
                register.register.kind(),
                RegisterKind::NeonVector
                    | RegisterKind::SveData
                    | RegisterKind::SvePredicate
                    | RegisterKind::SvePredicateAsCounter
            ) {
                return None;
            }
            let indices: Vec<SyntaxNode> = node.children().collect();
            let [index] = indices.as_slice() else {
                return None;
            };
            let Value::Int(index) = eval(index, scope) else {
                return None;
            };
            let neon_suffix = neon_suffix(&register);
            push(out, Operand::Register(register), node);
            if let Some(suffix) = neon_suffix {
                push(out, Operand::Token(suffix), node);
            }
            push(out, Operand::VectorIndex(index), node);
        }
        SyntaxKind::Writeback => {
            for child in node.children() {
                flatten_operand(&child, scope, out)?;
            }
            push(out, Operand::Token("!".into()), node);
        }
        // Literal pool loads, macro arguments, errors: clang handles these
        // outside the matcher, or they can't be judged before expansion.
        _ => return None,
    }
    Some(())
}

/// Shift and extend names, which clang reads as a shift-extend operand even
/// without an amount.
const SHIFT_EXTENDS: &[&str] = &[
    "lsl", "lsr", "asr", "ror", "msl", "uxtb", "uxth", "uxtw", "uxtx", "sxtb", "sxth", "sxtw",
    "sxtx",
];

/// `{v0.4s, v1.4s}`, `{v0.4s-v3.4s}`, `{z0.d, z8.d}`, then an optional index.
fn register_list(node: &SyntaxNode, scope: &Scope, out: &mut Flattened) -> Option<()> {
    let mut registers: Vec<RegisterOperand> = Vec::new();
    let mut count = 0;
    let mut index = None;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::NameRef => {
                let text = first_token(&child)?.text().to_string();
                let target = scope
                    .aliases
                    .get(&text.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or(text);
                registers.push(register(&target)?);
                count += 1;
            }
            SyntaxKind::RegisterRange => {
                let names: Vec<SyntaxToken> = child
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .filter(|t| t.kind() == SyntaxKind::Ident)
                    .collect();
                let [first, last] = names.as_slice() else {
                    return None;
                };
                let first = register(first.text())?;
                let last = register(last.text())?;
                let span = distance(&first.register, &last.register)?;
                registers.push(first);
                count += span + 1;
            }
            // The index after the closing brace.
            _ if index.is_none() && registers.len() == count as usize => {
                index = Some(match eval(&child, scope) {
                    Value::Int(i) => i,
                    _ => return None,
                });
            }
            _ => return None,
        }
    }
    let first = registers.first()?.clone();
    if !matches!(
        first.register.kind(),
        RegisterKind::NeonVector
            | RegisterKind::SveData
            | RegisterKind::SvePredicate
            | RegisterKind::SvePredicateAsCounter
    ) {
        return None;
    }
    let stride = match registers.get(1) {
        Some(second) if registers.len() == count as usize => {
            distance(&first.register, &second.register)?
        }
        _ => 1,
    };
    push(
        out,
        Operand::VectorList {
            first: first.register,
            count,
            stride,
            suffix: first.suffix,
        },
        node,
    );
    if let Some(index) = index {
        push(out, Operand::VectorIndex(index), node);
    }
    Some(())
}

/// A register name with an optional suffix: `x0`, `v0.4s`, `z1.d`, `za0h.s`.
pub(crate) fn register(text: &str) -> Option<RegisterOperand> {
    let pick =
        |name: &str| Register::lookup(name).find(|r| r.kind() != RegisterKind::MatrixTileList);
    if let Some(register) = pick(text) {
        return Some(RegisterOperand {
            register,
            suffix: None,
            shift: None,
        });
    }
    let (base, suffix) = text.split_once('.')?;
    let register = pick(base)?;
    if register.kind() == RegisterKind::Scalar {
        return None;
    }
    Some(RegisterOperand {
        register,
        suffix: Some(suffix.to_ascii_lowercase()),
        shift: None,
    })
}

/// Clang gives a NEON register's arrangement as a separate token too.
fn neon_suffix(register: &RegisterOperand) -> Option<String> {
    (register.register.kind() == RegisterKind::NeonVector)
        .then(|| register.suffix.as_ref().map(|s| format!(".{s}")))
        .flatten()
}

/// How many registers from `from` to `to`, wrapping around: lists may wrap
/// from the last register to the first, as in `{v31.2d, v0.2d}`. SVE
/// predicates wrap at 16; vectors at 32.
fn distance(from: &Register, to: &Register) -> Option<u32> {
    let count = match from.kind() {
        RegisterKind::SvePredicate | RegisterKind::SvePredicateAsCounter => 16,
        _ => 32,
    };
    Some((number(to)? + count - number(from)?) % count)
}

fn number(register: &Register) -> Option<u32> {
    register
        .llvm_name()
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .parse()
        .ok()
}

fn predicate_qualifier(node: &SyntaxNode) -> Option<(RegisterOperand, String)> {
    if !has_token(node, SyntaxKind::Slash) {
        return None;
    }
    let children: Vec<SyntaxNode> = node.children().collect();
    let [lhs, rhs] = children.as_slice() else {
        return None;
    };
    let register = register(first_token(lhs)?.text())?;
    if !matches!(
        register.register.kind(),
        RegisterKind::SvePredicate | RegisterKind::SvePredicateAsCounter
    ) {
        return None;
    }
    let qualifier = first_token(rhs)?.text().to_ascii_lowercase();
    matches!(qualifier.as_str(), "z" | "m").then_some((register, qualifier))
}

/// The value of an expression, as far as the assembler knows it here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Int(i64),
    Float(f64),
    Symbol {
        name: String,
        variant: Option<String>,
        specifier: Option<String>,
        addend: i64,
    },
    Unknown,
}

fn imm(value: Value) -> Imm {
    match value {
        Value::Int(v) => Imm::Constant(v),
        Value::Symbol {
            name,
            variant,
            specifier,
            addend,
        } => Imm::Symbol {
            name,
            variant,
            specifier,
            addend,
        },
        Value::Float(_) | Value::Unknown => Imm::Other,
    }
}

fn value_operand(value: Value) -> Operand {
    match value {
        Value::Float(f) => Operand::FpImm(f),
        other => Operand::Imm(imm(other)),
    }
}

/// Evaluates an expression node with Apple's semantics: 64-bit wrapping,
/// comparisons give -1 for true, `!x` gives 1 or 0, `>>` is logical.
pub(crate) fn eval(node: &SyntaxNode, scope: &Scope) -> Value {
    match node.kind() {
        SyntaxKind::Literal => first_token(node).map_or(Value::Unknown, |t| literal(&t)),
        SyntaxKind::NameRef => {
            let Some(name_ref) = ast::NameRef::cast(node.clone()) else {
                return Value::Unknown;
            };
            let Some(name) = name_ref.name() else {
                return Value::Unknown;
            };
            let text = unquote(name.text());
            let variant = name_ref.variant().map(|v| v.text().to_ascii_lowercase());
            match (variant, scope.constants.get(&text)) {
                (None, Some(value)) => Value::Int(*value),
                (variant, _) => Value::Symbol {
                    name: text,
                    variant,
                    specifier: None,
                    addend: 0,
                },
            }
        }
        SyntaxKind::ParenExpr => {
            let inner = node
                .first_child()
                .map_or(Value::Unknown, |n| eval(&n, scope));
            match (inner, variant_of(node)) {
                (
                    Value::Symbol {
                        name,
                        specifier,
                        addend,
                        ..
                    },
                    Some(variant),
                ) => Value::Symbol {
                    name,
                    variant: Some(variant),
                    specifier,
                    addend,
                },
                (inner, None) => inner,
                (_, Some(_)) => Value::Unknown,
            }
        }
        SyntaxKind::Specifier => {
            let specifier = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind() == SyntaxKind::Ident)
                .map(|t| t.text().to_ascii_lowercase());
            let inner = node.first_child();
            match inner.as_ref().map(|n| eval(n, scope)) {
                Some(Value::Symbol {
                    name,
                    variant,
                    addend,
                    ..
                }) => Value::Symbol {
                    name,
                    variant,
                    specifier,
                    addend,
                },
                // `#:abs_g0:limit` with `limit` a known constant is still a
                // relocation of the symbol, as clang reads it.
                Some(Value::Int(_)) => match inner.and_then(ast::NameRef::cast) {
                    Some(name_ref) => Value::Symbol {
                        name: name_ref
                            .name()
                            .map(|n| n.text().to_string())
                            .unwrap_or_default(),
                        variant: None,
                        specifier,
                        addend: 0,
                    },
                    None => Value::Unknown,
                },
                _ => Value::Unknown,
            }
        }
        SyntaxKind::PrefixExpr => {
            let Some(op) = first_token(node) else {
                return Value::Unknown;
            };
            let operand = node
                .first_child()
                .map_or(Value::Unknown, |n| eval(&n, scope));
            match (op.kind(), operand) {
                (SyntaxKind::Minus, Value::Int(v)) => Value::Int(v.wrapping_neg()),
                (SyntaxKind::Minus, Value::Float(f)) => Value::Float(-f),
                (SyntaxKind::Plus, v @ (Value::Int(_) | Value::Float(_))) => v,
                (SyntaxKind::Tilde, Value::Int(v)) => Value::Int(!v),
                (SyntaxKind::Bang, Value::Int(v)) => Value::Int(i64::from(v == 0)),
                _ => Value::Unknown,
            }
        }
        SyntaxKind::BinExpr => {
            let children: Vec<SyntaxNode> = node.children().collect();
            let [lhs, rhs] = children.as_slice() else {
                return Value::Unknown;
            };
            let Some(op) = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| !t.kind().is_trivia())
            else {
                return Value::Unknown;
            };
            binary(op.kind(), eval(lhs, scope), eval(rhs, scope))
        }
        _ => Value::Unknown,
    }
}

fn binary(op: SyntaxKind, lhs: Value, rhs: Value) -> Value {
    use SyntaxKind::*;
    match (lhs, rhs) {
        (Value::Int(a), Value::Int(b)) => {
            let truth = |t: bool| Value::Int(if t { -1 } else { 0 });
            match op {
                Plus => Value::Int(a.wrapping_add(b)),
                Minus => Value::Int(a.wrapping_sub(b)),
                Star => Value::Int(a.wrapping_mul(b)),
                Slash if b != 0 => Value::Int(a.wrapping_div(b)),
                Percent if b != 0 => Value::Int(a.wrapping_rem(b)),
                Amp => Value::Int(a & b),
                Pipe => Value::Int(a | b),
                Caret => Value::Int(a ^ b),
                Shl => Value::Int(a.wrapping_shl(u32::try_from(b & 63).unwrap_or(0))),
                Shr => Value::Int(((a as u64) >> (b & 63)) as i64),
                EqEq => truth(a == b),
                NotEq => truth(a != b),
                Lt => truth(a < b),
                LtEq => truth(a <= b),
                Gt => truth(a > b),
                GtEq => truth(a >= b),
                AmpAmp => Value::Int(i64::from(a != 0 && b != 0)),
                PipePipe => Value::Int(i64::from(a != 0 || b != 0)),
                _ => Value::Unknown,
            }
        }
        (
            Value::Symbol {
                name,
                variant,
                specifier,
                addend,
            },
            Value::Int(b),
        ) if matches!(op, Plus | Minus) => {
            let addend = if op == Plus {
                addend.wrapping_add(b)
            } else {
                addend.wrapping_sub(b)
            };
            Value::Symbol {
                name,
                variant,
                specifier,
                addend,
            }
        }
        (
            Value::Int(a),
            Value::Symbol {
                name,
                variant,
                specifier,
                addend,
            },
        ) if op == Plus => Value::Symbol {
            name,
            variant,
            specifier,
            addend: addend.wrapping_add(a),
        },
        _ => Value::Unknown,
    }
}

/// A number, character, or local label reference.
fn literal(token: &SyntaxToken) -> Value {
    let text = token.text();
    match token.kind() {
        SyntaxKind::Number => number_value(text),
        SyntaxKind::CharLit => {
            let inner = text.trim_start_matches('\'').trim_end_matches('\'');
            let mut chars = inner.chars();
            match (chars.next(), chars.next()) {
                (Some('\\'), Some(c)) => Value::Int(match c {
                    'n' => 10,
                    't' => 9,
                    'r' => 13,
                    '0' => 0,
                    c => c as i64,
                }),
                (Some(c), None) => Value::Int(c as i64),
                _ => Value::Unknown,
            }
        }
        SyntaxKind::LocalLabelRef => Value::Symbol {
            name: text.to_string(),
            variant: None,
            specifier: None,
            addend: 0,
        },
        _ => Value::Unknown,
    }
}

/// LLVM's integer forms: `0x1f`, `0b101`, `017` (octal), `42`; and floats.
pub(crate) fn number_value(text: &str) -> Value {
    let lower = text.to_ascii_lowercase();
    let parsed = if let Some(hex) = lower.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = lower.strip_prefix("0b") {
        u64::from_str_radix(bin, 2).ok()
    } else if lower.contains(['.', 'e']) {
        return lower.parse().map_or(Value::Unknown, Value::Float);
    } else if lower.len() > 1 && lower.starts_with('0') {
        u64::from_str_radix(&lower[1..], 8).ok()
    } else {
        lower.parse::<u64>().ok()
    };
    parsed.map_or(Value::Unknown, |v| Value::Int(v as i64))
}

fn variant_of(node: &SyntaxNode) -> Option<String> {
    let variant = node.children().find(|c| c.kind() == SyntaxKind::Variant)?;
    variant
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::Ident)
        .map(|t| t.text().to_ascii_lowercase())
}

fn first_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| !t.kind().is_trivia())
}

fn has_token(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == kind)
}

fn unquote(text: &str) -> String {
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text)
        .to_string()
}

fn node_range(node: &SyntaxNode) -> Range<usize> {
    let r = node.text_range();
    usize::from(r.start())..usize::from(r.end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(text: &str) -> Vec<Operand> {
        let parse = arm_syntax::parse(text);
        let insn = parse
            .tree()
            .statements()
            .find_map(|s| match s {
                ast::Statement::Instruction(i) => Some(i),
                _ => None,
            })
            .expect("an instruction");
        let constants = HashMap::from([("four".to_string(), 4)]);
        let aliases = HashMap::from([("tmp".to_string(), "x9".to_string())]);
        let scope = Scope {
            constants: &constants,
            aliases: &aliases,
        };
        flatten(&insn, &scope)
            .map(|f| f.operands)
            .unwrap_or_default()
    }

    fn names(ops: &[Operand]) -> Vec<String> {
        ops.iter()
            .map(|o| match o {
                Operand::Token(t) => t.clone(),
                Operand::Register(r) => r.register.name().to_string(),
                Operand::Imm(Imm::Constant(v)) => format!("#{v}"),
                Operand::Imm(Imm::Symbol { name, variant, .. }) => {
                    format!(
                        "sym:{name}{}",
                        variant
                            .as_ref()
                            .map(|v| format!("@{v}"))
                            .unwrap_or_default()
                    )
                }
                Operand::Imm(Imm::Other) => "?".into(),
                Operand::FpImm(f) => format!("#{f}f"),
                Operand::ShiftExtend { kind, amount, .. } => format!(
                    "{kind}{}",
                    amount.map(|a| format!(" #{a}")).unwrap_or_default()
                ),
                Operand::VectorList {
                    count,
                    stride,
                    suffix,
                    ..
                } => format!(
                    "list{count}/{stride}.{}",
                    suffix.clone().unwrap_or_default()
                ),
                Operand::VectorIndex(i) => format!("[{i}]"),
                Operand::ImmRange { first, last } => format!("{first}:{last}"),
                Operand::Ident(n) => format!("id:{n}"),
                Operand::CondCode(c) => format!("cc:{c}"),
                Operand::ShiftedImm { shift, .. } => format!("shifted {shift}"),
            })
            .collect()
    }

    #[test]
    fn sme_array_indexes() {
        // vgx2 is dropped, as clang drops it; a range stays a range.
        assert_eq!(
            names(&flat("add za.d[w8, 0, vgx2], {z0.d, z1.d}")),
            ["za", "[", "w8", "#0", "]", "list2/1.d"]
        );
        assert_eq!(
            names(&flat("smlall za.s[w8, 0:3], z0.b, z1.b")),
            ["za", "[", "w8", "0:3", "]", "z0", "z1"]
        );
        // A range written with expressions isn't one clang reads.
        assert!(flat("smlall za.s[w8, four:3], z0.b, z1.b").is_empty());
    }

    #[test]
    fn registers_immediates_and_memory() {
        assert_eq!(
            names(&flat("ldp x29, x30, [sp, #-16]!")),
            ["x29", "x30", "[", "sp", "#-16", "]", "!"]
        );
        assert_eq!(
            names(&flat("ldr x0, [x1, x2, lsl #3]")),
            ["x0", "[", "x1", "x2", "lsl #3", "]"]
        );
        assert_eq!(names(&flat("add x0, x0, #four")), ["x0", "x0", "#4"]);
        assert_eq!(names(&flat("mov tmp, #1 << 4")), ["x9", "#16"]);
    }

    #[test]
    fn bare_extends() {
        assert_eq!(
            names(&flat("ldr x0, [x1, w2, sxtw]")),
            ["x0", "[", "x1", "w2", "sxtw", "]"]
        );
    }

    #[test]
    fn neon_arrangements_are_also_tokens() {
        assert_eq!(
            names(&flat("add v0.4s, v1.4s, v2.4s")),
            ["v0", ".4s", "v1", ".4s", "v2", ".4s"]
        );
        assert_eq!(names(&flat("mov v0.s[1], w0")), ["v0", ".s", "[1]", "w0"]);
    }

    #[test]
    fn lists() {
        assert_eq!(
            names(&flat("ld1 {v0.16b-v3.16b}, [x0], #64")),
            ["list4/1.16b", "[", "x0", "]", "#64"]
        );
        assert_eq!(
            names(&flat("ld1 {v0.s, v1.s}[1], [x0]")),
            ["list2/1.s", "[1]", "[", "x0", "]"]
        );
        assert_eq!(
            names(&flat("ld1 {v31.2d, v0.2d}, [x0]")),
            ["list2/1.2d", "[", "x0", "]"]
        );
    }

    #[test]
    fn predicate_lists_wrap_at_16() {
        assert_eq!(
            names(&flat("pext {p15.b, p0.b}, pn15[1]")),
            ["list2/1.b", "pn15", "[1]"]
        );
    }

    #[test]
    fn symbols_and_variants() {
        assert_eq!(names(&flat("adrp x0, _msg@PAGE")), ["x0", "sym:_msg@page"]);
        assert_eq!(names(&flat("b _f")), ["id:_f"]);
        assert_eq!(names(&flat("b 1f")), ["sym:1f"]);
    }

    #[test]
    fn sve_predicates_and_mul_vl() {
        assert_eq!(
            names(&flat("ld1d {z0.d}, p0/z, [x0, #1, mul vl]")),
            [
                "list1/1.d",
                "p0",
                "/",
                "z",
                "[",
                "x0",
                "#1",
                "mul",
                "vl",
                "]"
            ]
        );
    }

    #[test]
    fn floats_and_writeback() {
        assert_eq!(names(&flat("fmov d0, #-1.5")), ["d0", "#-1.5f"]);
        assert_eq!(
            names(&flat("cpyfp [x0]!, [x1]!, x2!")),
            ["[", "x0", "]", "!", "[", "x1", "]", "!", "x2", "!"]
        );
    }

    #[test]
    fn unjudgeable_operands() {
        assert!(flat("ldr x0, =0x1234").is_empty());
    }

    #[test]
    fn numbers() {
        assert_eq!(number_value("0x1F"), Value::Int(31));
        assert_eq!(number_value("0b101"), Value::Int(5));
        assert_eq!(number_value("017"), Value::Int(15));
        assert_eq!(number_value("42"), Value::Int(42));
        assert_eq!(number_value("1.5"), Value::Float(1.5));
        assert_eq!(number_value("0"), Value::Int(0));
    }

    #[test]
    fn apple_expression_semantics() {
        let value = |text: &str| {
            let parse = arm_syntax::parse(&format!("a = {text}"));
            let assignment = parse
                .tree()
                .statements()
                .find_map(|s| match s {
                    ast::Statement::Assignment(a) => Some(a),
                    _ => None,
                })
                .expect("assignment");
            let (constants, aliases) = (HashMap::new(), HashMap::new());
            let scope = Scope {
                constants: &constants,
                aliases: &aliases,
            };
            eval(&assignment.value().expect("value"), &scope)
        };
        // Probed with clang 21 (docs/design.md#syntax).
        assert_eq!(value("1 << 2 + 1"), Value::Int(8));
        assert_eq!(value("6 & 3 == 3"), Value::Int(6));
        assert_eq!(value("3 == 3 + 0"), Value::Int(-1));
        assert_eq!(value("!0 + 1"), Value::Int(2));
        assert_eq!(value("~0 & 7"), Value::Int(7));
        assert_eq!(value("10 % 4 * 2"), Value::Int(4));
        assert_eq!(value("-8 >> 1"), Value::Int(((-8i64 as u64) >> 1) as i64));
        assert_eq!(value("1 / 0"), Value::Unknown);
    }
}
