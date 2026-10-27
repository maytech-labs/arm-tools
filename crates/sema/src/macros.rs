//! Macro definitions, argument binding, and expansion, as clang does them
//! (docs/decisions/0024-macro-expansion.md).

/// A `.macro` definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroDef {
    pub params: Vec<Param>,
    /// The lines between `.macro` and `.endm`.
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Param {
    pub name: String,
    pub default: Option<String>,
    /// `name:req`: a call must give it.
    pub required: bool,
    /// `name:vararg`: takes the rest of the arguments.
    pub vararg: bool,
}

/// Parameters from the text after the macro's name: `a, b=1, c:req, d:vararg`.
/// Commas or spaces separate them.
pub(crate) fn parse_params(spec: &str) -> Vec<Param> {
    let spec = spec.replace(" =", "=").replace("= ", "=");
    spec.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (head, default) = match p.split_once('=') {
                Some((h, d)) => (h, Some(d.to_string())),
                None => (p, None),
            };
            let (name, qualifier) = head.split_once(':').unwrap_or((head, ""));
            Param {
                name: name.to_string(),
                default,
                required: qualifier.eq_ignore_ascii_case("req"),
                vararg: qualifier.eq_ignore_ascii_case("vararg"),
            }
        })
        .collect()
}

/// A call's arguments: split at commas outside parentheses. Brackets don't
/// group, as in clang: `[x1, #8]` is two arguments.
pub(crate) fn split_args(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(current.trim().to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    out.push(current.trim().to_string());
    out
}

/// Values for each parameter, from positional and `name=value` arguments.
pub(crate) fn bind(
    name: &str,
    def: &MacroDef,
    args: &[String],
) -> Result<Vec<(String, String)>, String> {
    let mut values: Vec<Option<String>> = vec![None; def.params.len()];
    let mut position = 0;
    for (i, arg) in args.iter().enumerate() {
        if let Some((key, value)) = arg.split_once('=')
            && let Some(p) = def.params.iter().position(|p| p.name == key.trim())
        {
            values[p] = Some(value.trim().to_string());
            continue;
        }
        match def.params.get(position) {
            Some(param) if param.vararg => {
                values[position] = Some(args[i..].join(", "));
                break;
            }
            Some(_) => {
                values[position] = Some(arg.clone());
                position += 1;
            }
            None => return Err("too many positional arguments".into()),
        }
    }
    def.params
        .iter()
        .zip(values)
        .map(
            |(param, value)| match value.or_else(|| param.default.clone()) {
                Some(v) if !(v.is_empty() && param.required) => Ok((param.name.clone(), v)),
                _ if param.required => Err(format!(
                    "missing value for required parameter '{}' in macro '{name}'",
                    param.name
                )),
                _ => Ok((param.name.clone(), String::new())),
            },
        )
        .collect()
}

/// The body with `\param` replaced by its value, `\()` removed, and `\@`
/// replaced by `counter`, up to any `.exitm`.
pub(crate) fn substitute(body: &str, values: &[(String, String)], counter: usize) -> String {
    let body = match body
        .lines()
        .position(|l| l.trim().eq_ignore_ascii_case(".exitm"))
    {
        Some(stop) => body.lines().take(stop).collect::<Vec<_>>().join("\n"),
        None => body.to_string(),
    };
    let mut out = String::with_capacity(body.len());
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let rest = &body[i + 1..];
        if rest.starts_with("()") {
            chars.next();
            chars.next();
        } else if rest.starts_with('@') {
            chars.next();
            out.push_str(&counter.to_string());
        } else {
            let name_len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
                .unwrap_or(rest.len());
            match values
                .iter()
                .find(|(n, _)| *n == rest[..name_len])
                .filter(|_| name_len > 0)
            {
                Some((_, value)) => {
                    out.push_str(value);
                    for _ in 0..rest[..name_len].chars().count() {
                        chars.next();
                    }
                }
                None => out.push('\\'),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(spec: &str, body: &str) -> MacroDef {
        MacroDef {
            params: parse_params(spec),
            body: body.into(),
        }
    }

    fn args(text: &str) -> Vec<String> {
        split_args(text)
    }

    #[test]
    fn params() {
        let params = parse_params("a, b=1 c:req, d:vararg");
        let names: Vec<_> = params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d"]);
        assert_eq!(params[1].default.as_deref(), Some("1"));
        assert!(params[2].required && params[3].vararg);
        assert_eq!(parse_params("a = 5")[0].default.as_deref(), Some("5"));
    }

    #[test]
    fn splitting_follows_parentheses_not_brackets() {
        assert_eq!(args("x0, [x1, #8]"), ["x0", "[x1", "#8]"]);
        assert_eq!(args("(1, 2), 3"), ["(1, 2)", "3"]);
        assert!(args("  ").is_empty());
    }

    #[test]
    fn binding() {
        let m = def("a=1, b=2", "");
        assert_eq!(
            bind("m", &m, &args("b=5")).expect("ok"),
            [("a".into(), "1".into()), ("b".into(), "5".into())]
        );
        assert_eq!(
            bind("m", &m, &args("1, 2, 3")).unwrap_err(),
            "too many positional arguments"
        );
        let required = def("a:req", "");
        assert_eq!(
            bind("m", &required, &[]).unwrap_err(),
            "missing value for required parameter 'a' in macro 'm'"
        );
        let vararg = def("a, rest:vararg", "");
        assert_eq!(
            bind("m", &vararg, &args("1, 2, 3")).expect("ok")[1].1,
            "2, 3"
        );
    }

    #[test]
    fn substitution() {
        let values = [
            ("r".to_string(), "x0".to_string()),
            ("sz".to_string(), "4s".to_string()),
        ];
        assert_eq!(substitute("add \\r, \\r, #1", &values, 0), "add x0, x0, #1");
        assert_eq!(
            substitute("add v0.\\sz, v1.\\sz", &values, 0),
            "add v0.4s, v1.4s"
        );
        assert_eq!(substitute("\\r\\()_end: l\\@:", &values, 7), "x0_end: l7:");
        assert_eq!(substitute("mov \\other", &values, 0), "mov \\other");
        assert_eq!(substitute("nop\n.exitm\nfrob\n", &values, 0), "nop");
    }
}
