//! A file's call graph, for call hierarchy in editors: its functions, as
//! the control-flow graph finds them, and the calls each one makes.

use std::ops::Range;

use arm_sema::Analysis;

use crate::flow::{Cfg, is_terminator};

/// A function: a label that's exported or called with `bl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSpan {
    pub name: String,
    pub name_range: Range<usize>,
    /// From the label to the end of its last instruction.
    pub range: Range<usize>,
    /// Whether other files can call it: `.globl` or `.private_extern`.
    pub exported: bool,
}

/// A `bl` to a name, or a tail call: `b` to a function or to a name this
/// file doesn't define.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The function the call is in, as an index into
    /// [`CallGraph::functions`]. None for code before the first function.
    pub caller: Option<usize>,
    /// The name called, as written.
    pub callee: String,
    /// Where the name is written.
    pub range: Range<usize>,
    pub tail: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallGraph {
    pub functions: Vec<FunctionSpan>,
    pub calls: Vec<Call>,
}

impl CallGraph {
    /// The function with this name, if the file defines one.
    pub fn function(&self, name: &str) -> Option<usize> {
        self.functions.iter().position(|f| f.name == name)
    }
}

/// The functions and calls in a file.
pub fn call_graph(analysis: &Analysis) -> CallGraph {
    let cfg = Cfg::new(analysis);
    let symbols = analysis.symbols();
    let functions: Vec<FunctionSpan> = cfg
        .functions
        .iter()
        .map(|f| {
            let end = cfg.insns[f.span.clone()]
                .last()
                .map_or(f.name_range.end, |insn| insn.range.end);
            let exported = symbols
                .iter()
                .any(|s| s.name == f.name && s.range == f.name_range && s.exported);
            FunctionSpan {
                name: f.name.clone(),
                name_range: f.name_range.clone(),
                range: f.name_range.start..end,
                exported,
            }
        })
        .collect();
    let mut calls = Vec::new();
    for (i, insn) in cfg.insns.iter().enumerate() {
        let head = insn.mnemonic.head.as_str();
        let branch = head == "b" && is_terminator(insn);
        if head != "bl" && !branch {
            continue;
        }
        let Some(range) = insn.operand_ranges.last() else {
            continue;
        };
        let Some(reference) = analysis.reference_at(range.start) else {
            continue;
        };
        let callee = reference.name.clone();
        // A branch is a tail call if it leaves for another function.
        if branch {
            let leaves = match reference.symbol {
                None => true,
                Some(s) => functions.iter().any(|f| f.name_range == symbols[s].range),
            };
            if !leaves {
                continue;
            }
        }
        let caller = cfg.functions.iter().position(|f| f.span.contains(&i));
        calls.push(Call {
            caller,
            callee,
            range: reference.range.clone(),
            tail: branch,
        });
    }
    CallGraph { functions, calls }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "\
start:
    bl _setup
.globl _main
_main:
    bl helper
    bl _printf
    b.eq 1f
1:  bl helper
    b _exit_now
helper:
    b 2f
2:  ret
.globl _exit_now
_exit_now:
    b _abort
";

    #[test]
    fn functions_and_their_calls() {
        let analysis = Analysis::new(TEXT);
        let graph = call_graph(&analysis);
        let names: Vec<(&str, bool)> = graph
            .functions
            .iter()
            .map(|f| (f.name.as_str(), f.exported))
            .collect();
        assert_eq!(
            names,
            [("_main", true), ("helper", false), ("_exit_now", true)]
        );
        let main = &graph.functions[0];
        assert!(TEXT[main.range.clone()].starts_with("_main:"));
        assert!(TEXT[main.range.clone()].ends_with("b _exit_now"));
        let calls: Vec<(Option<&str>, &str, bool)> = graph
            .calls
            .iter()
            .map(|c| {
                (
                    c.caller.map(|i| graph.functions[i].name.as_str()),
                    c.callee.as_str(),
                    c.tail,
                )
            })
            .collect();
        assert_eq!(
            calls,
            [
                (None, "_setup", false),
                (Some("_main"), "helper", false),
                (Some("_main"), "_printf", false),
                (Some("_main"), "helper", false),
                (Some("_main"), "_exit_now", true),
                (Some("_exit_now"), "_abort", true),
            ],
            "branches inside a function aren't calls"
        );
        assert_eq!(&TEXT[graph.calls[1].range.clone()], "helper");
        assert_eq!(graph.function("helper"), Some(1));
        assert_eq!(graph.function("_printf"), None);
    }
}
