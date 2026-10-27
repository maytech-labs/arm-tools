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
        id: "abi/callee-saved-restore",
        default_level: Level::Warning,
        summary: "Returns without restoring a callee-saved register it saved",
        explanation: "The function saves x19 to x28 or d8 to d15, changes it, and then returns along some path without loading the saved value back. Its caller gets the changed value. Restore every saved register before every ret, usually with the ldp that mirrors the prologue's stp.",
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
        id: "flow/flags-clobbered",
        default_level: Level::Warning,
        summary: "Reads the condition flags after a call that may have changed them",
        explanation: "A call doesn't preserve the condition flags (NZCV): `cmp x0, #0`, then `bl _f`, then `b.eq` branches on whatever _f left behind. Compare after the call, or save the result before it with cset and test the register afterward. Calls to helpers in the same file are trusted, since they may return a result in the flags.",
    },
    RuleInfo {
        id: "types/call",
        default_level: Level::Warning,
        summary: "An argument or return value that doesn't match the annotated signature",
        explanation: "With `; armtool: fn _f(x0: ptr<u8>, x1: i64) -> i32`, each call to _f is checked: x0 must hold a pointer and x1 a 64-bit integer where their types are known, and _f's own returns must leave an i32 in x0. A 32-bit signed value where 64 bits are wanted needs sign-extending first (sxtw). See docs/types.md.",
    },
    RuleInfo {
        id: "types/signedness",
        default_level: Level::Warning,
        summary: "A signed test on unsigned values, or the reverse",
        explanation: "After comparing unsigned values, b.gt, b.ge, b.lt, and b.le test the wrong flags; use b.hi, b.hs, b.lo, and b.ls. Signed values want the first set. sdiv and udiv must match their operands too. Types come from annotations; see docs/types.md.",
    },
    RuleInfo {
        id: "types/memory",
        default_level: Level::Warning,
        summary: "A load or store that doesn't fit the pointer's type",
        explanation: "Through a `ptr<i32>`, loads and stores move 4 bytes, offsets are multiples of 4, and pointer arithmetic moves by whole elements. Loading 8 bytes through it reads past the value; an offset of 6 lands mid-element. Using a non-pointer as an address is a finding too. Types come from annotations; see docs/types.md.",
    },
    RuleInfo {
        id: "types/format",
        default_level: Level::Warning,
        summary: "printf arguments that don't match the format string",
        explanation: "At a call to printf, fprintf, sprintf, snprintf, or dprintf, the format string is read from the data it points to, and each conversion is checked against its argument: `%d` wants an integer, `%f` a double, `%s` a pointer. On Darwin variadic arguments go on the stack, 8 bytes each, not in x1 and up; on Linux they go in registers. See docs/types.md.",
    },
    RuleInfo {
        id: "types/layout",
        default_level: Level::Warning,
        summary: "A field-offset constant that disagrees with the struct's layout",
        explanation: "With `; armtool: struct point { x: i32, y: i32 }`, constants named point_x, point_y, and point_size are checked against the layout C would give the struct: fields at the next multiple of their alignment, the size rounded up to the largest alignment. A stale offset reads the wrong field. See docs/types.md.",
    },
    RuleInfo {
        id: "types/pointer-width",
        default_level: Level::Warning,
        summary: "A pointer read through a 32-bit W register",
        explanation: "Pointers are 64 bits. Reading one as w0 instead of x0 keeps only the low half. On macOS every user-space address is above 4 GB, so the result is always wrong there; on Linux, heap and stack addresses usually are too. Use the X register.",
    },
    RuleInfo {
        id: "types/annotation",
        default_level: Level::Warning,
        summary: "A type annotation comment that can't be read",
        explanation: "`armtool:` comments declare types: `fn name(x0: T, ...) -> T` for a function, or `x9: T` for a register here. Types are i8 to i64, u8 to u64, f32, f64, `ptr`, `ptr<T>`, and declared structs. See docs/types.md.",
    },
    RuleInfo {
        id: "flow/dead-store",
        default_level: Level::Warning,
        summary: "A register set and never read",
        explanation: "On every path from here, the register is overwritten or the function returns before anything reads it, so the instruction does nothing useful. It's often a typo for another register, or a leftover. Remove it, or use the value. Loads, stores, compares, calls, and the frame registers x29, x30, and sp aren't reported.",
    },
    RuleInfo {
        id: "abi/syscall-number",
        default_level: Level::Warning,
        summary: "svc without the syscall number in x16 (Darwin) or x8 (Linux)",
        explanation: "On Darwin, svc #0x80 takes the system call number in x16, not x8 as on Linux. Set x16 before the svc, for example mov x16, #4 for write. On Linux, svc #0 takes it in x8: mov x8, #64 for write.",
    },
    RuleInfo {
        id: "elf/gnu-stack",
        default_level: Level::Warning,
        summary: "A Linux file with code but no .note.GNU-stack section",
        explanation: "Without `.section .note.GNU-stack,\"\",%progbits`, the GNU linker assumes the code needs an executable stack and makes the whole program's stack executable, which turns stack overflows into code execution. Recent linkers warn about it. Add the line at the end of every assembly file. Linux only.",
    },
    RuleInfo {
        id: "elf/function-symbol",
        default_level: Level::Warning,
        summary: "An exported Linux function without .type or .size",
        explanation: "On ELF, `.type name, %function` marks a symbol as code and `.size name, .-name` gives its length. Without them, debuggers, profilers, and crash reports can't place the function, and the dynamic linker treats it as data. Add `.type` before the label and `.size` after the last instruction. Linux only.",
    },
    RuleInfo {
        id: "style/unused-label",
        default_level: Level::Hint,
        summary: "A label nothing refers to",
        explanation: "The label isn't exported and nothing in the file branches to or loads it. Remove it, or export it with .globl if other files need it.",
    },
];

pub(crate) fn run(analysis: &Analysis, externals: &crate::CDefinitions) -> Vec<Finding> {
    let cfg = Cfg::new(analysis);
    let mut out = Vec::new();
    crate::types::run(analysis, &cfg, externals, &mut out);
    let darwin = analysis.platform() == Platform::Darwin;
    if darwin {
        x18(analysis, &mut out);
    } else {
        elf_symbols(analysis, &cfg, &mut out);
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
        let exported = analysis
            .symbols()
            .iter()
            .any(|s| s.name == function.name && s.exported);
        dead_stores(analysis, &cfg, &reached, exported, &mut out);
        undefined_registers(analysis, &cfg, function, entry, &mut out);
        callee_saved(function, &insns, &mut out);
        callee_saved_restore(analysis, &cfg, function, entry, &insns, &mut out);
        unbalanced_stack(analysis, &cfg, function, entry, &mut out);
        flags_clobbered(analysis, &cfg, entry, &mut out);
    }
    out
}

/// The ELF bookkeeping a Linux file needs: a `.note.GNU-stack` section, and
/// `.type` and `.size` for each exported function.
fn elf_symbols(analysis: &Analysis, cfg: &Cfg, out: &mut Vec<Finding>) {
    use arm_syntax::ast::Statement;
    let text = analysis.text();
    let mut typed = HashSet::new();
    let mut sized = HashSet::new();
    let mut gnu_stack = false;
    for statement in analysis.parse().tree().statements() {
        let Statement::Directive(directive) = statement else {
            continue;
        };
        let Some(name) = directive.name() else {
            continue;
        };
        let line = directive.syntax().text().to_string();
        let args = line
            .split_once(char::is_whitespace)
            .map_or("", |(_, a)| a)
            .trim();
        let first = args.split(',').next().unwrap_or("").trim().to_string();
        match name.text().to_ascii_lowercase().as_str() {
            ".type" if args.contains("function") => {
                typed.insert(first);
            }
            ".size" => {
                sized.insert(first);
            }
            ".section" | ".pushsection" if first == ".note.GNU-stack" => gnu_stack = true,
            _ => {}
        }
    }
    let has_code = analysis
        .items()
        .iter()
        .any(|i| matches!(i, Item::Instruction(_)));
    if has_code && !gnu_stack {
        let first_line = text.find('\n').unwrap_or(text.len());
        let line = "        .section .note.GNU-stack,\"\",%progbits";
        let ending = if text.ends_with('\n') || text.is_empty() {
            ""
        } else {
            "\n"
        };
        out.push(fixed(
            finding(
                "elf/gnu-stack",
                0..first_line,
                "no .note.GNU-stack section: the linker will make the stack executable. Add `.section .note.GNU-stack,\"\",%progbits` at the end of the file".into(),
            ),
            "Add the .note.GNU-stack section".into(),
            vec![(text.len()..text.len(), format!("{ending}\n{line}\n"))],
        ));
    }
    let line_start = |offset: usize| text[..offset].rfind('\n').map_or(0, |i| i + 1);
    for function in &cfg.functions {
        let exported = analysis
            .symbols()
            .iter()
            .any(|s| s.name == function.name && s.range == function.name_range && s.exported);
        if !exported {
            continue;
        }
        let name = &function.name;
        let start = line_start(function.name_range.start);
        if !typed.contains(name) {
            out.push(fixed(
                finding(
                    "elf/function-symbol",
                    function.name_range.clone(),
                    format!("'{name}' has no `.type {name}, %function`, so tools see data, not a function. Add it before the label"),
                ),
                format!("Add `.type {name}, %function`"),
                vec![(start..start, format!("        .type   {name}, %function\n"))],
            ));
        }
        let last = function.span.clone().last().map(|i| cfg.insns[i]);
        if let (false, Some(last)) = (sized.contains(name), last) {
            let end = text[last.range.end..]
                .find('\n')
                .map_or(text.len(), |i| last.range.end + i);
            out.push(fixed(
                finding(
                    "elf/function-symbol",
                    function.name_range.clone(),
                    format!("'{name}' has no `.size {name}, .-{name}`, so tools don't know where it ends. Add it after its last instruction"),
                ),
                format!("Add `.size {name}, .-{name}`"),
                vec![(end..end, format!("\n        .size   {name}, .-{name}"))],
            ));
        }
    }
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

/// Callee-saved registers changed and not restored on some path to a return.
/// Only for registers the function saves somewhere: never saving them is
/// `abi/callee-saved`'s finding.
fn callee_saved_restore(
    analysis: &Analysis,
    cfg: &Cfg,
    function: &Function,
    entry: usize,
    insns: &[&Instruction],
    out: &mut Vec<Finding>,
) {
    use std::collections::BTreeSet;
    let saved: HashSet<String> = insns
        .iter()
        .flat_map(|i| flow::stored(i))
        .filter(|r| is_callee_saved(r))
        .collect();
    if saved.is_empty() {
        return;
    }
    // Changed and not yet restored, on some path: union where paths meet.
    let mut before: HashMap<usize, BTreeSet<String>> = HashMap::new();
    let mut work = vec![(entry, BTreeSet::new())];
    let mut reported = HashSet::new();
    while let Some((i, incoming)) = work.pop() {
        let mut merged: BTreeSet<String> = before.get(&i).cloned().unwrap_or_default();
        let size = merged.len();
        merged.extend(incoming);
        let grew = merged.len() > size;
        if before.contains_key(&i) && !grew {
            continue;
        }
        before.insert(i, merged.clone());
        let insn = cfg.insns[i];
        let mut after = merged;
        let restored = flow::restored(insn);
        for register in flow::writes(insn) {
            if saved.contains(&register) && !restored.contains(&register) {
                after.insert(register);
            }
        }
        for register in &restored {
            after.remove(register);
        }
        let head = insn.mnemonic.head.as_str();
        if matches!(head, "ret" | "retaa" | "retab") {
            for register in &after {
                if reported.insert(register.clone()) {
                    let shown = match register.strip_prefix('v') {
                        Some(n) => format!("d{n}"),
                        None => register.clone(),
                    };
                    out.push(finding(
                        "abi/callee-saved-restore",
                        insn.range.clone(),
                        format!(
                            "'{}' saves {shown} but returns here with it changed: load it back before this ret, on this path too",
                            function.name
                        ),
                    ));
                }
            }
        }
        for next in cfg.successors(analysis, i).next {
            work.push((next, after.clone()));
        }
    }
}

/// Register writes nothing reads, by backward liveness over the function.
/// A helper that isn't exported may return values in any register, by its
/// own convention; only exported functions are held to the ABI's.
fn dead_stores(
    analysis: &Analysis,
    cfg: &Cfg,
    reached: &[usize],
    exported: bool,
    out: &mut Vec<Finding>,
) {
    use std::collections::BTreeSet;
    type Live = BTreeSet<String>;
    let platform = analysis.platform();
    let x =
        |range: std::ops::RangeInclusive<u32>| range.map(|n| format!("x{n}")).collect::<Vec<_>>();
    let v =
        |range: std::ops::RangeInclusive<u32>| range.map(|n| format!("v{n}")).collect::<Vec<_>>();
    let everything: Live = x(0..=30)
        .into_iter()
        .chain(v(0..=31))
        .chain(["sp".to_string()])
        .collect();
    // What a caller can see after a return: results, and what the ABI
    // preserves.
    let at_return: Live = x(0..=1)
        .into_iter()
        .chain(v(0..=3))
        .chain(x(19..=30))
        .chain(v(8..=15))
        .chain(["sp".to_string()])
        .collect();
    // What a call or system call may read: its arguments.
    let arguments: Live = x(0..=8)
        .into_iter()
        .chain(v(0..=7))
        .chain(["x16".to_string()])
        .collect();
    let mut live_in: HashMap<usize, Live> = HashMap::new();
    let live_out = |i: usize, live_in: &HashMap<usize, Live>| -> Live {
        let insn = cfg.insns[i];
        let successors = cfg.successors(analysis, i);
        let mut live: Live = successors
            .next
            .iter()
            .flat_map(|s| live_in.get(s).cloned().unwrap_or_default())
            .collect();
        let head = insn.mnemonic.head.as_str();
        let returns = matches!(head, "ret" | "retaa" | "retab");
        // Running off the end into whatever follows.
        let runs_off =
            successors.next.is_empty() && !successors.exits && !flow::is_terminator(insn);
        let leaves_unseen = runs_off
            || (returns && !exported)
            || successors.exits && !returns
            // A conditional branch to code outside the function.
            || (flow::target(analysis, insn).is_none()
                && (insn.mnemonic.condition.is_some()
                    || matches!(head, "cbz" | "cbnz" | "tbz" | "tbnz")));
        if leaves_unseen {
            live.extend(everything.iter().cloned());
        } else if returns {
            live.extend(at_return.iter().cloned());
        }
        live
    };
    // Grow to a fixed point, latest instructions first.
    let mut order: Vec<usize> = reached.to_vec();
    order.sort_unstable_by(|a, b| b.cmp(a));
    loop {
        let mut changed = false;
        for &i in &order {
            let insn = cfg.insns[i];
            let mut live = live_out(i, &live_in);
            for written in flow::writes(insn) {
                live.remove(&written);
            }
            live.extend(flow::reads(insn, platform));
            // A call reads its arguments; a helper in this file may read
            // anything, by its own convention.
            if calls_local_helper(analysis, insn) {
                live.extend(everything.iter().cloned());
            } else if flow::is_call(insn) || insn.mnemonic.head == "svc" {
                live.extend(arguments.iter().cloned());
            }
            if live_in.get(&i) != Some(&live) {
                live_in.insert(i, live);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut ordered = reached.to_vec();
    ordered.sort_unstable();
    for i in ordered {
        let insn = cfg.insns[i];
        let head = insn.mnemonic.head.as_str();
        // Instructions that matter for more than their register.
        let effects = head.starts_with("ld")
            || head.starts_with("st")
            || matches!(head, "swp" | "mrs" | "sys" | "sysl" | "svc" | "hvc" | "smc")
            || head.starts_with("cas")
            || head.starts_with("swp")
            || flow::is_call(insn)
            || flow::sets_flags(insn, platform)
            || insn.operands.is_none();
        if effects {
            continue;
        }
        let live = live_out(i, &live_in);
        let Some(first) = operands(insn).first() else {
            continue;
        };
        for written in flow::writes(insn) {
            if matches!(written.as_str(), "x29" | "x30" | "sp") || live.contains(&written) {
                continue;
            }
            let shown = match first {
                Operand::Register(r)
                    if flow::register_id(first).as_deref() == Some(written.as_str()) =>
                {
                    r.register.name().to_string()
                }
                _ => written.clone(),
            };
            out.push(finding(
                "flow/dead-store",
                insn.range.clone(),
                format!(
                    "{shown} is set here but never read: every path overwrites it or returns first. Remove this, or use the value"
                ),
            ));
        }
    }
}

/// Flag reads that a call may have clobbered on some path since the flags
/// were last set.
fn flags_clobbered(analysis: &Analysis, cfg: &Cfg, entry: usize, out: &mut Vec<Finding>) {
    // The call that last clobbered the flags, on some path: None if they're
    // set (or unknown, as at entry) on every path.
    let mut before: HashMap<usize, Option<usize>> = HashMap::new();
    let mut work = vec![(entry, None)];
    let platform = analysis.platform();
    while let Some((i, incoming)) = work.pop() {
        let merged = match (before.get(&i), incoming) {
            (Some(Some(old)), _) => Some(*old),
            (_, new) => new,
        };
        if before.get(&i) == Some(&merged) {
            continue;
        }
        before.insert(i, merged);
        let insn = cfg.insns[i];
        let after = if flow::sets_flags(insn, platform) {
            None
        } else if flow::is_call(insn) && !calls_local_helper(analysis, insn) {
            Some(i)
        } else if flow::is_call(insn) {
            // A local helper may return its result in the flags.
            None
        } else {
            merged
        };
        for next in cfg.successors(analysis, i).next {
            work.push((next, after));
        }
    }
    let mut indices: Vec<usize> = before.keys().copied().collect();
    indices.sort_unstable();
    for i in indices {
        let insn = cfg.insns[i];
        let (true, Some(call)) = (flow::reads_flags(insn), before[&i]) else {
            continue;
        };
        let line = analysis
            .line_index()
            .line_col(
                analysis.text(),
                cfg.insns[call].range.start,
                arm_sema::Encoding::Utf8,
            )
            .line
            + 1;
        out.push(finding(
            "flow/flags-clobbered",
            insn.range.clone(),
            format!(
                "this reads the condition flags, but the call on line {line} may have changed them: calls don't preserve NZCV. Compare after the call, or save the result with cset before it"
            ),
        ));
    }
}

/// Argument registers a call doesn't return values in: x2 to x7, and v4 to
/// v7 (v0 to v3 can hold a floating-point aggregate result).
fn argument_registers() -> impl Iterator<Item = String> {
    (2..=7)
        .map(|n| format!("x{n}"))
        .chain((4..=7).map(|n| format!("v{n}")))
}

/// Whether a call goes to a function this file defines without exporting:
/// a helper whose conventions are its own.
fn calls_local_helper(analysis: &Analysis, insn: &Instruction) -> bool {
    insn.mnemonic.head == "bl"
        && flow::target(analysis, insn).is_some_and(|s| !analysis.symbols()[s].exported)
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
            // Argument registers past the return value are garbage after a
            // call too, unless it's to a helper in this file, which may
            // return more by its own convention.
            for register in argument_registers() {
                if calls_local_helper(analysis, insn) {
                    after.remove(&register);
                } else {
                    after.insert(register, origin.clone());
                }
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
        let saved = ".globl _f\n_f:\n  str x19, [sp, #-16]!\n  mov x19, x0\n  add x0, x19, #1\n  ldr x19, [sp], #16\n  ret\n";
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
        // ELF bookkeeping has its own test.
        .filter(|d| d.code != "style/unused-label" && !d.code.starts_with("elf/"))
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
        let ok = ".globl main\nmain:\n  str x30, [sp, #-16]!\n  mov x18, #1\n  mov x0, x18\n  bl puts\n  ldr x30, [sp], #16\n  ret\n";
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
    fn callee_saved_restore() {
        // Restored on one path, not the other.
        let early = ".globl _f\n_f:\n  stp x19, x20, [sp, #-16]!\n  mov x19, x0\n  cbz x0, 1f\n  ldp x19, x20, [sp], #16\n  ret\n1:\n  add sp, sp, #16\n  ret\n";
        let found = findings(early);
        assert_eq!(
            found,
            [("abi/callee-saved-restore", "ret".to_string())],
            "{found:?}"
        );
        // Restored on every path: fine.
        let good = ".globl _f\n_f:\n  stp x19, x20, [sp, #-16]!\n  mov x19, x0\n  cbz x0, 1f\n  add x0, x19, #1\n1:\n  ldp x19, x20, [sp], #16\n  ret\n";
        assert_eq!(findings(good), []);
        // Restored with ldr from the frame pointer, or a d register.
        let fp = ".globl _f\n_f:\n  stp x29, x30, [sp, #-32]!\n  mov x29, sp\n  str d8, [x29, #16]\n  fmov d8, x0\n  fadd d0, d8, d8\n  ldr d8, [x29, #16]\n  ldp x29, x30, [sp], #32\n  ret\n";
        assert_eq!(findings(fp), []);
        // Never saved at all is abi/callee-saved, not this.
        let never = ".globl _f\n_f:\n  mov x19, x0\n  ret\n";
        assert_eq!(rules(never), ["abi/callee-saved"]);
    }

    #[test]
    fn argument_registers_after_calls() {
        // printf may have changed x2.
        let after = ".globl _f\n_f:\n  stp x29, x30, [sp, #-16]!\n  mov x29, sp\n  mov x2, #1\n  bl _printf\n  add x0, x0, x2\n  ldp x29, x30, [sp], #16\n  ret\n";
        let found = findings(after);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, "flow/undefined-register");
        // x0 and x1 are results; a local helper may return more.
        let fine = ".globl _f\n_f:\n  stp x29, x30, [sp, #-16]!\n  mov x29, sp\n  bl _printf\n  mov x3, x1\n  bl helper\n  add x0, x2, x3\n  ldp x29, x30, [sp], #16\n  ret\nhelper:\n  mov x2, #1\n  ret\n";
        assert_eq!(findings(fine), []);
        // At entry, arguments are defined.
        let entry = ".globl _f\n_f:\n  add x0, x6, x7\n  ret\n";
        assert_eq!(findings(entry), []);
    }

    #[test]
    fn elf_bookkeeping() {
        let lint_linux = |text: &str| {
            lint(
                &Analysis::for_platform(text, arm_sema::Platform::Linux),
                &Config::default(),
            )
        };
        let bare = ".globl main\nmain:\n    mov w0, #0\n    ret\n";
        let found = lint_linux(bare);
        let codes: Vec<&str> = found.iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            [
                "elf/gnu-stack",
                "elf/function-symbol",
                "elf/function-symbol"
            ],
            "{found:?}"
        );
        // Each fix, applied, makes its finding go away.
        let mut fixed = bare.to_string();
        let mut edits: Vec<(std::ops::Range<usize>, String)> = found
            .iter()
            .flat_map(|d| d.fix.clone().expect("a fix").edits)
            .collect();
        edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
        for (range, text) in edits {
            fixed.replace_range(range, &text);
        }
        assert_eq!(lint_linux(&fixed), [], "{fixed}");
        assert!(fixed.contains(".type   main, %function\nmain:"), "{fixed}");
        assert!(
            fixed.contains("ret\n        .size   main, .-main"),
            "{fixed}"
        );
        // Not for local functions, data-only files, or Darwin.
        let local = "bl helper\nhelper:\n    ret\n.section .note.GNU-stack,\"\",%progbits\n";
        assert_eq!(lint_linux(local), []);
        assert_eq!(lint_linux(".data\n.globl x\nx: .word 1\n"), []);
        assert!(rules(bare).iter().all(|r| !r.starts_with("elf/")));
    }

    #[test]
    fn flags_clobbered() {
        let frame = |body: &str| {
            format!(
                ".globl _f\n_f:\n  stp x29, x30, [sp, #-16]!\n  mov x29, sp\n{body}  ldp x29, x30, [sp], #16\n  ret\n"
            )
        };
        let after_call = frame("  cmp x0, #0\n  bl _g\n  b.eq 1f\n  mov x0, #1\n1:\n");
        assert_eq!(
            findings(&after_call),
            [("flow/flags-clobbered", "b.eq 1f".to_string())]
        );
        // csel and adc read flags too; a compare after the call is fine.
        let csel = frame("  cmp x0, #0\n  bl _g\n  csel x0, x0, x1, eq\n");
        assert_eq!(rules(&csel), ["flow/flags-clobbered"]);
        let fixed = frame("  bl _g\n  cmp x0, #0\n  csel x0, x0, xzr, eq\n");
        assert_eq!(rules(&fixed), Vec::<&str>::new());
        // On one path only: still a finding.
        let one_path = frame("  cmp x0, #0\n  cbz x1, 2f\n  bl _g\n2:\n  cset x0, eq\n");
        assert_eq!(rules(&one_path), ["flow/flags-clobbered"]);
        // A local helper may return its result in the flags.
        let helper = ".globl _f\n_f:\n  stp x29, x30, [sp, #-16]!\n  mov x29, sp\n  bl check\n  cset x0, eq\n  ldp x29, x30, [sp], #16\n  ret\ncheck:\n  cmp x0, #4\n  ret\n";
        assert_eq!(rules(helper), Vec::<&str>::new());
        // Darwin's svc sets the carry flag for errors.
        let svc = ".globl _f\n_f:\n  mov x16, #4\n  svc #0x80\n  b.cs 1f\n  ret\n1:\n  neg x0, x0\n  ret\n";
        assert_eq!(rules(svc), Vec::<&str>::new());
    }

    #[test]
    fn dead_stores() {
        let dead = ".globl _f\n_f:\n  mov x9, #1\n  mov x9, #2\n  add x0, x9, #1\n  ret\n";
        assert_eq!(
            findings(dead),
            [("flow/dead-store", "mov x9, #1".to_string())]
        );
        // Overwritten on one path only: live.
        let maybe = ".globl _f\n_f:\n  mov x9, #1\n  cbz x0, 1f\n  mov x9, #2\n1:\n  add x0, x9, #1\n  ret\n";
        assert_eq!(rules(maybe), Vec::<&str>::new());
        // Results, callee-saved registers, and arguments to calls are read.
        let fine = ".globl _f\n_f:\n  stp x29, x30, [sp, #-16]!\n  mov x29, sp\n  mov x1, #3\n  bl _g\n  mov x0, #0\n  mov x1, #0\n  ldp x29, x30, [sp], #16\n  ret\n";
        assert_eq!(rules(fine), Vec::<&str>::new());
        // movk and lane inserts keep the rest of the register.
        let pieces = ".globl _f\n_f:\n  movz x0, #1\n  movk x0, #2, lsl #16\n  ret\n";
        assert_eq!(rules(pieces), Vec::<&str>::new());
        // Loads, compares, and the frame registers aren't reported.
        let quiet = ".globl _f\n_f:\n  ldr x9, [x0]\n  subs x10, x0, #1\n  b.eq 1f\n1:\n  mov x29, sp\n  ret\n";
        assert_eq!(rules(quiet), Vec::<&str>::new());
        // A tail call may read anything.
        let tail = ".globl _f\n_f:\n  mov x5, #1\n  b _g\n";
        assert_eq!(rules(tail), Vec::<&str>::new());
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
