//! The rules. Each finds problems clang accepts but that break at run time
//! or under Apple's ABI. See docs/lint.md for each rule's page.

use std::collections::{HashMap, HashSet};

use arm_isa::{Imm, Operand};
use arm_sema::{Analysis, Instruction, Item, SymbolKind};

use crate::flow::{self, Cfg, Function, SpChange, operands};
use crate::{Finding, Level, RuleInfo};

pub(crate) const RULES: &[RuleInfo] = &[
    RuleInfo {
        id: "abi/x18-reserved",
        default_level: Level::Warning,
        summary: "Uses x18, which Darwin reserves for the platform",
        explanation: "Apple's arm64 ABI reserves x18 for the OS. The kernel may change it at any time, such as on a context switch, so a value kept there can vanish. Use another scratch register, like x9 to x15.",
    },
    RuleInfo {
        id: "abi/stack-alignment",
        default_level: Level::Warning,
        summary: "Moves sp by an amount that isn't a multiple of 16",
        explanation: "sp must stay 16-byte aligned whenever it's used to access memory, or the CPU raises an alignment fault. Round stack adjustments up to a multiple of 16.",
    },
    RuleInfo {
        id: "abi/frame-pointer",
        default_level: Level::Warning,
        summary: "A function that calls others doesn't set up x29",
        explanation: "Darwin requires a valid chain of frame records for backtraces, profilers, and crash reports. A function that calls others should start with `stp x29, x30, [sp, #-16]!` and `mov x29, sp`.",
    },
    RuleInfo {
        id: "abi/link-register",
        default_level: Level::Warning,
        summary: "A function calls another without saving x30",
        explanation: "bl and blr write the return address to x30, replacing the caller's. A function that calls others must save x30 first, usually with `stp x29, x30, [sp, #-16]!`, and restore it before ret.",
    },
    RuleInfo {
        id: "abi/callee-saved",
        default_level: Level::Warning,
        summary: "Writes x19 to x28 or d8 to d15 without saving them",
        explanation: "Callers expect x19 to x28 and the low 64 bits of v8 to v15 to survive a call. A function that writes one must store it in its prologue and restore it before returning.",
    },
    RuleInfo {
        id: "addr/adrp-pageoff",
        default_level: Level::Warning,
        summary: "adrp loads a page that nothing adds the offset to",
        explanation: "adrp gives only the 4 KB page of a symbol. The full address needs the offset within the page too: `add x0, x0, _sym@PAGEOFF`, or `ldr x0, [x0, _sym@PAGEOFF]`.",
    },
    RuleInfo {
        id: "addr/variant-mismatch",
        default_level: Level::Warning,
        summary: "Pairs @PAGE, @GOTPAGE, or @TLVPPAGE with the wrong offset",
        explanation: "Each page variant pairs with one offset variant: @PAGE with @PAGEOFF, @GOTPAGE with @GOTPAGEOFF, and @TLVPPAGE with @TLVPPAGEOFF. Mixing them computes an address in the wrong place.",
    },
    RuleInfo {
        id: "flow/unreachable",
        default_level: Level::Warning,
        summary: "An instruction that nothing can reach",
        explanation: "The instruction follows one that never falls through, like b or ret, and has no label, so no branch can reach it. Add a label, or remove the code.",
    },
    RuleInfo {
        id: "flow/fallthrough",
        default_level: Level::Warning,
        summary: "A function's last instruction falls into the next function",
        explanation: "The function doesn't end with ret or a branch, so it keeps running into whatever comes next. End it with ret, or a branch if that's intended.",
    },
    RuleInfo {
        id: "flow/unbalanced-stack",
        default_level: Level::Warning,
        summary: "Returns with sp at a different place than on entry",
        explanation: "Along some path, the function moves sp and doesn't move it back before ret or a tail call, so the caller's stack is corrupted. Every push needs a matching pop on every path.",
    },
    RuleInfo {
        id: "style/unused-label",
        default_level: Level::Hint,
        summary: "A label nothing refers to",
        explanation: "The label isn't exported and nothing in the file branches to or loads it. Remove it, or export it with .globl if other files need it.",
    },
];

pub(crate) fn run(analysis: &Analysis) -> Vec<Finding> {
    let cfg = Cfg::new(analysis);
    let mut out = Vec::new();
    x18(analysis, &mut out);
    stack_alignment(analysis, &mut out);
    unreachable(analysis, &mut out);
    unused_labels(analysis, &mut out);
    for function in &cfg.functions {
        fallthrough(&cfg, function, &mut out);
        let Some(entry) = function.entry else {
            continue;
        };
        // The function's body is what's reachable from its entry, which can
        // be laid out anywhere in the file.
        let (reached, opaque) = cfg.reachable(analysis, entry);
        let insns: Vec<&Instruction> = reached.iter().map(|&i| cfg.insns[i]).collect();
        adrp(analysis, &insns, &mut out);
        // Code we can't see, like a macro call, would make these guesses.
        if opaque {
            continue;
        }
        frame(function, &insns, &mut out);
        callee_saved(function, &insns, &mut out);
        unbalanced_stack(analysis, &cfg, function, entry, &mut out);
    }
    out
}

fn finding(rule: &'static str, range: std::ops::Range<usize>, message: String) -> Finding {
    Finding {
        rule,
        range,
        message,
        fix: None,
    }
}

/// A finding with a quick fix.
fn fixed(
    mut finding: Finding,
    title: String,
    edits: Vec<(std::ops::Range<usize>, String)>,
) -> Finding {
    finding.fix = Some(arm_sema::Fix { title, edits });
    finding
}

fn x18(analysis: &Analysis, out: &mut Vec<Finding>) {
    for item in analysis.items() {
        let Item::Instruction(insn) = item else {
            continue;
        };
        for (i, operand) in operands(insn).iter().enumerate() {
            let Operand::Register(r) = operand else {
                continue;
            };
            let name = r.register.llvm_name();
            if name == "X18" || name == "W18" {
                out.push(finding(
                    "abi/x18-reserved",
                    insn.operand_ranges[i].clone(),
                    format!(
                        "{} is reserved by Darwin: the OS may overwrite it at any time. Use a scratch register like x9 to x15",
                        r.register.name()
                    ),
                ));
            }
        }
    }
}

fn stack_alignment(analysis: &Analysis, out: &mut Vec<Finding>) {
    for item in analysis.items() {
        let Item::Instruction(insn) = item else {
            continue;
        };
        if let SpChange::By(n) = flow::sp_change(insn)
            && n % 16 != 0
        {
            let rounded = (n.abs() + 15) / 16 * 16;
            let mut found = finding(
                "abi/stack-alignment",
                insn.range.clone(),
                format!(
                    "this moves sp by {} bytes, which isn't a multiple of 16, so sp loses its required 16-byte alignment. Use {rounded}",
                    n.abs()
                ),
            );
            // `add sp, sp, #n` and `sub sp, sp, #n`: the immediate is its own operand.
            let immediate = operands(insn)
                .iter()
                .position(|o| matches!(o, Operand::Imm(Imm::Constant(_))))
                .filter(|_| matches!(insn.mnemonic.head.as_str(), "add" | "sub"))
                .and_then(|i| insn.operand_ranges.get(i).cloned());
            if let Some(at) = immediate {
                found = fixed(
                    found,
                    format!("Round to {rounded}"),
                    vec![(at, format!("#{rounded}"))],
                );
            }
            out.push(found);
        }
    }
}

fn unreachable(analysis: &Analysis, out: &mut Vec<Finding>) {
    let mut after_terminator = false;
    for item in analysis.items() {
        match item {
            Item::Label(_) | Item::MacroCall(_) | Item::Section { .. } => after_terminator = false,
            Item::Data(_) => {}
            Item::Instruction(insn) => {
                if after_terminator {
                    out.push(finding(
                        "flow/unreachable",
                        insn.range.clone(),
                        "unreachable: the instruction above never falls through, and there's no label here to branch to".into(),
                    ));
                }
                after_terminator = flow::is_terminator(insn);
            }
        }
    }
}

fn unused_labels(analysis: &Analysis, out: &mut Vec<Finding>) {
    for (i, symbol) in analysis.symbols().iter().enumerate() {
        // L and l prefixes are assembler-local temporaries; compilers emit
        // many, like Ltmp0 for debug info, that nothing in the file names.
        let temporary = symbol.name.starts_with(['L', 'l']);
        // Labels from included files may be used by other files.
        let imported = symbol.origin.is_some();
        if symbol.kind != SymbolKind::Label || symbol.exported || temporary || imported {
            continue;
        }
        if analysis.references_to(i).next().is_none() {
            out.push(finding(
                "style/unused-label",
                symbol.range.clone(),
                format!(
                    "'{}' is never used: remove it, or export it with .globl",
                    symbol.name
                ),
            ));
        }
    }
}

fn fallthrough(cfg: &Cfg, function: &Function, out: &mut Vec<Finding>) {
    let Some(last) = function.span.clone().last().map(|i| cfg.insns[i]) else {
        return;
    };
    // A final `svc` is how a program ends: the exit syscall doesn't return.
    if flow::is_terminator(last) || last.mnemonic.head == "svc" || last.operands.is_none() {
        return;
    }
    let message = match &function.next {
        Some(next) => format!(
            "'{}' falls through into '{next}': end it with ret, or a branch if that's intended",
            function.name
        ),
        None => format!(
            "'{}' runs off the end of its code: end it with ret or a branch",
            function.name
        ),
    };
    out.push(finding("flow/fallthrough", last.range.clone(), message));
}

fn frame(function: &Function, insns: &[&Instruction], out: &mut Vec<Finding>) {
    let Some(call) = insns.iter().find(|i| flow::is_call(i)) else {
        return;
    };
    // A function that never returns, like a program's entry point that ends
    // with the exit syscall, doesn't need its return address.
    let returns = insns
        .iter()
        .any(|i| matches!(i.mnemonic.head.as_str(), "ret" | "retaa" | "retab"));
    if !returns {
        return;
    }
    let saves_lr = insns
        .iter()
        .any(|i| flow::stored(i).iter().any(|r| r == "x30"));
    if !saves_lr {
        out.push(finding(
            "abi/link-register",
            call.range.clone(),
            format!(
                "this call overwrites x30, and '{}' never saves it, so it can't return to its caller. Save it first: stp x29, x30, [sp, #-16]!",
                function.name
            ),
        ));
    }
    let sets_fp = insns.iter().any(|i| {
        let ops = operands(i);
        matches!(i.mnemonic.head.as_str(), "mov" | "add")
            && ops.first().and_then(flow::register_id).as_deref() == Some("x29")
            && ops.get(1).and_then(flow::register_id).as_deref() == Some("sp")
    });
    if !sets_fp {
        out.push(finding(
            "abi/frame-pointer",
            function.name_range.clone(),
            format!(
                "'{}' calls other functions but doesn't set up a frame pointer. Darwin needs one: after saving x29 and x30, add mov x29, sp",
                function.name
            ),
        ));
    }
}

fn is_callee_saved(register: &str) -> bool {
    let number = |prefix: &str| {
        register
            .strip_prefix(prefix)
            .and_then(|n| n.parse::<u32>().ok())
    };
    number("x").is_some_and(|n| (19..=28).contains(&n))
        || number("v").is_some_and(|n| (8..=15).contains(&n))
}

fn callee_saved(function: &Function, insns: &[&Instruction], out: &mut Vec<Finding>) {
    let saved: HashSet<String> = insns.iter().flat_map(|i| flow::stored(i)).collect();
    let mut reported = HashSet::new();
    for insn in insns {
        for register in flow::writes(insn) {
            if is_callee_saved(&register)
                && !saved.contains(&register)
                && reported.insert(register.clone())
            {
                let shown = match register.strip_prefix('v') {
                    Some(n) => format!("d{n}"),
                    None => register.clone(),
                };
                out.push(finding(
                    "abi/callee-saved",
                    insn.range.clone(),
                    format!(
                        "'{}' changes {shown} without saving it, but callers expect {shown} to survive the call. Save it in the prologue and restore it before ret",
                        function.name
                    ),
                ));
            }
        }
    }
}

/// adrp's page with no offset added, or the wrong offset variant.
fn adrp(analysis: &Analysis, insns: &[&Instruction], out: &mut Vec<Finding>) {
    let text = analysis.text();
    for (i, insn) in insns.iter().enumerate() {
        if insn.mnemonic.head != "adrp" {
            continue;
        }
        let ops = operands(insn);
        let (
            Some(dest),
            Some(Operand::Imm(Imm::Symbol {
                name,
                variant: Some(variant),
                ..
            })),
        ) = (ops.first().and_then(flow::register_id), ops.get(1))
        else {
            continue;
        };
        let expected = match variant.as_str() {
            "page" => "pageoff",
            "gotpage" => "gotpageoff",
            "tlvppage" => "tlvppageoff",
            _ => continue,
        };
        let mut paired = false;
        for later in &insns[i + 1..] {
            let later_ops = operands(later);
            let uses_dest = later_ops
                .iter()
                .skip(usize::from(!later.mnemonic.head.starts_with("st")))
                .chain(later_ops.first())
                .any(|o| flow::register_id(o).as_deref() == Some(dest.as_str()));
            let offset = later_ops.iter().enumerate().find_map(|(k, o)| match o {
                Operand::Imm(Imm::Symbol {
                    name: n,
                    variant: Some(v),
                    ..
                }) if n == name && v.ends_with("pageoff") => Some((k, v.clone())),
                _ => None,
            });
            if let (true, Some((k, found))) = (uses_dest, offset) {
                paired = true;
                if found != expected {
                    let at = later.operand_ranges[k].clone();
                    let mut finding = finding(
                        "addr/variant-mismatch",
                        at.clone(),
                        format!(
                            "'{name}@{}' pairs with @{}, not @{}: the address points somewhere else",
                            variant.to_uppercase(),
                            expected.to_uppercase(),
                            found.to_uppercase()
                        ),
                    );
                    // Only when the operand is exactly `name@VARIANT`, so an
                    // addend isn't lost.
                    let written = text.get(at.clone()).unwrap_or_default();
                    if written.eq_ignore_ascii_case(&format!("{name}@{found}")) {
                        let replacement = format!("{name}@{}", expected.to_uppercase());
                        finding = fixed(
                            finding,
                            format!("Change to {replacement}"),
                            vec![(at, replacement)],
                        );
                    }
                    out.push(finding);
                }
                break;
            }
            if flow::writes(later).contains(&dest) {
                break;
            }
        }
        if !paired {
            let register = insn
                .operand_ranges
                .first()
                .and_then(|r| text.get(r.clone()))
                .unwrap_or(dest.as_str());
            let add = format!(
                "add {register}, {register}, {name}@{}",
                expected.to_uppercase()
            );
            let found = finding(
                "addr/adrp-pageoff",
                insn.operand_ranges
                    .get(1)
                    .cloned()
                    .unwrap_or(insn.range.clone()),
                format!("adrp loads only the 4 KB page of '{name}'. Add the offset: {add}"),
            );
            // Insert the add on the next line, indented like the adrp.
            let line_start = text[..insn.range.start].rfind('\n').map_or(0, |i| i + 1);
            let indent: String = text[line_start..insn.range.start]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            let at = insn.range.end..insn.range.end;
            out.push(fixed(
                found,
                format!("Add `{add}`"),
                vec![(at, format!("\n{indent}{add}"))],
            ));
        }
    }
}

/// sp changes along every path through a function must cancel out by each
/// ret or tail call.
fn unbalanced_stack(
    analysis: &Analysis,
    cfg: &Cfg,
    function: &Function,
    entry: usize,
    out: &mut Vec<Finding>,
) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Delta {
        Known(i64),
        Unknown,
    }
    let mut state: HashMap<usize, Delta> = HashMap::new();
    let mut work = vec![(entry, Delta::Known(0))];
    let mut exits: Vec<(usize, i64)> = Vec::new();
    while let Some((i, delta)) = work.pop() {
        let merged = match state.get(&i) {
            None => delta,
            Some(old) if *old == delta => continue,
            Some(_) => Delta::Unknown,
        };
        if state.insert(i, merged) == Some(merged) {
            continue;
        }
        let insn = cfg.insns[i];
        let after = match (merged, flow::sp_change(insn)) {
            (Delta::Known(d), SpChange::None) => Delta::Known(d),
            (Delta::Known(d), SpChange::By(n)) => Delta::Known(d + n),
            _ => Delta::Unknown,
        };
        let successors = cfg.successors(analysis, i);
        if successors.exits
            && let Delta::Known(d) = merged
            && d != 0
        {
            exits.push((i, d));
        }
        for next in successors.next {
            work.push((next, after));
        }
    }
    exits.sort_unstable();
    exits.dedup();
    for (i, delta) in exits {
        let (direction, fix) = if delta < 0 {
            ("lower", "pop what the function pushed")
        } else {
            ("higher", "don't pop more than it pushed")
        };
        out.push(finding(
            "flow/unbalanced-stack",
            cfg.insns[i].range.clone(),
            format!(
                "sp is {} bytes {direction} here than when '{}' was entered: {fix} before leaving",
                delta.abs(),
                function.name
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use crate::{Config, lint};
    use arm_sema::Analysis;

    /// (rule, first line of the flagged text) for each finding, leaving out
    /// the unused-label hint, which test snippets trip on purpose.
    fn findings(text: &str) -> Vec<(&'static str, String)> {
        lint(&Analysis::new(text), &Config::default())
            .into_iter()
            .filter(|d| d.code != "style/unused-label")
            .map(|d| {
                let first = text[d.range]
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                (d.code, first)
            })
            .collect()
    }

    fn rules(text: &str) -> Vec<&'static str> {
        findings(text).into_iter().map(|(r, _)| r).collect()
    }

    const GOOD: &str = "\
.globl _main
_main:
    stp x29, x30, [sp, #-16]!
    mov x29, sp
    stp x19, x20, [sp, #-16]!
    mov x19, x0
    adrp x0, _msg@PAGE
    add x0, x0, _msg@PAGEOFF
    bl _puts
    mov x0, x19
    ldp x19, x20, [sp], #16
    ldp x29, x30, [sp], #16
    ret
";

    #[test]
    fn a_well_formed_function_is_clean() {
        assert_eq!(findings(GOOD), Vec::<(&str, String)>::new());
    }

    #[test]
    fn x18() {
        assert_eq!(
            findings("_f:\n  add w18, w0, #1\n  ret\n"),
            [("abi/x18-reserved", "w18".into())]
        );
    }

    #[test]
    fn stack_alignment() {
        assert_eq!(
            rules("_f:\n  sub sp, sp, #24\n  add sp, sp, #24\n  ret\n"),
            ["abi/stack-alignment", "abi/stack-alignment"]
        );
        assert_eq!(
            rules("_f:\n  str x0, [sp, #-8]!\n  ldr x0, [sp], #8\n  ret\n"),
            ["abi/stack-alignment", "abi/stack-alignment"]
        );
        assert!(rules("_f:\n  sub sp, sp, #32\n  add sp, sp, #32\n  ret\n").is_empty());
    }

    #[test]
    fn unreachable() {
        assert_eq!(
            findings("_f:\n  ret\n  mov x0, #1\n"),
            [("flow/unreachable", "mov x0, #1".into())]
        );
        assert!(rules("_f:\n  b 1f\n1: ret\n").is_empty());
        // Data after ret, like a literal pool, isn't flagged.
        assert!(rules("_f:\n  ret\n  .quad 0\n").is_empty());
    }

    #[test]
    fn fallthrough() {
        let text = ".globl _a, _b\n_a:\n  mov x0, #1\n_b:\n  ret\n";
        let found = findings(text);
        assert_eq!(found, [("flow/fallthrough", "mov x0, #1".into())]);
        let message = lint(&Analysis::new(text), &Config::default())[0]
            .message
            .clone();
        assert!(
            message.contains("'_a' falls through into '_b'"),
            "{message}"
        );
    }

    #[test]
    fn calls_need_lr_and_fp() {
        let text = ".globl _f\n_f:\n  bl _g\n  ret\n";
        assert_eq!(rules(text), ["abi/frame-pointer", "abi/link-register"]);
    }

    #[test]
    fn callee_saved() {
        let text = ".globl _f\n_f:\n  mov x19, x0\n  mov d8, d0\n  ret\n";
        assert_eq!(
            findings(text),
            [
                ("abi/callee-saved", "mov x19, x0".into()),
                ("abi/callee-saved", "mov d8, d0".into())
            ]
        );
        let saved =
            ".globl _f\n_f:\n  str x19, [sp, #-16]!\n  mov x19, x0\n  ldr x19, [sp], #16\n  ret\n";
        assert!(rules(saved).is_empty());
    }

    #[test]
    fn adrp_needs_its_offset() {
        let text = ".globl _f\n_f:\n  adrp x0, _msg@PAGE\n  ret\n";
        assert_eq!(findings(text), [("addr/adrp-pageoff", "_msg@PAGE".into())]);
        let load = ".globl _f\n_f:\n  adrp x8, _v@GOTPAGE\n  ldr x8, [x8, _v@GOTPAGEOFF]\n  ret\n";
        assert!(rules(load).is_empty());
    }

    #[test]
    fn variant_mismatch() {
        let text = ".globl _f\n_f:\n  adrp x8, _v@GOTPAGE\n  ldr x8, [x8, _v@PAGEOFF]\n  ret\n";
        assert_eq!(
            findings(text),
            [("addr/variant-mismatch", "_v@PAGEOFF".into())]
        );
    }

    #[test]
    fn unbalanced_stack() {
        let text = ".globl _f\n_f:\n  sub sp, sp, #16\n  ret\n";
        let found = lint(&Analysis::new(text), &Config::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "flow/unbalanced-stack");
        assert!(
            found[0].message.contains("16 bytes lower"),
            "{}",
            found[0].message
        );
        // An early return before the pop is caught on its path only.
        let early = ".globl _f\n_f:\n  sub sp, sp, #16\n  cbz x0, 1f\n  add sp, sp, #16\n  ret\n1:\n  ret\n";
        let found = findings(early);
        assert_eq!(found.len(), 1);
        // mov sp, x29 is a restore we don't follow: no guess.
        let restore = ".globl _f\n_f:\n  sub sp, sp, #16\n  mov sp, x29\n  ret\n";
        assert!(rules(restore).is_empty());
    }

    /// Applies a finding's fix to the text.
    fn apply_fix(text: &str, rule: &str) -> String {
        let found = lint(&Analysis::new(text), &Config::default());
        let fix = found
            .iter()
            .find(|d| d.code == rule)
            .and_then(|d| d.fix.clone())
            .expect("a fix");
        let mut out = text.to_string();
        for (range, new) in fix.edits.iter().rev() {
            out.replace_range(range.clone(), new);
        }
        out
    }

    #[test]
    fn fixes() {
        assert_eq!(
            apply_fix(
                ".globl _f\n_f:\n  sub sp, sp, #24\n  add sp, sp, #24\n  ret\n",
                "abi/stack-alignment"
            ),
            ".globl _f\n_f:\n  sub sp, sp, #32\n  add sp, sp, #24\n  ret\n"
        );
        assert_eq!(
            apply_fix(
                ".globl _f\n_f:\n  adrp x8, _v@GOTPAGE\n  ldr x8, [x8, _v@PAGEOFF]\n  ret\n",
                "addr/variant-mismatch"
            ),
            ".globl _f\n_f:\n  adrp x8, _v@GOTPAGE\n  ldr x8, [x8, _v@GOTPAGEOFF]\n  ret\n"
        );
        assert_eq!(
            apply_fix(
                ".globl _f\n_f:\n    adrp x0, _msg@PAGE\n    ret\n",
                "addr/adrp-pageoff"
            ),
            ".globl _f\n_f:\n    adrp x0, _msg@PAGE\n    add x0, x0, _msg@PAGEOFF\n    ret\n"
        );
    }

    #[test]
    fn unused_labels_are_hints() {
        let found = lint(
            &Analysis::new("_f:\n  ret\nhelper:\n  ret\n"),
            &Config::default(),
        );
        let unused: Vec<_> = found
            .iter()
            .filter(|d| d.code == "style/unused-label")
            .collect();
        assert_eq!(unused.len(), 2);
        assert_eq!(unused[0].severity, arm_sema::Severity::Hint);
        // Temporaries and exported labels don't count.
        assert!(
            rules(".globl _f\n_f:\n  ret\nLtmp0:\n  ret\n")
                .iter()
                .all(|r| *r != "style/unused-label")
        );
    }
}
