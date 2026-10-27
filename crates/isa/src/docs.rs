//! Text for people: readable instruction signatures, what each register is
//! for on Darwin, and what common directives do. Written by us; Arm's openly
//! licensed spec has no prose (docs/decisions/0013-isa-from-llvm-tablegen.md).

use crate::{ClassKind, Form, OperandClass, Register, RegisterKind};

impl Form {
    /// The form as you'd write it, with placeholders for operands:
    /// `ldp Xt, Xt, [Xn|SP, #-512..504 (×8)]!`.
    pub fn signature(self) -> String {
        let operands: Vec<OperandClass> = self.operands().collect();
        let mut out = self.mnemonic().to_string();
        let mut rest = operands.as_slice();
        // `b.eq` is `b`, `.`, a condition code, then the label.
        if let [dot, cond, tail @ ..] = rest
            && dot.kind() == ClassKind::Token
            && dot.name() == "."
        {
            out.push('.');
            out.push_str(&placeholder(*cond));
            rest = tail;
        }
        let mut first = true;
        let mut after_open = false;
        for class in rest {
            let name = class.name();
            let attached = class.kind() == ClassKind::Token
                && (matches!(name, "]" | "!") || name.starts_with('.'));
            if attached {
                out.push_str(name);
            } else {
                if first {
                    out.push(' ');
                } else if !after_open {
                    out.push_str(", ");
                }
                out.push_str(&placeholder(*class));
                first = false;
            }
            after_open = class.kind() == ClassKind::Token && name == "[";
        }
        out
    }
}

/// How an operand class reads in a signature.
fn placeholder(class: OperandClass) -> String {
    let name = class.name();
    match class.kind() {
        ClassKind::Token => return name.to_string(),
        ClassKind::Register => {
            if let Some(text) = register_class_placeholder(name) {
                return text.to_string();
            }
        }
        _ => {}
    }
    if let Some(text) = predicate_placeholder(class.predicate()) {
        return text;
    }
    let text = match name {
        "CondCode" => "<cond>",
        "AddSubImm" | "AddSubImmNeg" => "#imm12{, lsl #12}",
        "LogicalImm32" | "LogicalImm64" | "LogicalImm32Not" | "LogicalImm64Not" => "#bitmask",
        "PCRelLabel19" | "PCRelLabel14" | "BranchTarget26" | "BranchTarget14" => "<label>",
        "AdrpLabel" => "<label>@PAGE",
        "AdrLabel" => "<label>",
        "ArithmeticShifter32" | "ArithmeticShifter64" => "{lsl|lsr|asr #amount}",
        "LogicalShifter32" | "LogicalShifter64" => "{lsl|lsr|asr|ror #amount}",
        "MovImm32Shifter" | "MovImm64Shifter" => "{lsl #shift}",
        "Barrier" => "<barrier>",
        "Prefetch" => "<prfop>",
        "MRSSystemRegister" | "MSRSystemRegister" => "<sysreg>",
        _ => return format!("<{name}>"),
    };
    text.to_string()
}

fn register_class_placeholder(name: &str) -> Option<&'static str> {
    Some(match name {
        "GPR64" | "GPR64common" | "GPR64z" | "GPR64x8" | "GPR64noip" => "Xn",
        "GPR32" | "GPR32common" | "GPR32z" => "Wn",
        "GPR64sp" | "GPR64sponly" => "Xn|SP",
        "GPR32sp" | "GPR32sponly" => "Wn|WSP",
        "FPR8" => "Bn",
        "FPR16" => "Hn",
        "FPR32" => "Sn",
        "FPR64" => "Dn",
        "FPR128" => "Qn",
        _ => return None,
    })
}

/// `#lo..hi` for range predicates, from LLVM's predicate names.
fn predicate_placeholder(predicate: &str) -> Option<String> {
    let (family, args) = predicate.split_once('<')?;
    let args: Vec<i64> = args
        .trim_end_matches('>')
        .split(',')
        .map(|a| a.trim().parse().ok())
        .collect::<Option<_>>()?;
    match (family, args.as_slice()) {
        ("isImmInRange", [lo, hi]) => Some(format!("#{lo}..{hi}")),
        ("isUImm12Offset", [scale]) => Some(format!("#0..{} (×{scale})", 4095 * scale)),
        ("isSImmScaled", [bits, scale]) => {
            let half = 1i64 << (bits - 1);
            Some(format!(
                "#{}..{} (×{scale})",
                -half * scale,
                (half - 1) * scale
            ))
        }
        ("isUImmScaled", [bits, scale]) => {
            Some(format!("#0..{} (×{scale})", ((1i64 << bits) - 1) * scale))
        }
        _ => None,
    }
}

impl Register {
    /// What the register is, and its role in Apple's arm64 ABI.
    pub fn description(self) -> String {
        let name = self.name();
        let kind = self.kind();
        if kind != RegisterKind::Scalar {
            return match kind {
                RegisterKind::NeonVector => format!(
                    "{name}: 128-bit SIMD and floating-point vector register. Write an arrangement like {name}.4s or {name}.16b.{}",
                    vector_abi(name)
                ),
                RegisterKind::SveData => format!("{name}: SVE scalable vector register."),
                RegisterKind::SvePredicate => format!("{name}: SVE predicate register."),
                RegisterKind::SvePredicateAsCounter => {
                    format!("{name}: SVE predicate-as-counter register.")
                }
                RegisterKind::Matrix | RegisterKind::MatrixTileList => {
                    format!("{name}: SME ZA array or tile.")
                }
                RegisterKind::LookupTable => format!("{name}: SME2 lookup table register."),
                RegisterKind::Scalar => String::new(),
            };
        }
        let number: Option<u32> = name.get(1..).and_then(|n| n.parse().ok());
        match (name.as_bytes().first(), number) {
            (Some(b'x'), Some(n)) => format!("{name}: 64-bit general-purpose register.{}", gpr_abi(n)),
            (Some(b'w'), Some(n)) => {
                format!("{name}: 32-bit general-purpose register, the low half of x{n}. Writing it zeroes the high half.{}", gpr_abi(n))
            }
            (Some(b'b'), Some(n)) => format!("{name}: 8-bit SIMD and floating-point register, the low byte of v{n}."),
            (Some(b'h'), Some(n)) => format!("{name}: 16-bit SIMD and floating-point register, the low half of s{n}."),
            (Some(b's'), Some(n)) => format!("{name}: 32-bit floating-point register, the low half of d{n}."),
            (Some(b'd'), Some(n)) => {
                format!("{name}: 64-bit floating-point register, the low half of v{n}.{}", vector_abi(&format!("v{n}")))
            }
            (Some(b'q'), Some(n)) => {
                format!("{name}: 128-bit SIMD and floating-point register, the same as v{n}.{}", vector_abi(&format!("v{n}")))
            }
            _ => match name {
                "sp" => "sp: the stack pointer. It must stay 16-byte aligned whenever it's used to access memory.".into(),
                "wsp" => "wsp: the low 32 bits of the stack pointer.".into(),
                "xzr" | "x31" => format!("{name}: the zero register. Reads as 0; writes are discarded."),
                "wzr" | "w31" => format!("{name}: the 32-bit zero register. Reads as 0; writes are discarded."),
                "fp" => format!("fp: another name for x29.{}", gpr_abi(29)),
                "lr" => format!("lr: another name for x30.{}", gpr_abi(30)),
                _ => format!("{name}: {} register.", self.llvm_name()),
            },
        }
    }
}

/// The register's role in Apple's arm64 calling convention.
fn gpr_abi(n: u32) -> &'static str {
    match n {
        0 => " Holds the first argument and the return value.",
        1..=7 => " Holds an argument; x1 also holds the second half of a 128-bit return value.",
        8 => " Holds the address for an indirect (large struct) return value.",
        9..=15 => " Scratch: callers can't rely on it surviving a call.",
        16 | 17 => {
            " Intra-procedure-call scratch (IP0/IP1): the linker's veneers may overwrite it, and Darwin passes the syscall number in x16."
        }
        18 => {
            " Reserved by Darwin for the platform. Don't use it: the OS may overwrite it at any time."
        }
        19..=28 => " Callee-saved: a function that writes it must save and restore it.",
        29 => " The frame pointer (fp). Darwin requires a valid frame record chain.",
        30 => " The link register (lr): bl and blr put the return address here.",
        _ => "",
    }
}

fn vector_abi(v: &str) -> &'static str {
    match v.trim_start_matches('v').parse::<u32>() {
        Ok(0..=7) => " Holds floating-point and vector arguments and results.",
        Ok(8..=15) => " The low 64 bits (d8 to d15) are callee-saved.",
        Ok(_) => " Scratch: callers can't rely on it surviving a call.",
        Err(_) => "",
    }
}

/// The name of a Darwin system call by number, for the common ones: what
/// `svc #0x80` does with that number in x16. From XNU's syscalls.master.
pub fn syscall_name(number: i64) -> Option<&'static str> {
    Some(match number {
        1 => "exit",
        2 => "fork",
        3 => "read",
        4 => "write",
        5 => "open",
        6 => "close",
        7 => "wait4",
        9 => "link",
        10 => "unlink",
        12 => "chdir",
        15 => "chmod",
        20 => "getpid",
        24 => "getuid",
        37 => "kill",
        42 => "pipe",
        54 => "ioctl",
        59 => "execve",
        73 => "munmap",
        74 => "mprotect",
        97 => "socket",
        98 => "connect",
        116 => "gettimeofday",
        136 => "mkdir",
        137 => "rmdir",
        194 => "getrlimit",
        197 => "mmap",
        199 => "lseek",
        202 => "sysctl",
        338 => "stat64",
        339 => "fstat64",
        _ => return None,
    })
}

/// What a directive does, for the common ones.
pub fn directive_description(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    Some(match name.as_str() {
        ".text" => "Switch to the code section (__TEXT,__text).",
        ".data" => "Switch to the writable data section (__DATA,__data).",
        ".bss" => "Switch to the zero-filled data section.",
        ".const" => "Switch to the read-only constant section (__TEXT,__const).",
        ".cstring" => "Switch to the C string literal section (__TEXT,__cstring).",
        ".section" => {
            "Switch to a named section: .section __SEGMENT,__section[,type[,attributes]]."
        }
        ".globl" | ".global" => "Make a symbol visible to the linker, so other files can use it.",
        ".private_extern" => {
            "Make a symbol visible to other files in the same linked image, but not exported from it."
        }
        ".extern" => {
            "Declare a symbol defined elsewhere. Optional: undefined symbols are external anyway."
        }
        ".p2align" => {
            "Align to a power of two: .p2align 2 aligns to 4 bytes. Instructions need .p2align 2."
        }
        ".align" => "Align to a power of two on Darwin: .align 3 aligns to 8 bytes.",
        ".balign" => "Align to a number of bytes: .balign 16.",
        ".byte" => "Emit 1-byte values.",
        ".short" | ".hword" | ".2byte" => "Emit 2-byte values.",
        ".long" | ".int" | ".word" | ".4byte" => "Emit 4-byte values.",
        ".quad" | ".xword" | ".8byte" | ".dword" => "Emit 8-byte values.",
        ".octa" => "Emit 16-byte values.",
        ".float" | ".single" => "Emit 32-bit floats.",
        ".double" => "Emit 64-bit floats.",
        ".ascii" => "Emit a string without a terminating zero.",
        ".asciz" | ".string" => "Emit a string with a terminating zero.",
        ".space" | ".skip" | ".zero" => {
            "Emit a number of bytes, zero unless a fill value is given."
        }
        ".fill" => "Emit repeated values: .fill count, size, value.",
        ".zerofill" => {
            "Reserve zero-filled space in a section: .zerofill __DATA,__bss,symbol,size,align."
        }
        ".comm" => "Declare a common symbol: .comm name, size, align.",
        ".lcomm" => "Declare a local common symbol: .lcomm name, size, align.",
        ".set" | ".equ" => "Give a symbol a value: .set name, expression. It can be set again.",
        ".equiv" => "Give a symbol a value, failing if it already has one.",
        ".macro" => {
            "Start a macro: .macro name param1, param2=default. Use parameters as \\param1. End with .endm."
        }
        ".endm" | ".endmacro" => "End a macro definition.",
        ".rept" => "Repeat the lines up to .endr a number of times.",
        ".irp" => {
            "Repeat the lines up to .endr once per value: .irp r, x0, x1. Use the value as \\r."
        }
        ".irpc" => "Repeat the lines up to .endr once per character of a string.",
        ".endr" => "End a .rept, .irp, or .irpc block.",
        ".if" => "Assemble the lines up to .else or .endif only if the expression is nonzero.",
        ".ifdef" => "Assemble the following lines only if the symbol is defined.",
        ".ifndef" => "Assemble the following lines only if the symbol isn't defined.",
        ".else" => "The other branch of an .if.",
        ".elseif" => "Another condition in an .if chain.",
        ".endif" => "End an .if block.",
        ".include" => "Insert another file here: .include \"file.s\".",
        ".req" => "Name a register: name .req x19. End it with .unreq name.",
        ".unreq" => "End a register alias made with .req.",
        ".cfi_startproc" => "Start the unwind information for a function. Pair with .cfi_endproc.",
        ".cfi_endproc" => "End the unwind information started by .cfi_startproc.",
        ".cfi_def_cfa" => "Unwind info: the canonical frame address is register + offset.",
        ".cfi_def_cfa_offset" => {
            "Unwind info: the canonical frame address is now at this offset from its register."
        }
        ".cfi_offset" => {
            "Unwind info: a register is saved at this offset from the canonical frame address."
        }
        ".subsections_via_symbols" => {
            "Tell the linker each symbol starts a separate block it may dead-strip or reorder. Compilers emit it on Darwin."
        }
        ".build_version" => {
            "Record the platform and minimum OS version: .build_version macos, 14, 0."
        }
        ".loh" => {
            "A linker optimization hint: tells ld64 which adrp/add/ldr sequences it may simplify."
        }
        ".alt_entry" => {
            "Mark a symbol as an alternate entry point into the preceding function, so the linker keeps them together."
        }
        ".weak_definition" => {
            "Make a definition weak: another strong definition wins at link time."
        }
        ".weak_reference" => "Make a reference weak: it's null if no definition is linked.",
        ".no_dead_strip" => "Keep a symbol even if nothing references it.",
        ".inst" => "Emit a raw 32-bit instruction encoding.",
        ".ltorg" | ".pool" => "Emit the literal pool for ldr x0, =value here.",
        ".arch" | ".arch_extension" | ".cpu" => {
            "Change which architecture features the assembler accepts below this line."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RegisterKind, forms};

    /// Every signature of an opcode: optional operands give it several.
    fn signatures(mnemonic: &str, opcode: &str) -> Vec<String> {
        let all: Vec<_> = forms(mnemonic)
            .filter(|f| f.opcode() == opcode)
            .map(Form::signature)
            .collect();
        assert!(!all.is_empty(), "no {opcode}");
        all
    }

    #[test]
    fn signatures_read_like_assembly() {
        let has = |mnemonic, opcode, expected: &str| {
            let all = signatures(mnemonic, opcode);
            assert!(all.iter().any(|s| s == expected), "{all:?}");
        };
        has("add", "ADDXri", "add Xn|SP, Xn|SP, #imm12{, lsl #12}");
        has("ldp", "LDPXpre", "ldp Xn, Xn, [Xn|SP, #-512..504 (×8)]!");
        has("ldr", "LDRXui", "ldr Xn, [Xn|SP, #0..32760 (×8)]");
        has("ldr", "LDRXui", "ldr Xn, [Xn|SP]");
        has("b", "Bcc", "b.<cond> <label>");
        has("ret", "RET", "ret Xn");
    }

    fn scalar(name: &str) -> Register {
        Register::lookup_kind(name, RegisterKind::Scalar).expect("register")
    }

    #[test]
    fn register_descriptions_include_the_darwin_abi() {
        assert!(scalar("x18").description().contains("Reserved by Darwin"));
        assert!(scalar("x19").description().contains("Callee-saved"));
        assert!(scalar("w0").description().contains("low half of x0"));
        assert!(scalar("d8").description().contains("callee-saved"));
        assert!(scalar("sp").description().contains("16-byte aligned"));
        assert!(scalar("lr").description().contains("x30"));
        let v0 = Register::lookup_kind("v0", RegisterKind::NeonVector).expect("v0");
        assert!(v0.description().contains("arguments"));
    }

    #[test]
    fn syscall_names() {
        assert_eq!(syscall_name(4), Some("write"));
        assert_eq!(syscall_name(1), Some("exit"));
        assert_eq!(syscall_name(99_999), None);
    }

    #[test]
    fn directive_descriptions() {
        assert!(directive_description(".P2ALIGN").is_some());
        assert!(directive_description(".frob").is_none());
        for name in [".text", ".globl", ".macro", ".loh"] {
            assert!(
                crate::is_directive(name),
                "{name} described but not a directive"
            );
        }
    }
}
