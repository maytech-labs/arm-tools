//! The rules. Each finds problems clang accepts but that break at run time
//! or under Apple's ABI. See docs/lint.md for each rule's page.

use std::collections::{HashMap, HashSet};

use arm_isa::{Imm, Operand, Platform};
use arm_sema::{Analysis, Instruction, Item, SymbolKind};

use crate::flow::{self, Cfg, Function, SpChange, operands};
use crate::{Finding, Level, RuleInfo};

pub(crate) const RULES: &[RuleInfo] = &[
    RuleInfo {
        id: "abi/x18-reserved",
        default_level: Level::Warning,
        summary: "Uses x18, which Darwin reserves for the platform",
        explanation: "Apple's arm64 ABI reserves x18 for the OS. The kernel may change it at any time, such as on a context switch, so a value kept there can vanish. Use another scratch register, like x9 to x15. Darwin only: Linux leaves x18 to programs.",
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
        explanation: "Darwin requires a valid chain of frame records for backtraces, profilers, and crash reports. A function that calls others should start with `stp x29, x30, [sp, #-16]!` and `mov x29, sp`. Darwin only: on Linux a frame pointer is optional.",
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
        explanation: "adrp gives only the 4 KB page of a symbol. The full address needs the offset within the page too: `add x0, x0, _sym@PAGEOFF`, or `ldr x0, [x0, _sym@PAGEOFF]`. On Linux: `add x0, x0, :lo12:sym`.",
    },
    RuleInfo {
        id: "addr/variant-mismatch",
        default_level: Level::Warning,
        summary: "Pairs a page relocation with the wrong offset, like @GOTPAGE with @PAGEOFF",
        explanation: "Each page variant pairs with one offset variant: @PAGE with @PAGEOFF, @GOTPAGE with @GOTPAGEOFF, and @TLVPPAGE with @TLVPPAGEOFF. On Linux, `adrp x0, sym` pairs with :lo12:, :got: with :got_lo12:, :gottprel: with :gottprel_lo12:, and :tlsdesc: with :tlsdesc_lo12:. Mixing them computes an address in the wrong place.",
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
        id: "flow/undefined-register",
        default_level: Level::Warning,
        summary: "Reads a scratch register that nothing has set",
        explanation: "x9 to x17 and v16 to v31 hold nothing useful when a function starts, and every call (bl, blr) may overwrite them. Reading one before setting it on every path gets garbage. Set it first, or keep the value in a callee-saved register (x19 to x28) across calls.",
    },
    RuleInfo {
        id: "abi/syscall-number",
        default_level: Level::Warning,
        summary: "svc without the syscall number in x16 (Darwin) or x8 (Linux)",
        explanation: "On Darwin, svc #0x80 takes the system call number in x16, not x8 as on Linux. Set x16 before the svc, for example mov x16, #4 for write. On Linux, svc #0 takes it in x8: mov x8, #64 for write.",
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
    let darwin = analysis.platform() == Platform::Darwin;
    if darwin {
        x18(analysis, &mut out);
    }
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
        frame(function, &insns, darwin, &mut out);
        undefined_registers(analysis, &cfg, function, entry, &mut out);
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

/// The link register always; the frame pointer only on Darwin, whose ABI
/// requires frame records.
fn frame(function: &Function, insns: &[&Instruction], darwin: bool, out: &mut Vec<Finding>) {
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
    if !sets_fp && darwin {
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
    // A function that never returns, like _start ending in the exit
    // syscall, has no caller to preserve registers for.
    let returns = insns
        .iter()
        .any(|i| matches!(i.mnemonic.head.as_str(), "ret" | "retaa" | "retab"));
    if !returns {
        return;
    }
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
/// How a platform writes page relocations: Mach-O's `sym@PAGE` and
/// `sym@PAGEOFF`, or ELF's `adrp x0, sym` and `:lo12:sym`.
struct Relocations(Platform);

impl Relocations {
    /// The offset relocation an adrp operand pairs with, if it's a page.
    fn expected_offset(&self, operand: &Operand) -> Option<(String, &'static str)> {
        // `adrp x0, msg`: a bare name, on Linux the whole relocation.
        if let (Operand::Ident(name), Platform::Linux) = (operand, self.0) {
            return Some((name.clone(), "lo12"));
        }
        let Operand::Imm(Imm::Symbol {
            name,
            variant,
            specifier,
            ..
        }) = operand
        else {
            return None;
        };
        let offset = match self.0 {
            Platform::Darwin => match variant.as_deref()? {
                "page" => "pageoff",
                "gotpage" => "gotpageoff",
                "tlvppage" => "tlvppageoff",
                _ => return None,
            },
            Platform::Linux => match specifier.as_deref() {
                None if variant.is_none() => "lo12",
                Some("got") => "got_lo12",
                Some("gottprel") => "gottprel_lo12",
                Some("tlsdesc") => "tlsdesc_lo12",
                _ => return None,
            },
        };
        Some((name.clone(), offset))
    }

    /// The offset relocation an operand uses on `name`, if any.
    fn offset_of(&self, operand: &Operand, name: &str) -> Option<String> {
        let Operand::Imm(Imm::Symbol {
            name: n,
            variant,
            specifier,
            ..
        }) = operand
        else {
            return None;
        };
        if n != name {
            return None;
        }
        match self.0 {
            Platform::Darwin => variant.clone().filter(|v| v.ends_with("pageoff")),
            Platform::Linux => specifier.clone().filter(|s| s.ends_with("lo12")),
        }
    }

    /// The page relocation as written: `_g@GOTPAGE`, `:got:g`, or `g`.
    fn page(&self, name: &str, offset: &str) -> String {
        match self.0 {
            Platform::Darwin => {
                format!("{name}@{}", offset.trim_end_matches("off").to_uppercase())
            }
            Platform::Linux => match offset.strip_suffix("_lo12") {
                Some(kind) => format!(":{kind}:{name}"),
                None => name.to_string(),
            },
        }
    }

    /// The offset relocation as written: `_g@PAGEOFF` or `:lo12:g`.
    fn offset(&self, name: &str, offset: &str) -> String {
        match self.0 {
            Platform::Darwin => format!("{name}@{}", offset.to_uppercase()),
            Platform::Linux => format!(":{offset}:{name}"),
        }
    }
}

fn adrp(analysis: &Analysis, insns: &[&Instruction], out: &mut Vec<Finding>) {
    let text = analysis.text();
    let relocations = Relocations(analysis.platform());
    for (i, insn) in insns.iter().enumerate() {
        if insn.mnemonic.head != "adrp" {
            continue;
        }
        let ops = operands(insn);
        let (Some(dest), Some((name, expected))) = (
            ops.first().and_then(flow::register_id),
            ops.get(1).and_then(|o| relocations.expected_offset(o)),
        ) else {
            continue;
        };
        let mut paired = false;
        for later in &insns[i + 1..] {
            let later_ops = operands(later);
            let uses_dest = later_ops
                .iter()
                .skip(usize::from(!later.mnemonic.head.starts_with("st")))
                .chain(later_ops.first())
                .any(|o| flow::register_id(o).as_deref() == Some(dest.as_str()));
            let offset = later_ops
                .iter()
                .enumerate()
                .find_map(|(k, o)| relocations.offset_of(o, &name).map(|v| (k, v)));
            if let (true, Some((k, found))) = (uses_dest, offset) {
                paired = true;
                if found != expected {
                    let at = later.operand_ranges[k].clone();
                    let page = relocations.page(&name, expected);
                    let wanted = relocations.offset(&name, expected);
                    let wrong = relocations.offset(&name, &found);
                    let mut finding = finding(
                        "addr/variant-mismatch",
                        at.clone(),
                        format!(
                            "'{page}' pairs with '{wanted}', not '{wrong}': the address points somewhere else"
                        ),
                    );
                    // Only when the operand is exactly the relocation, so an
                    // addend isn't lost.
                    let written = text.get(at.clone()).unwrap_or_default();
                    if written.eq_ignore_ascii_case(&wrong) {
                        finding = fixed(finding, format!("Change to {wanted}"), vec![(at, wanted)]);
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
                "add {register}, {register}, {}",
                relocations.offset(&name, expected)
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

/// Registers with no defined value at entry or after a call.
/// On Linux, also x8: svc's syscall number, and the indirect result
/// register, which calls don't preserve.
fn scratch_registers(platform: Platform) -> impl Iterator<Item = String> {
    let x8 = (platform == Platform::Linux).then(|| "x8".to_string());
    x8.into_iter()
        .chain((9..=17).map(|n| format!("x{n}")))
        .chain((16..=31).map(|n| format!("v{n}")))
}

/// Reads of scratch registers that are undefined on every path to the read:
/// at the start of an exported function (helpers may pass values in scratch
/// registers by their own convention), or since a call.
fn undefined_registers(
    analysis: &Analysis,
    cfg: &Cfg,
    function: &Function,
    entry: usize,
    out: &mut Vec<Finding>,
) {
    use std::collections::BTreeMap;
    type Undefined = BTreeMap<String, String>;
    let exported = analysis
        .symbols()
        .iter()
        .any(|s| s.name == function.name && s.exported);
    let initial: Undefined = if exported {
        scratch_registers(analysis.platform())
            .map(|r| (r, "when the function starts".to_string()))
            .collect()
    } else {
        Undefined::new()
    };
    let line = |offset: usize| {
        analysis
            .line_index()
            .line_col(analysis.text(), offset, arm_sema::Encoding::Utf8)
            .line
            + 1
    };
    // Undefined after an instruction, given undefined before it.
    let transfer = |i: usize, before: &Undefined| -> Undefined {
        let insn = cfg.insns[i];
        let mut after = before.clone();
        for written in flow::writes(insn) {
            after.remove(&written);
        }
        if flow::is_call(insn) {
            let origin = format!("since the call on line {}", line(insn.range.start));
            for register in scratch_registers(analysis.platform()) {
                after.insert(register, origin.clone());
            }
        }
        after
    };
    // Undefined on every path: intersect where paths meet.
    let mut before: HashMap<usize, Undefined> = HashMap::new();
    let mut work = vec![(entry, initial)];
    while let Some((i, incoming)) = work.pop() {
        let merged = match before.get(&i) {
            None => incoming,
            Some(old) => old
                .iter()
                .filter(|(r, _)| incoming.contains_key(*r))
                .map(|(r, o)| (r.clone(), o.clone()))
                .collect(),
        };
        if before.get(&i) == Some(&merged) {
            continue;
        }
        before.insert(i, merged.clone());
        let after = transfer(i, &merged);
        for next in cfg.successors(analysis, i).next {
            work.push((next, after.clone()));
        }
    }
    let mut reported = HashSet::new();
    let mut indices: Vec<usize> = before.keys().copied().collect();
    indices.sort_unstable();
    for i in indices {
        let insn = cfg.insns[i];
        for register in flow::reads(insn, analysis.platform()) {
            let Some(origin) = before[&i].get(&register) else {
                continue;
            };
            // x8 can carry a struct-return address into a function; only a
            // call is sure to have overwritten it.
            if register == "x8" && insn.mnemonic.head != "svc" && origin.starts_with("when") {
                continue;
            }
            if !reported.insert((register.clone(), origin.clone())) {
                continue;
            }
            let shown = match register.strip_prefix('v') {
                Some(n) if insn.mnemonic.head != "svc" => format!("v{n}"),
                _ => register.clone(),
            };
            let number = flow::syscall_register(analysis.platform());
            let found = if insn.mnemonic.head == "svc" && register == number {
                let example = match analysis.platform() {
                    Platform::Darwin => "mov x16, #1 for exit",
                    Platform::Linux => "mov x8, #93 for exit",
                };
                finding(
                    "abi/syscall-number",
                    insn.range.clone(),
                    format!(
                        "svc takes the syscall number in {number}, but nothing sets {number} {origin} on any path here. Set it first, like {example}"
                    ),
                )
            } else {
                finding(
                    "flow/undefined-register",
                    insn.range.clone(),
                    format!(
                        "this reads {shown}, but nothing sets {shown} {origin} on any path here, so its value is garbage. Set it first, or keep the value in x19 to x28 across calls"
                    ),
                )
            };
            out.push(found);
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
    fn undefined_scratch_registers() {
        // Read at entry of an exported function.
        let entry = ".globl _f\n_f:\n  add x0, x9, #1\n  ret\n";
        assert_eq!(rules(entry), ["flow/undefined-register"]);
        // Clobbered by a call.
        let call = ".globl _f\n_f:\n  stp x29, x30, [sp, #-16]!\n  mov x29, sp\n  mov x9, #1\n  bl _g\n  add x0, x9, #1\n  ldp x29, x30, [sp], #16\n  ret\n";
        let found = lint(&Analysis::new(call), &Config::default());
        let undefined: Vec<_> = found
            .iter()
            .filter(|d| d.code == "flow/undefined-register")
            .collect();
        assert_eq!(undefined.len(), 1);
        assert!(
            undefined[0].message.contains("since the call on line 6"),
            "{}",
            undefined[0].message
        );
        // Set on every path: fine.
        let set = ".globl _f\n_f:\n  mov x9, #1\n  add x0, x9, #1\n  ret\n";
        assert!(rules(set).is_empty());
        // Set on only one path: not reported (must be undefined on every path).
        let one_path = ".globl _f\n_f:\n  cbz x0, 1f\n  mov x9, #1\n1:\n  add x0, x9, #1\n  ret\n";
        assert!(rules(one_path).is_empty());
        // Helpers may take scratch registers as inputs by their own convention.
        let helper = ".globl _f\n_f:\n  stp x29, x30, [sp, #-16]!\n  mov x29, sp\n  mov x9, #1\n  bl helper\n  ldp x29, x30, [sp], #16\n  ret\nhelper:\n  add x0, x9, #1\n  ret\n";
        assert!(rules(helper).is_empty(), "{:?}", rules(helper));
    }

    #[test]
    fn syscall_number() {
        let missing = ".globl _main\n_main:\n  mov x0, #0\n  svc #0x80\n";
        assert_eq!(rules(missing), ["abi/syscall-number"]);
        let set = ".globl _main\n_main:\n  mov x0, #0\n  mov x16, #1\n  svc #0x80\n";
        assert!(rules(set).is_empty());
    }

    fn linux(text: &str) -> Vec<(&'static str, String)> {
        lint(
            &Analysis::for_platform(text, arm_sema::Platform::Linux),
            &Config::default(),
        )
        .into_iter()
        .filter(|d| d.code != "style/unused-label")
        .map(|d| {
            (
                d.code,
                text[d.range]
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            )
        })
        .collect()
    }

    #[test]
    fn linux_rules() {
        // x18 is free, and a frame pointer optional; x30 still needs saving.
        let ok = ".globl main\nmain:\n  str x30, [sp, #-16]!\n  mov x18, #1\n  bl puts\n  ldr x30, [sp], #16\n  ret\n";
        assert_eq!(linux(ok), []);
        let darwin: Vec<&str> = rules(ok);
        assert!(darwin.contains(&"abi/x18-reserved") && darwin.contains(&"abi/frame-pointer"));
        // The syscall number goes in x8.
        let missing = ".globl main\nmain:\n  mov x0, #0\n  svc #0\n";
        let found = linux(missing);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, "abi/syscall-number");
        let set = ".globl main\nmain:\n  mov x0, #0\n  mov x8, #93\n  svc #0\n";
        assert_eq!(linux(set), []);
        // x16 is the Darwin register; on Linux it's not the number.
        let darwin_style = ".globl main\nmain:\n  mov x16, #1\n  svc #0\n";
        assert_eq!(linux(darwin_style)[0].0, "abi/syscall-number");
        // x8 read after a call is garbage; at entry it may be a result pointer.
        let entry = ".globl f\nf:\n  str x0, [x8]\n  ret\n";
        assert_eq!(linux(entry), []);
        // A program entry that exits never returns: x19 is its own.
        let start =
            ".globl _start\n_start:\n  mov x19, #5\n  mov x0, x19\n  mov x8, #93\n  svc #0\n";
        assert_eq!(linux(start), []);
        // LSE atomics write their second register, not their first.
        let lse = ".globl f\nf:\n  mov x10, #1\n  ldaddal x10, x11, [x0]\n  mov x0, x11\n  ret\n";
        assert_eq!(rules(lse), Vec::<&str>::new());
    }

    #[test]
    fn linux_relocation_pairs() {
        let missing = ".globl main\nmain:\n  adrp x0, msg\n  ret\nmsg: .asciz \"x\"\n";
        let found = lint(
            &Analysis::for_platform(missing, arm_sema::Platform::Linux),
            &Config::default(),
        );
        let adrp = found
            .iter()
            .find(|d| d.code == "addr/adrp-pageoff")
            .expect("adrp-pageoff");
        assert!(
            adrp.message.contains("add x0, x0, :lo12:msg"),
            "{}",
            adrp.message
        );
        let fix = adrp.fix.as_ref().expect("a fix");
        assert_eq!(fix.edits[0].1, "\n  add x0, x0, :lo12:msg");
        let paired = ".globl main\nmain:\n  adrp x0, msg\n  add x0, x0, :lo12:msg\n  adrp x1, :got:msg\n  ldr x1, [x1, :got_lo12:msg]\n  ret\nmsg: .asciz \"x\"\n";
        assert_eq!(linux(paired), []);
        let mixed = ".globl main\nmain:\n  adrp x1, :got:msg\n  ldr x1, [x1, :lo12:msg]\n  ret\nmsg: .asciz \"x\"\n";
        let found = lint(
            &Analysis::for_platform(mixed, arm_sema::Platform::Linux),
            &Config::default(),
        );
        let mismatch = found
            .iter()
            .find(|d| d.code == "addr/variant-mismatch")
            .expect("variant-mismatch");
        assert!(
            mismatch
                .message
                .contains("':got:msg' pairs with ':got_lo12:msg', not ':lo12:msg'"),
            "{}",
            mismatch.message
        );
        assert_eq!(
            mismatch.fix.as_ref().map(|f| f.edits[0].1.as_str()),
            Some(":got_lo12:msg")
        );
        // Darwin's message, in its own syntax.
        let darwin =
            ".globl _f\n_f:\n  adrp x0, _g@GOTPAGE\n  ldr x0, [x0, _g@PAGEOFF]\n  ret\n_g: nop\n";
        let found = lint(&Analysis::new(darwin), &Config::default());
        let mismatch = found
            .iter()
            .find(|d| d.code == "addr/variant-mismatch")
            .expect("darwin mismatch");
        assert!(
            mismatch
                .message
                .contains("'_g@GOTPAGE' pairs with '_g@GOTPAGEOFF', not '_g@PAGEOFF'"),
            "{}",
            mismatch.message
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
