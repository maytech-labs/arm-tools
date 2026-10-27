//! `armtool`: check and lint AArch64 assembly from the command line and CI.
//! Exit codes: 0 clean, 1 errors or warnings found, 2 usage or I/O problem.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use arm_lint::Config;
use arm_sema::{Diagnostic, Encoding, Severity};

const HELP: &str = "\
armtool: check and lint AArch64 assembly for macOS

Usage:
  armtool check [--json] [--fix] <path>...
                                      Syntax, symbol, and operand errors, as clang reports them
  armtool lint [--json] [--fix] [--config <file>] <path>...
                                      check, plus the lint rules; --fix applies
                                      the fixes they suggest, in place
  armtool fmt [--check] <path>...     Format files in place; --check lists files that
                                      need formatting and changes nothing
  armtool rules                       List the lint rules
  armtool explain <rule>              Explain a rule and how to fix what it finds
  armtool --version

Paths can be files or directories; directories are searched for .s and .S
files. lint reads the nearest armtool.toml above each file unless --config
is given.

Exit codes: 0 clean, 1 errors or warnings found, 2 usage or I/O problem.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprint!("{HELP}");
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    match command.as_str() {
        "check" => run(rest, false),
        "fmt" => fmt(rest),
        "lint" => run(rest, true),
        "rules" => {
            for rule in arm_lint::rules() {
                println!(
                    "{:<24} {:<8} {}",
                    rule.id,
                    level_name(rule.default_level),
                    rule.summary
                );
            }
            ExitCode::SUCCESS
        }
        "explain" => match rest.first().and_then(|id| arm_lint::rule(id)) {
            Some(rule) => {
                println!("{}\n\n{}\n\n{}", rule.id, rule.summary, rule.explanation);
                println!(
                    "\nDefault level: {}. Change it in armtool.toml:\n\n[rules]\n\"{}\" = \"off\"\n\nOr silence one line with a comment: ; armtool: allow({})",
                    level_name(rule.default_level),
                    rule.id,
                    rule.id
                );
                ExitCode::SUCCESS
            }
            None => {
                let given = rest.first().map_or("", String::as_str);
                eprintln!("armtool: no rule '{given}'. Run `armtool rules` for the list.");
                ExitCode::from(2)
            }
        },
        "--version" | "-V" => {
            println!(
                "armtool {} (instruction tables from LLVM {})",
                env!("CARGO_PKG_VERSION"),
                arm_isa::llvm_version()
            );
            ExitCode::SUCCESS
        }
        "--help" | "-h" | "help" => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        other => {
            eprint!("armtool: unknown command '{other}'\n\n{HELP}");
            ExitCode::from(2)
        }
    }
}

fn level_name(level: arm_lint::Level) -> &'static str {
    match level {
        arm_lint::Level::Error => "error",
        arm_lint::Level::Warning => "warning",
        arm_lint::Level::Info => "info",
        arm_lint::Level::Hint => "hint",
        arm_lint::Level::Off => "off",
    }
}

fn run(args: &[String], lint: bool) -> ExitCode {
    let mut json = false;
    let mut fix = false;
    let mut config_path = None;
    let mut paths = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--fix" => fix = true,
            "--config" if lint => match args.next() {
                Some(path) => config_path = Some(PathBuf::from(path)),
                None => return usage("--config needs a file"),
            },
            flag if flag.starts_with('-') => return usage(&format!("unknown flag '{flag}'")),
            path => paths.push(PathBuf::from(path)),
        }
    }
    if paths.is_empty() {
        return usage("give at least one file or directory");
    }
    let fixed_config = match &config_path {
        Some(path) => match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| Config::parse(&text))
        {
            Ok(config) => Some(config),
            Err(error) => {
                eprintln!("armtool: {}: {error}", path.display());
                return ExitCode::from(2);
            }
        },
        None => None,
    };

    let mut files = Vec::new();
    for path in &paths {
        if let Err(error) = collect(path, &mut files) {
            eprintln!("armtool: {error}");
            return ExitCode::from(2);
        }
    }
    files.sort();
    let mut failed = false;
    let mut problems = false;
    let mut records = Vec::new();
    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("armtool: can't read {}: {error}", file.display());
                problems = true;
                continue;
            }
        };
        // The config sets include paths for check and lint, and rule levels
        // for lint.
        let config = match &fixed_config {
            Some(config) => config.clone(),
            None => {
                let dir = file.parent().unwrap_or(Path::new("."));
                match Config::discover(dir) {
                    Ok(config) => config,
                    Err(error) => {
                        eprintln!("armtool: {error}");
                        return ExitCode::from(2);
                    }
                }
            }
        };
        let path = arm_sema::normalize(&std::env::current_dir().unwrap_or_default().join(file));
        let diagnose = |text: &str| {
            let checked = arm_lint::check(text, Some(&path), &config, Default::default(), lint);
            (checked.analysis, checked.diagnostics)
        };
        let (mut analysis, mut diagnostics) = diagnose(&text);
        let mut text = text;
        if fix && let Some(fixed) = apply_fixes(&text, &diagnostics) {
            if let Err(error) = std::fs::write(file, &fixed.0) {
                eprintln!("armtool: can't write {}: {error}", file.display());
                return ExitCode::from(2);
            }
            eprintln!("armtool: fixed {} in {}", fixed.1, file.display());
            text = fixed.0;
            (analysis, diagnostics) = diagnose(&text);
        }
        for d in &diagnostics {
            failed |= matches!(d.severity, Severity::Error | Severity::Warning);
            let start = analysis
                .line_index()
                .line_col(&text, d.range.start, Encoding::Utf8);
            let end = analysis
                .line_index()
                .line_col(&text, d.range.end, Encoding::Utf8);
            if json {
                records.push(format!(
                    "{{\"path\":{},\"line\":{},\"column\":{},\"end_line\":{},\"end_column\":{},\"severity\":\"{}\",\"code\":{},\"message\":{}}}",
                    json_string(&file.display().to_string()),
                    start.line + 1,
                    start.col + 1,
                    end.line + 1,
                    end.col + 1,
                    severity_name(d.severity),
                    json_string(d.code),
                    json_string(&d.message)
                ));
            } else {
                println!(
                    "{}:{}:{}: {}[{}]: {}",
                    file.display(),
                    start.line + 1,
                    start.col + 1,
                    severity_name(d.severity),
                    d.code,
                    d.message
                );
            }
        }
    }
    if json {
        println!("[{}]", records.join(","));
    } else if !failed {
        eprintln!(
            "armtool: {} file{} checked, no errors or warnings",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        );
    }
    if problems {
        ExitCode::from(2)
    } else if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Formats files with the nearest armtool.toml's [format] settings.
fn fmt(args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    let mut files = Vec::new();
    for arg in args.iter().filter(|a| *a != "--check") {
        if arg.starts_with('-') {
            return usage(&format!("unknown flag '{arg}'"));
        }
        if let Err(error) = collect(Path::new(arg), &mut files) {
            eprintln!("armtool: {error}");
            return ExitCode::from(2);
        }
    }
    if files.is_empty() {
        return usage("give at least one file or directory");
    }
    files.sort();
    let mut unformatted = 0;
    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("armtool: can't read {}: {error}", file.display());
                return ExitCode::from(2);
            }
        };
        let dir = file.parent().unwrap_or(Path::new("."));
        let config = match Config::discover(dir) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("armtool: {error}");
                return ExitCode::from(2);
            }
        };
        let formatted = arm_syntax::format(&text, &config.format);
        if formatted == text {
            continue;
        }
        unformatted += 1;
        if check {
            println!("{}", file.display());
        } else if let Err(error) = std::fs::write(file, formatted) {
            eprintln!("armtool: can't write {}: {error}", file.display());
            return ExitCode::from(2);
        }
    }
    match (check, unformatted) {
        (true, 0) | (false, _) => {
            if !check {
                eprintln!("armtool: formatted {unformatted} of {} files", files.len());
            }
            ExitCode::SUCCESS
        }
        (true, n) => {
            eprintln!(
                "armtool: {n} file{} need formatting: run armtool fmt",
                if n == 1 { "" } else { "s" }
            );
            ExitCode::from(1)
        }
    }
}

/// Applies every fix that doesn't overlap an earlier one. Returns the new
/// text and how many fixes went in, or None if there were none.
fn apply_fixes(text: &str, diagnostics: &[Diagnostic]) -> Option<(String, usize)> {
    let mut edits: Vec<(std::ops::Range<usize>, &str)> = Vec::new();
    let mut applied = 0;
    for fix in diagnostics.iter().filter_map(|d| d.fix.as_ref()) {
        let overlaps = fix.edits.iter().any(|(r, _)| {
            edits
                .iter()
                .any(|(e, _)| r.start < e.end.max(e.start + 1) && e.start < r.end.max(r.start + 1))
        });
        if overlaps {
            continue;
        }
        edits.extend(fix.edits.iter().map(|(r, t)| (r.clone(), t.as_str())));
        applied += 1;
    }
    if applied == 0 {
        return None;
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut out = text.to_string();
    for (range, new) in edits {
        out.replace_range(range, new);
    }
    Some((out, applied))
}

fn usage(message: &str) -> ExitCode {
    eprint!("armtool: {message}\n\n{HELP}");
    ExitCode::from(2)
}

/// Files to check: a file as given, or every .s and .S file under a directory.
fn collect(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let meta =
        std::fs::metadata(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    if meta.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let entries =
        std::fs::read_dir(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    for entry in entries.flatten() {
        let child = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if child.is_dir() {
            collect(&child, out)?;
        } else if child.extension().is_some_and(|e| e == "s" || e == "S") {
            out.push(child);
        }
    }
    Ok(())
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
