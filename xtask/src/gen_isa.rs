//! `cargo xtask gen-isa`: generates `crates/isa/src/generated.rs` from LLVM's
//! AArch64 assembler matcher (docs/decisions/0014-isa-from-asm-matcher.md).
//!
//! It runs `llvm-tblgen --gen-asm-matcher` on an LLVM checkout and reads the
//! C++ it writes: the operand classes, the match table, feature sets,
//! register names, and register classes. Operand error messages come from
//! `showMatchError` in `AArch64AsmParser.cpp`. The parsing is line-based and
//! strict: anything unexpected is an error, so a new LLVM layout fails loudly.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask gen-isa --llvm <llvm-project checkout> [--tblgen <llvm-tblgen>]

The checkout's tag must match llvm-tblgen's version (llvmorg-21.1.8 today).
It needs llvm/include and llvm/lib/Target/AArch64; a sparse checkout works:

  git clone --depth 1 --branch llvmorg-21.1.8 --filter=blob:none --sparse \\
      https://github.com/llvm/llvm-project /tmp/llvm21
  git -C /tmp/llvm21 sparse-checkout set llvm/include/llvm llvm/lib/Target/AArch64 \\
      llvm/lib/MC/MCParser

Directives are probed with `xcrun clang`, so this runs on macOS only.
";

const OUTPUT: &str = "crates/isa/src/generated.rs";

pub fn run(args: Vec<String>) -> ExitCode {
    let mut llvm = None;
    let mut tblgen = PathBuf::from("llvm-tblgen");
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match (arg.as_str(), args.next()) {
            ("--llvm", Some(v)) => llvm = Some(PathBuf::from(v)),
            ("--tblgen", Some(v)) => tblgen = PathBuf::from(v),
            _ => {
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(llvm) = llvm else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    match generate(&llvm, &tblgen) {
        Ok(stats) => {
            println!("wrote {OUTPUT}: {stats}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("gen-isa: {error}");
            ExitCode::FAILURE
        }
    }
}

fn generate(llvm: &Path, tblgen: &Path) -> Result<String, String> {
    let target = llvm.join("llvm/lib/Target/AArch64");
    let version = tblgen_version(tblgen)?;
    let matcher = run_tblgen(tblgen, "--gen-asm-matcher", llvm, &target)?;
    let register_info = run_tblgen(tblgen, "--gen-register-info", llvm, &target)?;
    let parser_cpp = read(&target.join("AsmParser/AArch64AsmParser.cpp"))?;

    let classes = parse_classes(&matcher)?;
    let class_index: HashMap<&str, usize> = classes
        .iter()
        .enumerate()
        .map(|(i, c)| (c.ident.as_str(), i))
        .collect();
    let predicates = parse_predicates(&matcher)?;
    let messages = parse_messages(&parser_cpp)?;
    let subclass = parse_subclass(&matcher)?;
    let feature_names = parse_feature_names(&matcher)?;
    let feature_sets = parse_feature_sets(&matcher)?;
    let forms = parse_forms(&matcher, &class_index)?;
    let reg_numbers = parse_register_enum(&register_info)?;
    let reg_classes = parse_register_classes(&matcher)?;
    let reg_names = parse_register_names(&matcher, &parser_cpp)?;
    let directives = probe_directives(&[
        read(&llvm.join("llvm/lib/MC/MCParser/AsmParser.cpp"))?,
        read(&llvm.join("llvm/lib/MC/MCParser/DarwinAsmParser.cpp"))?,
        parser_cpp.clone(),
    ])?;

    if reg_numbers.len() != reg_classes.len() {
        return Err(format!(
            "{} registers in the enum but {} in the class table",
            reg_numbers.len(),
            reg_classes.len()
        ));
    }
    if subclass.table.len() != classes.len() {
        return Err(format!(
            "isSubclass has {} entries for {} classes",
            subclass.table.len(),
            classes.len()
        ));
    }

    let json = crate::gen_features::dump_json(tblgen, llvm, &target)?;
    let mut bits: Vec<&str> = feature_sets
        .iter()
        .flat_map(|(_, bits)| bits.iter().map(String::as_str))
        .collect();
    bits.sort_unstable();
    bits.dedup();
    let feature_predicates: Vec<&str> = bits
        .iter()
        .map(|b| b.trim_start_matches("Feature_").trim_end_matches("Bit"))
        .collect();
    let features = crate::gen_features::extract(&json, &feature_predicates)?;
    drop(json);
    let mut out = render(&Render {
        version: &version,
        classes: &classes,
        predicates: &predicates,
        messages: &messages,
        subclass: &subclass,
        feature_names: &feature_names,
        feature_sets: &feature_sets,
        forms: &forms,
        reg_classes: &reg_classes,
        reg_names: &reg_names,
        reg_numbers: &reg_numbers,
        class_index: &class_index,
        directives: &directives,
    })?;
    features.render(&mut out, &feature_predicates);
    std::fs::write(OUTPUT, out).map_err(|e| format!("can't write {OUTPUT}: {e}"))?;
    let mnemonics: std::collections::BTreeSet<_> = forms.iter().map(|f| &f.mnemonic).collect();
    Ok(format!(
        "LLVM {version}, {} forms, {} mnemonics, {} operand classes, {} register names, {} directives, {}",
        forms.len(),
        mnemonics.len(),
        classes.len(),
        reg_names.len(),
        directives.len(),
        features.summary()
    ))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))
}

fn tblgen_version(tblgen: &Path) -> Result<String, String> {
    let out = Command::new(tblgen)
        .arg("--version")
        .output()
        .map_err(|e| format!("can't run {}: {e}. Pass --tblgen.", tblgen.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .skip_while(|w| *w != "version")
        .nth(1)
        .map(str::to_string)
        .ok_or_else(|| format!("can't read a version from `{} --version`", tblgen.display()))
}

fn run_tblgen(tblgen: &Path, mode: &str, llvm: &Path, target: &Path) -> Result<String, String> {
    let out = Command::new(tblgen)
        .arg(mode)
        .arg("-I")
        .arg(llvm.join("llvm/include"))
        .arg("-I")
        .arg(target)
        .arg(target.join("AArch64.td"))
        .output()
        .map_err(|e| format!("can't run {}: {e}", tblgen.display()))?;
    if !out.status.success() {
        return Err(format!(
            "llvm-tblgen failed. Does the checkout's tag match its version?\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| "llvm-tblgen wrote invalid UTF-8".to_string())
}

/// The lines between `start` (a line prefix) and the next line equal to `end`.
fn block<'a>(text: &'a str, start: &str, end: &str) -> Result<Vec<&'a str>, String> {
    let mut lines = text.lines().skip_while(|l| !l.starts_with(start));
    if lines.next().is_none() {
        return Err(format!("can't find `{start}`"));
    }
    Ok(lines.take_while(|l| l.trim_end() != end).collect())
}

/// LLVM register numbers, from the `AArch64::` enum in `--gen-register-info`:
/// `NoRegister,` then `FFR = 1,` and so on, up to `NUM_TARGET_REGS`.
fn parse_register_enum(info: &str) -> Result<HashMap<String, usize>, String> {
    let mut out = HashMap::new();
    let lines = info
        .lines()
        .skip_while(|l| l.trim() != "NoRegister,")
        .skip(1)
        .take_while(|l| !l.trim().starts_with("NUM_TARGET_REGS"));
    for line in lines {
        let Some((name, number)) = line.trim().trim_end_matches(',').split_once(" = ") else {
            return Err(format!("unexpected register enum line: {line}"));
        };
        let number = number.parse().map_err(|e| format!("{line}: {e}"))?;
        out.insert(name.to_string(), number);
    }
    if out.len() < 100 {
        return Err(format!("only {} registers in the enum", out.len()));
    }
    // NoRegister is 0.
    out.insert("NoRegister".to_string(), 0);
    Ok(out)
}

#[derive(Debug)]
struct Class {
    /// The C++ enumerator: `MCK_GPR64`, `MCK__DOT_16b`.
    ident: String,
    /// `GPR64`, `.16b`.
    name: String,
    kind: ClassKind,
}

#[derive(Debug, PartialEq)]
enum ClassKind {
    Invalid,
    Optional,
    Token,
    Register,
    Custom,
}

/// `enum MatchClassKind`, in order: the index is the class number.
fn parse_classes(matcher: &str) -> Result<Vec<Class>, String> {
    let mut classes = Vec::new();
    for line in block(matcher, "enum MatchClassKind {", "};")? {
        let line = line.trim();
        // `MCK_LAST_TOKEN = MCK__125_` and the like name an existing class.
        if line.is_empty()
            || line.starts_with("//")
            || line.starts_with("NumMatchClassKinds")
            || line.starts_with("MCK_LAST_")
        {
            continue;
        }
        let (ident, comment) = match line.split_once(',') {
            Some((ident, rest)) => (ident.trim(), rest.trim()),
            None => return Err(format!("unexpected class line: {line}")),
        };
        let ident = ident.split(" =").next().unwrap_or(ident).to_string();
        let comment = comment.strip_prefix("// ").unwrap_or(comment);
        let (name, kind) = if ident == "InvalidMatchClass" {
            (ident.clone(), ClassKind::Invalid)
        } else if ident == "OptionalMatchClass" {
            (ident.clone(), ClassKind::Optional)
        } else if let Some(token) = comment
            .strip_prefix('\'')
            .and_then(|c| c.strip_suffix('\''))
        {
            (token.to_string(), ClassKind::Token)
        } else if comment.contains("register class") {
            (strip_mck(&ident), ClassKind::Register)
        } else if comment.starts_with("user defined class") {
            (strip_mck(&ident), ClassKind::Custom)
        } else {
            return Err(format!("unknown class comment: {line}"));
        };
        classes.push(Class { ident, name, kind });
    }
    Ok(classes)
}

fn strip_mck(ident: &str) -> String {
    ident.strip_prefix("MCK_").unwrap_or(ident).to_string()
}

/// For each custom class: the predicate method and the diagnostic it reports
/// on a near miss, from `validateOperandClass`.
fn parse_predicates(matcher: &str) -> Result<HashMap<String, (String, String)>, String> {
    let lines = block(matcher, "static unsigned validateOperandClass(", "}")?;
    let mut out = HashMap::new();
    let mut current: Option<(String, String, String)> = None;
    for line in lines {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("case MCK_") {
            if let Some((name, predicate, diag)) = current.take() {
                out.insert(name, (predicate, diag));
            }
            let name = rest.trim_end_matches(": {").to_string();
            current = Some((name, String::new(), String::new()));
        } else if let Some(rest) = line.strip_prefix("DiagnosticPredicate DP(Operand.") {
            if let Some((_, predicate, _)) = current.as_mut() {
                *predicate = rest.trim_end_matches("());").to_string();
            }
        } else if let Some(rest) = line.strip_prefix("return AArch64AsmParser::Match_") {
            if let Some((_, _, diag)) = current.as_mut() {
                *diag = rest.trim_end_matches(';').to_string();
            }
        } else if line.starts_with("} // end switch") || line.starts_with("if (Operand.isReg())") {
            break;
        }
    }
    if let Some((name, predicate, diag)) = current {
        out.insert(name, (predicate, diag));
    }
    if out.is_empty() {
        return Err("no predicates in validateOperandClass".into());
    }
    Ok(out)
}

/// Operand error messages from `showMatchError`: each `case Match_X:` up to
/// the next `return Error(Loc, "...")` gets that message. Cases whose message
/// is computed are skipped.
fn parse_messages(cpp: &str) -> Result<BTreeMap<String, String>, String> {
    let lines = block(cpp, "bool AArch64AsmParser::showMatchError(", "}")?;
    let mut out = BTreeMap::new();
    let mut pending: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if let Some(rest) = line.strip_prefix("case Match_") {
            pending.push(
                rest.trim_end_matches(':')
                    .trim_end_matches(" {")
                    .to_string(),
            );
        } else if line.starts_with("return Error(Loc") {
            // The message can continue over several lines until `);`.
            let mut text = line.to_string();
            while !text.trim_end().ends_with(");") && i + 1 < lines.len() {
                i += 1;
                text.push_str(lines[i].trim());
            }
            if let Some(message) = string_literals(&text) {
                for case in pending.drain(..) {
                    out.insert(case, message.clone());
                }
            }
            pending.clear();
        } else if line.starts_with("return ") || line.starts_with("break;") {
            pending.clear();
        }
        i += 1;
    }
    if out.len() < 100 {
        return Err(format!("only {} messages in showMatchError", out.len()));
    }
    Ok(out)
}

/// The concatenation of the C string literals in `text`, if it's only literals
/// after `Loc,`.
fn string_literals(text: &str) -> Option<String> {
    let args = text.split_once("Loc,")?.1;
    let args = args.trim().strip_suffix(");")?.trim();
    let mut out = String::new();
    let mut chars = args.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        match chars.next() {
            None => return Some(out),
            Some('"') => {}
            Some(_) => return None,
        }
        loop {
            match chars.next()? {
                '"' => break,
                '\\' => out.push(chars.next()?),
                c => out.push(c),
            }
        }
    }
}

struct Subclass {
    /// Per class: offset into `data`, first superclass index, and count.
    table: Vec<(u32, u16, u16)>,
    data: Vec<u8>,
}

fn parse_subclass(matcher: &str) -> Result<Subclass, String> {
    let lines = block(matcher, "static bool isSubclass(", "}")?;
    let mut table = Vec::new();
    let mut data = Vec::new();
    let mut in_data = false;
    for line in lines {
        let line = line.trim();
        if line.starts_with("static constexpr uint8_t Data[]") {
            in_data = true;
        } else if in_data {
            if let Some(hex) = line.strip_prefix("0x") {
                let hex = hex.trim_end_matches(',');
                data.push(u8::from_str_radix(hex, 16).map_err(|e| format!("{line}: {e}"))?);
            }
        } else if let Some(row) = line.strip_prefix('{').and_then(|r| r.strip_suffix("},")) {
            let nums: Vec<u32> = row
                .split(',')
                .map(|n| n.trim().parse::<u32>())
                .collect::<Result<_, _>>()
                .map_err(|e| format!("{line}: {e}"))?;
            let [offset, start, len] = nums[..] else {
                return Err(format!("unexpected isSubclass row: {line}"));
            };
            table.push((offset, to_u16(start)?, to_u16(len)?));
        }
    }
    Ok(Subclass { table, data })
}

fn to_u16(n: u32) -> Result<u16, String> {
    u16::try_from(n).map_err(|_| format!("{n} doesn't fit in u16"))
}

/// `Feature_HasNEONBit` -> `neon`, from `getSubtargetFeatureName`.
fn parse_feature_names(matcher: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for line in block(matcher, "static const char *getSubtargetFeatureName(", "}")? {
        let line = line.trim();
        if let Some((bit, name)) = line
            .strip_prefix("case ")
            .and_then(|rest| rest.split_once(": return \""))
        {
            out.insert(bit.to_string(), name.trim_end_matches("\";").to_string());
        }
    }
    if out.is_empty() {
        return Err("no feature names".into());
    }
    Ok(out)
}

/// `AMFBS_*` in order, each with its feature bits.
fn parse_feature_sets(matcher: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let names: Vec<String> = matcher
        .lines()
        .skip_while(|l| !l.starts_with("// Feature bitsets."))
        .skip(2)
        .take_while(|l| l.trim() != "};")
        .map(|l| l.trim().trim_end_matches(',').to_string())
        .collect();
    let mut bits = Vec::new();
    for line in block(
        matcher,
        "static constexpr FeatureBitset FeatureBitsets[] = {",
        "};",
    )? {
        let set = line.trim().split("//").next().unwrap_or_default().trim();
        let set = set
            .trim_start_matches('{')
            .trim_end_matches(',')
            .trim_end_matches('}');
        bits.push(
            set.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>(),
        );
    }
    if names.len() != bits.len() || names.first().map(String::as_str) != Some("AMFBS_None") {
        return Err(format!(
            "{} feature set names but {} bitsets",
            names.len(),
            bits.len()
        ));
    }
    Ok(names.into_iter().zip(bits).collect())
}

struct Form {
    mnemonic: String,
    opcode: String,
    /// 0 is the generic syntax, 1 is Apple's legacy NEON syntax.
    variant: u8,
    features: String,
    operands: Vec<usize>,
}

/// `MatchTable0` (generic syntax) and `MatchTable1` (Apple's NEON syntax).
fn parse_forms(matcher: &str, classes: &HashMap<&str, usize>) -> Result<Vec<Form>, String> {
    let mut forms = Vec::new();
    for variant in 0..2u8 {
        let start = format!("static const MatchEntry MatchTable{variant}[] = {{");
        for line in block(matcher, &start, "};")? {
            forms.push(parse_entry(line.trim(), variant, classes)?);
        }
    }
    Ok(forms)
}

/// `{ 0 /* abs */, AArch64::ABSWr, Convert__Reg1_0__Reg1_1, AMFBS_HasCSSC, { MCK_GPR32, MCK_GPR32 }, },`
fn parse_entry(line: &str, variant: u8, classes: &HashMap<&str, usize>) -> Result<Form, String> {
    let bad = || format!("unexpected match entry: {line}");
    let mnemonic = line
        .split_once("/* ")
        .and_then(|(_, r)| r.split_once(" */"))
        .ok_or_else(bad)?
        .0
        .to_string();
    let (head, operands) = line.split_once(", {").ok_or_else(bad)?;
    let fields: Vec<&str> = head.split(", ").collect();
    let [_, opcode, _, features] = fields[..] else {
        return Err(bad());
    };
    let operands = operands
        .split('}')
        .next()
        .ok_or_else(bad)?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|ident| {
            classes
                .get(ident)
                .copied()
                .ok_or_else(|| format!("unknown class {ident} in: {line}"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Form {
        mnemonic,
        opcode: opcode.trim_start_matches("AArch64::").to_string(),
        variant,
        features: features.to_string(),
        operands,
    })
}

/// The match class of each register, by LLVM register number, from the table
/// in `validateOperandClass`.
fn parse_register_classes(matcher: &str) -> Result<Vec<String>, String> {
    let lines = block(matcher, "static unsigned validateOperandClass(", "}")?;
    let out: Vec<String> = lines
        .iter()
        .skip_while(|l| !l.contains("static constexpr uint16_t Table[AArch64::NUM_TARGET_REGS]"))
        .skip(1)
        .take_while(|l| l.trim() != "};")
        .map(|l| l.trim().trim_end_matches(',').to_string())
        .collect();
    if out.len() < 100 {
        return Err(format!("only {} registers in the class table", out.len()));
    }
    Ok(out)
}

/// The kinds of register name clang's parser tells apart, with the C++
/// function that matches each. Names are tried in this order.
const REGISTER_KINDS: &[(&str, &str)] = &[
    ("SveData", "static unsigned matchSVEDataVectorRegName("),
    (
        "SvePredicate",
        "static unsigned matchSVEPredicateVectorRegName(",
    ),
    (
        "SvePredicateAsCounter",
        "static unsigned matchSVEPredicateAsCounterRegName(",
    ),
    ("NeonVector", "static unsigned MatchNeonVectorRegName("),
    ("Matrix", "static unsigned matchMatrixRegName("),
    (
        "MatrixTileList",
        "static unsigned matchMatrixTileListRegName(",
    ),
];

/// Every register name clang accepts, with its kind and LLVM register:
/// scalar names from the generated `MatchRegisterName`, the rest from the
/// `.Case("v0", AArch64::Q0)` tables in AArch64AsmParser.cpp, plus the aliases
/// `matchRegisterNameAlias` adds.
fn parse_register_names(
    matcher: &str,
    cpp: &str,
) -> Result<BTreeMap<(String, String), String>, String> {
    let mut out = BTreeMap::new();
    let mut add = |name: &str, kind: &str, reg: &str| {
        out.entry((name.to_string(), kind.to_string()))
            .or_insert_with(|| reg.to_string());
    };
    let scalar = block(
        matcher,
        "static MCRegister MatchRegisterName(StringRef Name) {",
        "}",
    )?;
    for line in scalar {
        let Some(rest) = line.trim().strip_prefix("return AArch64::") else {
            continue;
        };
        let Some((reg, comment)) = rest.split_once(';') else {
            continue;
        };
        if let Some(name) = comment
            .trim()
            .strip_prefix("// \"")
            .and_then(|c| c.strip_suffix('"'))
        {
            add(name, "Scalar", reg);
        }
    }
    for (name, reg) in [("fp", "FP"), ("lr", "LR"), ("x31", "XZR"), ("w31", "WZR")] {
        add(name, "Scalar", reg);
    }
    add("zt0", "LookupTable", "ZT0");
    for (kind, function) in REGISTER_KINDS {
        let lines = block(cpp, function, "}")?;
        let mut found = 0;
        for line in lines {
            let Some(rest) = line.trim().strip_prefix(".Case(\"") else {
                continue;
            };
            let Some((name, reg)) = rest.split_once("\", AArch64::") else {
                return Err(format!("unexpected case in {function}: {line}"));
            };
            add(name, kind, reg.trim_end_matches(')'));
            found += 1;
        }
        if found == 0 {
            return Err(format!("no names in {function}"));
        }
    }
    // matchRegisterNameAlias tries every table above before MatchRegisterName
    // and stops at the first that knows the name, so `z0` is never a scalar.
    // Tile lists are parsed by a different function and don't shadow anything.
    let shadowed: Vec<String> = out
        .keys()
        .filter(|(_, kind)| kind != "Scalar" && kind != "MatrixTileList")
        .map(|(name, _)| name.clone())
        .collect();
    for name in shadowed {
        out.remove(&(name, "Scalar".to_string()));
    }
    if out.len() < 300 {
        return Err(format!("only {} register names", out.len()));
    }
    Ok(out)
}

/// Directive names clang accepts on macOS. Candidates are every `".name"`
/// string in LLVM's generic, Darwin, and AArch64 parsers; each is assembled
/// alone with clang, and kept unless clang calls it an unknown directive.
fn probe_directives(sources: &[String]) -> Result<Vec<String>, String> {
    // `.loh` is named by a function, MCLOHDirectiveName(), not a literal.
    // Harvesting LLVM's AArch64 MC tests found no other gaps (2026-10-26).
    let mut candidates = std::collections::BTreeSet::from([".loh".to_string()]);
    // Every `".name"` literal. Matching each literal on its own, rather than
    // splitting on quotes, survives char literals like '"' in the source.
    let is_name = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.';
    for source in sources {
        for (start, _) in source.match_indices("\".") {
            let rest = &source[start + 1..];
            let len = 1 + rest[1..]
                .find(|c: char| !is_name(c))
                .unwrap_or(rest.len() - 1);
            let name = &rest[..len];
            // `.2byte` is a directive; `.0` isn't.
            let has_letter = name.chars().any(|c| c.is_ascii_lowercase());
            if len > 1 && has_letter && rest[len..].starts_with('"') {
                candidates.insert(name.to_string());
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("arm-tools-directives-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let source = dir.join("probe.s");
    let mut known = Vec::new();
    for candidate in candidates {
        std::fs::write(&source, format!("{candidate}\n"))
            .map_err(|e| format!("can't write {}: {e}", source.display()))?;
        let out = Command::new("xcrun")
            .args(["clang", "-c", "-arch", "arm64", "-o"])
            .arg(dir.join("probe.o"))
            .arg(&source)
            .output()
            .map_err(|e| format!("can't run xcrun clang: {e}. gen-isa needs macOS."))?;
        if !String::from_utf8_lossy(&out.stderr).contains("unknown directive") {
            known.push(candidate);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    if known.len() < 150 {
        return Err(format!(
            "only {} directives; is clang working?",
            known.len()
        ));
    }
    Ok(known)
}

struct Render<'a> {
    version: &'a str,
    classes: &'a [Class],
    predicates: &'a HashMap<String, (String, String)>,
    messages: &'a BTreeMap<String, String>,
    subclass: &'a Subclass,
    feature_names: &'a BTreeMap<String, String>,
    feature_sets: &'a [(String, Vec<String>)],
    forms: &'a [Form],
    reg_classes: &'a [String],
    reg_names: &'a BTreeMap<(String, String), String>,
    reg_numbers: &'a HashMap<String, usize>,
    class_index: &'a HashMap<&'a str, usize>,
    directives: &'a [String],
}

fn render(r: &Render) -> Result<String, String> {
    let mut o = String::new();
    let w = &mut o;
    let _ = writeln!(
        w,
        "// @generated by `cargo xtask gen-isa` from LLVM {} (llvm-tblgen --gen-asm-matcher\n\
         // and AArch64AsmParser.cpp). Do not edit. Derived from LLVM, which is licensed\n\
         // Apache-2.0 WITH LLVM-exception; see crates/isa/LICENSE-LLVM.txt.\n",
        r.version
    );
    let _ = writeln!(w, "use crate::tables::*;\n");
    let _ = writeln!(
        w,
        "pub(crate) const LLVM_VERSION: &str = {:?};\n",
        r.version
    );

    // Features: every bit that appears in a set, in a stable order.
    let mut features: Vec<&str> = r
        .feature_sets
        .iter()
        .flat_map(|(_, bits)| bits.iter().map(String::as_str))
        .collect();
    features.sort_unstable();
    features.dedup();
    let feature_index: HashMap<&str, usize> =
        features.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let _ = writeln!(w, "pub(crate) static FEATURES: &[FeatureInfo] = &[");
    for bit in &features {
        let name = r
            .feature_names
            .get(*bit)
            .ok_or_else(|| format!("no name for feature {bit}"))?;
        let ident = bit.trim_start_matches("Feature_").trim_end_matches("Bit");
        let _ = writeln!(w, "    FeatureInfo {{ ident: {ident:?}, name: {name:?} }},");
    }
    let _ = writeln!(w, "];\n");

    let set_index: HashMap<&str, usize> = r
        .feature_sets
        .iter()
        .enumerate()
        .map(|(i, (n, _))| (n.as_str(), i))
        .collect();
    let _ = writeln!(w, "pub(crate) static FEATURE_SETS: &[&[u16]] = &[");
    for (_, bits) in r.feature_sets {
        let ids: Vec<String> = bits
            .iter()
            .map(|b| feature_index[b.as_str()].to_string())
            .collect();
        let _ = writeln!(w, "    &[{}],", ids.join(", "));
    }
    let _ = writeln!(w, "];\n");

    let _ = writeln!(w, "pub(crate) static CLASSES: &[ClassInfo] = &[");
    for class in r.classes {
        let kind = match class.kind {
            ClassKind::Invalid => "ClassKind::Invalid",
            ClassKind::Optional => "ClassKind::Optional",
            ClassKind::Token => "ClassKind::Token",
            ClassKind::Register => "ClassKind::Register",
            ClassKind::Custom => "ClassKind::Custom",
        };
        let (predicate, diag) = r
            .predicates
            .get(&strip_mck(&class.ident))
            .cloned()
            .unwrap_or_default();
        let message = r.messages.get(&diag).cloned().unwrap_or_default();
        let _ = writeln!(
            w,
            "    ClassInfo {{ name: {:?}, kind: {kind}, predicate: {predicate:?}, diagnostic: {diag:?}, message: {message:?} }},",
            class.name
        );
    }
    let _ = writeln!(w, "];\n");

    let _ = writeln!(
        w,
        "pub(crate) static SUBCLASS_TABLE: &[(u32, u16, u16)] = &["
    );
    for (offset, start, len) in &r.subclass.table {
        let _ = writeln!(w, "    ({offset}, {start}, {len}),");
    }
    let _ = writeln!(w, "];\n");
    let _ = writeln!(w, "pub(crate) static SUBCLASS_DATA: &[u8] = &[");
    for chunk in r.subclass.data.chunks(16) {
        let bytes: Vec<String> = chunk.iter().map(|b| format!("0x{b:02X}")).collect();
        let _ = writeln!(w, "    {},", bytes.join(", "));
    }
    let _ = writeln!(w, "];\n");

    // Forms sorted by mnemonic, then variant, keeping LLVM's order within each:
    // the matcher tries them in that order.
    let mut order: Vec<usize> = (0..r.forms.len()).collect();
    order.sort_by(|&a, &b| {
        let (fa, fb) = (&r.forms[a], &r.forms[b]);
        (fa.mnemonic.as_str(), fa.variant, a).cmp(&(fb.mnemonic.as_str(), fb.variant, b))
    });
    let mut operands: Vec<usize> = Vec::new();
    let mut mnemonics: Vec<(&str, usize, usize)> = Vec::new();
    let _ = writeln!(w, "pub(crate) static FORMS: &[FormInfo] = &[");
    for (i, &f) in order.iter().enumerate() {
        let form = &r.forms[f];
        match mnemonics.last_mut() {
            Some((m, _, len)) if *m == form.mnemonic => *len += 1,
            _ => mnemonics.push((&form.mnemonic, i, 1)),
        }
        let set = set_index
            .get(form.features.as_str())
            .ok_or_else(|| format!("unknown feature set {}", form.features))?;
        let _ = writeln!(
            w,
            "    FormInfo {{ opcode: {:?}, variant: {}, features: {set}, operands: ({}, {}) }},",
            form.opcode,
            form.variant,
            operands.len(),
            form.operands.len()
        );
        operands.extend(&form.operands);
    }
    let _ = writeln!(w, "];\n");

    let _ = writeln!(w, "pub(crate) static OPERANDS: &[u16] = &[");
    for chunk in operands.chunks(24) {
        let ids: Vec<String> = chunk.iter().map(usize::to_string).collect();
        let _ = writeln!(w, "    {},", ids.join(", "));
    }
    let _ = writeln!(w, "];\n");

    let _ = writeln!(
        w,
        "/// Sorted by name: (mnemonic, first form, number of forms).\npub(crate) static MNEMONICS: &[(&str, u32, u16)] = &["
    );
    for (m, start, len) in &mnemonics {
        let _ = writeln!(w, "    ({m:?}, {start}, {len}),");
    }
    let _ = writeln!(w, "];\n");

    // Registers: each name the assembler accepts, with its LLVM register and the
    // match class of that register.
    let _ = writeln!(
        w,
        "/// Sorted by name, then kind.\npub(crate) static REGISTERS: &[RegisterInfo] = &["
    );
    for ((name, kind), reg) in r.reg_names {
        let number = r
            .reg_numbers
            .get(reg.as_str())
            .copied()
            .ok_or_else(|| format!("no number for register {reg}"))?;
        let class = &r.reg_classes[number];
        let class = if class == "InvalidMatchClass" {
            0
        } else {
            *r.class_index
                .get(class.as_str())
                .ok_or_else(|| format!("unknown class {class} for {reg}"))?
        };
        let _ = writeln!(
            w,
            "    RegisterInfo {{ name: {name:?}, kind: RegisterKind::{kind}, llvm: {reg:?}, class: {class} }},"
        );
    }
    let _ = writeln!(w, "];\n");

    let _ = writeln!(
        w,
        "/// Sorted. Directive names clang accepts, probed with clang."
    );
    let _ = writeln!(w, "pub(crate) static DIRECTIVES: &[&str] = &[");
    for d in r.directives {
        let _ = writeln!(w, "    {d:?},");
    }
    let _ = writeln!(w, "];");
    Ok(o)
}
