//! The formatter (docs/decisions/0020-formatter.md). It changes only
//! whitespace: tokens are never added, removed, or joined, so formatting
//! can't change what a file assembles to. Lines it can't safely judge (inside
//! block comments, with `%%`, or with syntax errors) are left as they are.

use crate::{Platform, SyntaxKind, lex_for};

/// Layout settings, set in `armtool.toml` under `[format]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    /// Column where mnemonics and directives start.
    pub indent: usize,
    /// Width the mnemonic is padded to before its operands.
    pub mnemonic_width: usize,
    /// Column where trailing comments start, when the code is shorter.
    pub comment_column: usize,
    /// Most consecutive blank lines kept.
    pub max_blank_lines: usize,
    /// Whose conventions the file follows: `;` is a comment on Darwin and a
    /// separator on Linux.
    pub platform: Platform,
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions {
            indent: 4,
            mnemonic_width: 8,
            comment_column: 40,
            max_blank_lines: 1,
            platform: Platform::Darwin,
        }
    }
}

/// One line's tokens: (kind, text), whitespace dropped.
type Tokens<'a> = Vec<(SyntaxKind, &'a str)>;

/// Formats a whole file. The result always ends with one newline, unless the
/// file is empty.
pub fn format(text: &str, options: &FormatOptions) -> String {
    let lexed = lex_for(text, options.platform);
    let errors: Vec<(usize, usize)> = crate::parse_for(text, options.platform)
        .errors()
        .iter()
        .map(|e| (e.range.start, e.range.end))
        .collect();
    let has_error = |start: usize, end: usize| errors.iter().any(|&(s, e)| s <= end && e >= start);

    // Split into physical lines of tokens, noting lines that a block comment
    // spans: those are copied, not formatted.
    let mut lines: Vec<(usize, usize, bool)> = Vec::new(); // (start, end, verbatim)
    let mut start = 0;
    let mut verbatim = false;
    for (kind, range) in lexed.ranges() {
        let piece = &text[range.clone()];
        if kind == SyntaxKind::Newline {
            lines.push((start, range.start, verbatim));
            start = range.end;
            verbatim = false;
        } else if piece.contains('\n') {
            // A block comment (or bad string) across lines: this line, the
            // lines inside, and the line it ends on stay as written.
            verbatim = true;
            let mut offset = range.start;
            for part in piece.split_inclusive('\n') {
                if part.ends_with('\n') {
                    lines.push((start, offset + part.len() - 1, true));
                    start = offset + part.len();
                }
                offset += part.len();
            }
        } else if matches!(kind, SyntaxKind::Separator) {
            verbatim = true;
        }
    }
    if start < text.len() {
        lines.push((start, text.len(), verbatim));
    }

    let mut out = String::with_capacity(text.len());
    let mut blanks = 0;
    for (start, end, verbatim) in lines {
        let line = text[start..end].trim_end_matches('\r');
        if line.trim().is_empty() {
            blanks += 1;
            if blanks <= options.max_blank_lines && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blanks = 0;
        if verbatim || has_error(start, end) {
            out.push_str(line.trim_end());
        } else {
            out.push_str(&format_line(line, options));
        }
        out.push('\n');
    }
    // Drop blank lines at the end.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Formats one line with no block comment, separator, or syntax error.
fn format_line(line: &str, options: &FormatOptions) -> String {
    let lexed = lex_for(line, options.platform);
    let mut tokens: Tokens = Vec::new();
    // Whether whitespace came before each token: inside operands, tokens that
    // touched stay touching.
    let mut spaced: Vec<bool> = Vec::new();
    let mut gap = false;
    for (kind, range) in lexed.ranges() {
        if kind == SyntaxKind::Whitespace {
            gap = true;
            continue;
        }
        tokens.push((kind, &line[range]));
        spaced.push(gap);
        gap = false;
    }
    let comment = match tokens.last() {
        Some(&(SyntaxKind::Comment, text)) => {
            tokens.pop();
            spaced.pop();
            Some(text)
        }
        _ => None,
    };
    let leading_column = line.len() - line.trim_start().len();

    // Labels first: `name:` and `1:`.
    let mut i = 0;
    let mut labels = Vec::new();
    while i + 1 < tokens.len()
        && matches!(
            tokens[i].0,
            SyntaxKind::Ident | SyntaxKind::Number | SyntaxKind::StringLit
        )
        && tokens[i + 1].0 == SyntaxKind::Colon
    {
        labels.push(format!("{}:", tokens[i].1));
        i += 2;
    }
    let statement = statement(&tokens[i..], &spaced[i..], options);

    let mut code = labels.join(" ");
    if let Some(statement) = statement {
        let pad = if code.is_empty() {
            options.indent
        } else if code.len() < options.indent {
            options.indent - code.len()
        } else {
            1
        };
        code.push_str(&" ".repeat(pad));
        code.push_str(&statement);
    }
    match comment {
        None => code,
        // A comment alone: keep it at the left edge if it was there,
        // otherwise at the code's indent.
        Some(comment) if code.is_empty() => {
            if leading_column == 0 {
                comment.to_string()
            } else {
                format!("{}{comment}", " ".repeat(options.indent))
            }
        }
        Some(comment) => {
            let pad = options
                .comment_column
                .saturating_sub(code.chars().count())
                .max(1);
            format!("{code}{}{comment}", " ".repeat(pad))
        }
    }
}

/// The statement after any labels: mnemonic or directive, then operands.
/// The mnemonic is the run of tokens before the first whitespace, so a macro
/// paste like `\sz\()_x` stays one word.
fn statement(
    tokens: &[(SyntaxKind, &str)],
    spaced: &[bool],
    options: &FormatOptions,
) -> Option<String> {
    if tokens.is_empty() {
        return None;
    }
    // In `name=value`, `=` is its own token, so spacing it is safe.
    let assignment = tokens
        .get(1)
        .is_some_and(|(kind, _)| *kind == SyntaxKind::Eq);
    let word_end = if assignment {
        1
    } else {
        (1..tokens.len())
            .find(|&k| spaced[k])
            .unwrap_or(tokens.len())
    };
    let word: String = tokens[..word_end].iter().map(|(_, t)| *t).collect();
    let rest = &tokens[word_end..];
    if rest.is_empty() {
        return Some(word);
    }
    // `name = value` and `name .req register` read best with single spaces.
    let (second, _) = rest[0];
    let second_text = rest[0].1;
    if second == SyntaxKind::Eq || second_text.eq_ignore_ascii_case(".req") {
        let value = operands(&rest[1..], &spaced[word_end + 1..]);
        return Some(
            format!("{word} {second_text} {value}")
                .trim_end()
                .to_string(),
        );
    }
    let pad = options
        .mnemonic_width
        .saturating_sub(word.chars().count())
        .max(1);
    Some(format!(
        "{word}{}{}",
        " ".repeat(pad),
        operands(rest, &spaced[word_end..])
    ))
}

/// Operands with one space after each comma, none inside brackets and
/// parentheses, and otherwise the original spacing collapsed to one space.
fn operands(tokens: &[(SyntaxKind, &str)], spaced: &[bool]) -> String {
    use SyntaxKind::*;
    let mut out = String::new();
    for (i, (kind, text)) in tokens.iter().enumerate() {
        if i > 0 {
            let previous = tokens[i - 1].0;
            let space = if previous == Comma {
                true
            } else if matches!(previous, LBracket | LBrace | LParen)
                || matches!(kind, RBracket | RBrace | RParen | Comma)
            {
                false
            } else {
                spaced[i]
            };
            if space {
                out.push(' ');
            }
        }
        out.push_str(text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(text: &str) -> String {
        format(text, &FormatOptions::default())
    }

    #[test]
    fn aligns_mnemonics_operands_and_comments() {
        assert_eq!(
            fmt("_main:\n  stp x29,x30,[ sp, #-16 ]!   ;  save\nmov   x29 ,  sp\n"),
            "_main:\n    stp     x29, x30, [sp, #-16]!       ;  save\n    mov     x29, sp\n"
        );
    }

    #[test]
    fn labels_share_a_line_with_short_statements() {
        assert_eq!(
            fmt("1: b 1b\nlong_label: ret\n"),
            "1:  b       1b\nlong_label: ret\n"
        );
    }

    #[test]
    fn directives_assignments_and_aliases() {
        assert_eq!(
            fmt(".globl   _main\n.p2align 2\nlimit=4\ncounter    .req   x19\n"),
            "    .globl  _main\n    .p2align 2\n    limit = 4\n    counter .req x19\n"
        );
    }

    #[test]
    fn never_joins_or_splits_tokens() {
        // Macro pastes like `v0.\sz` and `\a\()_suffix` keep their adjacency.
        let text = ".macro m sz\n  add v0.\\sz,v1.\\sz , v2.\\sz\n  \\sz\\()_x:\n.endm\n";
        assert_eq!(
            fmt(text),
            "    .macro  m sz\n    add     v0.\\sz, v1.\\sz, v2.\\sz\n    \\sz\\()_x:\n    .endm\n"
        );
    }

    #[test]
    fn comments_alone_keep_the_left_edge_or_indent() {
        assert_eq!(
            fmt("// file header\n    ; note\nret\n"),
            "// file header\n    ; note\n    ret\n"
        );
    }

    #[test]
    fn blank_lines_collapse_and_trailing_space_goes() {
        assert_eq!(fmt("\n\nnop   \n\n\n\nret\n\n\n"), "    nop\n\n    ret\n");
    }

    #[test]
    fn leaves_what_it_cant_judge() {
        // Block comments across lines, `%%`, and syntax errors stay as written.
        assert_eq!(fmt("nop /* a\n   b */  ret\n"), "nop /* a\n   b */  ret\n");
        assert_eq!(fmt("nop  %%  ret\n"), "nop  %%  ret\n");
        assert_eq!(fmt("mov x0,,x1\n"), "mov x0,,x1\n");
    }

    #[test]
    fn crlf_and_missing_final_newline() {
        assert_eq!(fmt("nop\r\nret"), "    nop\n    ret\n");
        assert_eq!(fmt(""), "");
    }

    #[test]
    fn idempotent() {
        let text = "_f:  stp x29,x30,[sp,#-16]! // c\n1: b.ne 1b\n.quad _f-1 , 2\n";
        let once = fmt(text);
        assert_eq!(fmt(&once), once);
    }
}
