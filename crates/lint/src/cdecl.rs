//! Signatures from C: prototypes in the project's headers and sources, and
//! common C library functions, turned into register signatures by the
//! AAPCS64 rules (docs/decisions/0040-c-signatures.md).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arm_isa::Platform;

use crate::types::{Layout, Signature, Ty};

/// Typedef names for structs, to the struct's name: `node_t` to `node`.
pub type Aliases = HashMap<String, String>;

/// What the project's C defines: functions and structs.
#[derive(Debug, Clone, Default)]
pub struct CDefinitions {
    pub signatures: HashMap<String, Signature>,
    pub structs: HashMap<String, Layout>,
}

fn c_type_plain(text: &str) -> Option<Option<Ty>> {
    c_type(text, &Aliases::new())
}

/// The C type of a parameter or result, as far as registers care.
fn c_type(text: &str, aliases: &Aliases) -> Option<Option<Ty>> {
    // Some(None) is void.
    let pointers = text.matches('*').count();
    let words: Vec<&str> = text
        .split(|c: char| c == '*' || c.is_whitespace())
        .filter(|w| {
            !w.is_empty()
                && !matches!(
                    *w,
                    "const"
                        | "volatile"
                        | "restrict"
                        | "__restrict"
                        | "extern"
                        | "static"
                        | "inline"
                        | "register"
                        | "_Nonnull"
                        | "_Nullable"
                )
        })
        .collect();
    let base = words.join(" ");
    let int = |signed, bits| Some(Some(Ty::Int { signed, bits }));
    let scalar = match base.as_str() {
        "void" => Some(None),
        "char" | "signed char" | "int8_t" => int(true, 8),
        "unsigned char" | "uint8_t" | "bool" | "_Bool" => int(false, 8),
        "short" | "short int" | "signed short" | "int16_t" => int(true, 16),
        "unsigned short" | "unsigned short int" | "uint16_t" => int(false, 16),
        "int" | "signed" | "signed int" | "int32_t" | "pid_t" => int(true, 32),
        "unsigned" | "unsigned int" | "uint32_t" | "mode_t" | "uid_t" | "gid_t" => int(false, 32),
        "long" | "long int" | "long long" | "long long int" | "signed long" | "int64_t"
        | "ssize_t" | "ptrdiff_t" | "intptr_t" | "off_t" | "time_t" | "intmax_t" => int(true, 64),
        "unsigned long" | "unsigned long int" | "unsigned long long" | "uint64_t" | "size_t"
        | "uintptr_t" | "uintmax_t" => int(false, 64),
        "float" => Some(Some(Ty::Float { bits: 32 })),
        "double" => Some(Some(Ty::Float { bits: 64 })),
        _ => match base.strip_prefix("struct ") {
            Some(name) => Some(Some(Ty::Struct(name.trim().to_string()))),
            // A typedef of a struct, or one we don't know: then only its
            // pointers are useful.
            None => aliases
                .get(&base)
                .map(|name| Some(Ty::Struct(name.clone()))),
        },
    };
    if pointers == 0 {
        return scalar;
    }
    // `void *` is `ptr`; `char **` is `ptr<ptr<i8>>`.
    let mut ty = match scalar {
        Some(Some(t)) => Some(t),
        _ => None,
    };
    for _ in 0..pointers {
        ty = Some(Ty::Ptr(ty.map(Box::new)));
    }
    Some(ty)
}

/// A parameter's type, without its name: `const char *fmt` is `const char *`.
/// The last word is a name unless it's part of a type (`unsigned long`,
/// `size_t`).
fn param_type(param: &str) -> &str {
    let param = param.trim();
    let name_start = param
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let name = &param[name_start..];
    let type_word = matches!(
        name,
        "int"
            | "long"
            | "short"
            | "char"
            | "unsigned"
            | "signed"
            | "double"
            | "float"
            | "void"
            | "bool"
            | "_Bool"
    ) || (name_start == 0 && c_type_plain(name).is_some());
    if name.is_empty() || type_word {
        param
    } else {
        param[..name_start].trim_end()
    }
}

/// Parses `int printf(const char *fmt, ...)` into a signature named for the
/// platform's linker (`_printf` on Darwin). None if a type isn't one we can
/// place in registers.
pub fn parse_prototype(text: &str, platform: Platform) -> Option<Signature> {
    parse_prototype_with(text, platform, &Aliases::new())
}

/// Like [`parse_prototype`], knowing the project's struct typedefs.
fn parse_prototype_with(text: &str, platform: Platform, aliases: &Aliases) -> Option<Signature> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let head = text[..open].trim();
    let name_start = head
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let name = &head[name_start..];
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let ret = c_type(&head[..name_start], aliases)?;
    // A struct by value comes back in memory, not x0.
    if matches!(ret, Some(Ty::Struct(_))) {
        return None;
    }
    let mut params = Vec::new();
    let mut variadic = false;
    let (mut ints, mut floats) = (0, 0);
    let list = text[open + 1..close].trim();
    if !(list.is_empty() || list == "void") {
        for param in list.split(',') {
            let param = param.trim();
            if param == "..." {
                variadic = true;
                continue;
            }
            let ty = c_type(param_type(param), aliases)?;
            // void as a parameter type, or a struct by value: not a
            // function we can place in registers.
            let ty = ty.filter(|t| !matches!(t, Ty::Struct(_)))?;
            let register = if matches!(ty, Ty::Float { .. }) {
                floats += 1;
                format!("v{}", floats - 1)
            } else {
                ints += 1;
                format!("x{}", ints - 1)
            };
            if ints > 8 || floats > 8 {
                // Past the registers: on the stack, which signatures don't cover.
                break;
            }
            params.push((register, ty));
        }
    }
    let symbol = match platform {
        Platform::Darwin => format!("_{name}"),
        Platform::Linux => name.to_string(),
    };
    Some(Signature {
        name: symbol,
        params,
        variadic,
        ret,
        returns_known: true,
    })
}

/// Common C library functions, by their C prototypes.
const LIBC: &str = "
int printf(const char *format, ...);
int fprintf(void *stream, const char *format, ...);
int sprintf(char *str, const char *format, ...);
int snprintf(char *str, size_t size, const char *format, ...);
int dprintf(int fd, const char *format, ...);
int puts(const char *s);
int putchar(int c);
int fputs(const char *s, void *stream);
int getchar(void);
size_t strlen(const char *s);
int strcmp(const char *a, const char *b);
int strncmp(const char *a, const char *b, size_t n);
char *strcpy(char *dst, const char *src);
char *strncpy(char *dst, const char *src, size_t n);
char *strcat(char *dst, const char *src);
char *strchr(const char *s, int c);
char *strdup(const char *s);
void *memcpy(void *dst, const void *src, size_t n);
void *memmove(void *dst, const void *src, size_t n);
void *memset(void *s, int c, size_t n);
int memcmp(const void *a, const void *b, size_t n);
void *malloc(size_t size);
void *calloc(size_t count, size_t size);
void *realloc(void *p, size_t size);
void free(void *p);
void exit(int status);
void abort(void);
int atoi(const char *s);
long atol(const char *s);
long strtol(const char *s, char **end, int base);
unsigned long strtoul(const char *s, char **end, int base);
double strtod(const char *s, char **end);
int abs(int n);
long labs(long n);
double sqrt(double x);
double sin(double x);
double cos(double x);
double pow(double x, double y);
float sqrtf(float x);
ssize_t write(int fd, const void *buf, size_t n);
ssize_t read(int fd, void *buf, size_t n);
int open(const char *path, int flags, ...);
int close(int fd);
";

/// The C library's signatures for a platform.
pub fn libc(platform: Platform) -> HashMap<String, Signature> {
    LIBC.lines()
        .filter_map(|line| parse_prototype(line, platform))
        .map(|s| (s.name.clone(), s))
        .collect()
}

/// Function declarations and definitions in C text: each `type name(params)`
/// at the top level, ahead of `;` or `{`. Comments, strings, and
/// preprocessor lines are skipped; `static` functions too, since assembly
/// in another file can't call them.
#[cfg(test)]
pub fn prototypes(text: &str, platform: Platform) -> Vec<Signature> {
    prototypes_with(text, platform, &Aliases::new())
}

fn prototypes_with(text: &str, platform: Platform, aliases: &Aliases) -> Vec<Signature> {
    let mut out = Vec::new();
    let mut statement = String::new();
    let mut depth = 0usize;
    let mut chars = text.chars().peekable();
    let mut line_start = true;
    while let Some(c) = chars.next() {
        match c {
            '#' if line_start => {
                // A preprocessor line, with continuations.
                let mut escaped = false;
                for c in chars.by_ref() {
                    if c == '\n' && !escaped {
                        break;
                    }
                    escaped = c == '\\';
                }
                line_start = true;
                continue;
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                line_start = true;
                statement.push(' ');
                continue;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
                statement.push(' ');
                continue;
            }
            '"' | '\'' => {
                let mut escaped = false;
                for d in chars.by_ref() {
                    if d == c && !escaped {
                        break;
                    }
                    escaped = d == '\\' && !escaped;
                }
            }
            '{' => {
                if depth == 0 {
                    finish(&statement, platform, aliases, &mut out);
                    statement.clear();
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    statement.clear();
                }
            }
            ';' if depth == 0 => {
                finish(&statement, platform, aliases, &mut out);
                statement.clear();
            }
            _ if depth == 0 => statement.push(c),
            _ => {}
        }
        line_start = c == '\n' || (line_start && c.is_whitespace());
    }
    out
}

fn finish(statement: &str, platform: Platform, aliases: &Aliases, out: &mut Vec<Signature>) {
    let statement = statement.trim();
    let first = statement.split_whitespace().next();
    // Not `static` (other files can't call it), not a typedef, a variable,
    // or a function pointer like `void (*f)(int)`.
    if matches!(first, Some("static" | "typedef"))
        || statement.contains('=')
        || statement.contains("(*")
        || !statement.ends_with(')')
    {
        return;
    }
    if let Some(signature) = parse_prototype_with(statement, platform, aliases) {
        out.push(signature);
    }
}

/// C text with comments, string and character literals, and preprocessor
/// lines blanked out, so the rest can be read by structure.
fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut line_start = true;
    while let Some(c) = chars.next() {
        match c {
            '#' if line_start => {
                let mut escaped = false;
                for c in chars.by_ref() {
                    if c == '\n' && !escaped {
                        break;
                    }
                    escaped = c == '\\';
                }
                out.push('\n');
                continue;
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                out.push('\n');
                line_start = true;
                continue;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
                out.push(' ');
                continue;
            }
            '"' | '\'' => {
                let mut escaped = false;
                for d in chars.by_ref() {
                    if d == c && !escaped {
                        break;
                    }
                    escaped = d == '\\' && !escaped;
                }
                out.push('0');
            }
            c => out.push(c),
        }
        line_start = c == '\n' || (line_start && c.is_whitespace());
    }
    out
}

/// A struct definition in C: its name, its fields as (name, C type, array
/// length), and the typedef name it's given, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CStruct {
    name: String,
    fields: Vec<(String, String, u64)>,
    alias: Option<String>,
}

/// `struct name { ... };` and `typedef struct [name] { ... } alias;` at the
/// top level. Structs we can't lay out exactly (unions, bit-fields, nested
/// definitions, multi-dimensional arrays) are left out.
fn c_structs(text: &str) -> Vec<CStruct> {
    let text = clean(text);
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut statement_start = 0;
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => statement_start = i + 1,
            's' if depth == 0
                && text[i..].starts_with("struct")
                && !text[..i].ends_with(is_ident)
                && !text[i + 6..].starts_with(is_ident) =>
            {
                let typedef = text[statement_start..i]
                    .split_whitespace()
                    .any(|w| w == "typedef");
                if let Some((found, end)) = struct_at(&text, i + 6, typedef) {
                    out.extend(found);
                    statement_start = end;
                    i = end;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// A struct definition after the `struct` keyword at `from`: the struct (if
/// it can be laid out) and where its statement ends.
fn struct_at(text: &str, from: usize, typedef: bool) -> Option<(Option<CStruct>, usize)> {
    let rest = &text[from..];
    let name_len = rest
        .trim_start()
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
    let skipped = rest.len() - rest.trim_start().len();
    let name = rest[skipped..skipped + name_len].to_string();
    let after = rest[skipped + name_len..].trim_start();
    if !after.starts_with('{') {
        return None;
    }
    let open = from + (rest.len() - after.len());
    let mut depth = 0;
    let close = text[open..].char_indices().find_map(|(k, c)| {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + k);
                }
            }
            _ => {}
        }
        None
    })?;
    let end = text[close..]
        .find(';')
        .map_or(text.len(), |k| close + k + 1);
    let declarators = text[close + 1..end.saturating_sub(1)].trim();
    let alias = typedef
        .then(|| declarators.split(',').next().unwrap_or("").trim())
        .filter(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(str::to_string);
    let name = if name.is_empty() {
        alias.clone()?
    } else {
        name
    };
    let body = &text[open + 1..close];
    if body.contains('{') || body.contains(':') || body.contains("union") {
        return Some((None, end));
    }
    let mut fields = Vec::new();
    for declaration in body.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let Some(parsed) = field_declaration(declaration) else {
            return Some((None, end));
        };
        fields.extend(parsed);
    }
    Some((
        Some(CStruct {
            name,
            fields,
            alias,
        }),
        end,
    ))
}

/// `int x, *y, z[4]` into (name, C type, count) for each declarator.
fn field_declaration(declaration: &str) -> Option<Vec<(String, String, u64)>> {
    let parts: Vec<&str> = declaration.split(',').collect();
    let first = parts.first()?.trim();
    let (base, _) = declarator(first)?;
    let base = base
        .trim_end_matches(|c: char| c == '*' || c.is_whitespace())
        .to_string();
    let mut out = Vec::new();
    for part in &parts {
        // The first part carries the base type; each carries its own stars.
        let (before, (name, count)) = declarator(part.trim())?;
        let stars = before.matches('*').count();
        out.push((name, format!("{base} {}", "*".repeat(stars)), count));
    }
    Some(out)
}

/// A declarator's name and array length, and the text before the name:
/// `char *name[16]` is ("char *", ("name", 16)).
fn declarator(text: &str) -> Option<(&str, (String, u64))> {
    let (text, count) = match text.strip_suffix(']') {
        Some(rest) => {
            let (before, n) = rest.rsplit_once('[')?;
            if before.trim_end().ends_with(']') {
                return None;
            }
            (before.trim_end(), n.trim().parse::<u64>().ok()?)
        }
        None => (text, 1),
    };
    let name_start = text
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let name = &text[name_start..];
    if name.is_empty() {
        return None;
    }
    Some((&text[..name_start], (name.to_string(), count)))
}

/// Lays out C structs, with what each needs declared first.
fn layouts(found: &[CStruct], aliases: &Aliases) -> HashMap<String, Layout> {
    let mut structs: HashMap<String, Layout> = HashMap::new();
    let mut pending: Vec<&CStruct> = found.iter().collect();
    // Each pass lays out the structs whose fields are all known.
    loop {
        let before = pending.len();
        pending.retain(|s| {
            let mut fields = Vec::new();
            for (name, ctype, count) in &s.fields {
                let Some(Some(ty)) = c_type(ctype, aliases) else {
                    return false;
                };
                let ty = ty.to_string();
                fields.push(if *count > 1 {
                    format!("{name}: {ty}[{count}]")
                } else {
                    format!("{name}: {ty}")
                });
            }
            let text = format!("{} {{ {} }}", s.name, fields.join(", "));
            match crate::types::parse_struct(&text, &structs) {
                Ok((name, layout)) => {
                    structs.insert(name, layout);
                    false
                }
                Err(_) => true,
            }
        });
        if pending.len() == before {
            break;
        }
    }
    structs
}

/// Functions and structs from the C files under a project root, cached
/// for a few seconds so an editor can ask on every edit.
pub fn project(root: &Path, platform: Platform) -> CDefinitions {
    type Cache = HashMap<(PathBuf, Platform), (Instant, CDefinitions)>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (root.to_path_buf(), platform);
    if let Ok(cache) = cache.lock()
        && let Some((when, found)) = cache.get(&key)
        && when.elapsed() < Duration::from_secs(2)
    {
        return found.clone();
    }
    let mut files = Vec::new();
    walk(root, &mut files);
    let texts: Vec<String> = files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .collect();
    let found = definitions(&texts, platform);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, (Instant::now(), found.clone()));
    }
    found
}

/// Functions and structs from C texts: structs and typedefs first, so
/// prototypes can name them.
fn definitions(texts: &[String], platform: Platform) -> CDefinitions {
    let found: Vec<CStruct> = texts.iter().flat_map(|t| c_structs(t)).collect();
    let aliases: Aliases = found
        .iter()
        .filter_map(|s| Some((s.alias.clone()?, s.name.clone())))
        .collect();
    let structs = layouts(&found, &aliases);
    let mut signatures = HashMap::new();
    for text in texts {
        for signature in prototypes_with(text, platform, &aliases) {
            signatures
                .entry(signature.name.clone())
                .or_insert(signature);
        }
    }
    CDefinitions {
        signatures,
        structs,
    }
}

/// C headers and sources, skipping hidden and build directories.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    const MAX: usize = 2000;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !name.starts_with('.')
                && !matches!(name.as_ref(), "target" | "build" | "node_modules")
            {
                walk(&path, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| e == "h" || e == "c" || e == "m")
        {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn darwin(text: &str) -> Signature {
        parse_prototype(text, Platform::Darwin).expect("parses")
    }

    #[test]
    fn prototypes_become_register_signatures() {
        let printf = darwin("int printf(const char *format, ...)");
        assert_eq!(printf.name, "_printf");
        assert_eq!(printf.params[0].0, "x0");
        assert_eq!(printf.params[0].1.to_string(), "ptr<i8>");
        assert!(printf.variadic);
        assert_eq!(
            printf.ret,
            Some(Ty::Int {
                signed: true,
                bits: 32
            })
        );
        // Floats count separately from integers.
        assert_eq!(param_type("unsigned long n"), "unsigned long");
        assert_eq!(param_type("unsigned long"), "unsigned long");
        assert_eq!(param_type("size_t"), "size_t");
        assert_eq!(param_type("const char *"), "const char *");
        let f = darwin("double scale(long n, double by, unsigned *out)");
        let regs: Vec<(String, String)> = f
            .params
            .iter()
            .map(|(r, t)| (r.clone(), t.to_string()))
            .collect();
        assert_eq!(
            regs,
            [
                ("x0".to_string(), "i64".to_string()),
                ("v0".to_string(), "f64".to_string()),
                ("x1".to_string(), "ptr<u32>".to_string())
            ]
        );
        assert_eq!(f.ret, Some(Ty::Float { bits: 64 }));
        let v = parse_prototype("void free(void *)", Platform::Linux).expect("parses");
        assert_eq!((v.name.as_str(), v.ret.clone()), ("free", None));
        assert_eq!(v.params[0].1, Ty::Ptr(None));
        assert_eq!(darwin("int getchar(void)").params, []);
        // A struct by value can't be placed: no signature.
        assert!(parse_prototype("int f(struct point p)", Platform::Darwin).is_none());
        // A pointer to one can.
        assert_eq!(
            darwin("int f(struct point *p)").params[0].1.to_string(),
            "ptr<point>"
        );
    }

    #[test]
    fn finds_prototypes_in_c_text() {
        let text = r#"#include <stdio.h>
#define MAX(a, b) ((a) > (b) ? (a) : (b))
/* int fake(void); */
typedef int (*callback)(int);
static int hidden(int x) { return x; }
int counter = 3;
struct point { int x, y; };
long add2(long a, long b);
const char *name(void) {
    return "{";
}
void (*handler)(int);
unsigned long hash(const unsigned char *s,
                   size_t n);
"#;
        let names: Vec<String> = prototypes(text, Platform::Linux)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["add2", "name", "hash"]);
    }

    #[test]
    fn finds_struct_definitions() {
        let text = r#"
#include <stdint.h>
/* struct fake { int a; }; */
struct point { int32_t x, y; };
typedef struct node {
    struct point pos;
    struct node *next;   // the list
    unsigned long id;
    char tag[4];
} node_t;
typedef struct { double re, im; } complex_t;
struct flags { unsigned a : 1; unsigned b : 3; };
union both { int i; float f; };
struct outer { struct { int x; } inner; };
struct grid { int cells[2][3]; };
struct point origin = { 0, 0 };
node_t *find(node_t *head, unsigned long id);
long sum(struct point p);
"#;
        let found = c_structs(text);
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["point", "node", "complex_t"], "{found:?}");
        let fields: Vec<(&str, &str, u64)> = found[1]
            .fields
            .iter()
            .map(|(n, t, c)| (n.as_str(), t.as_str(), *c))
            .collect();
        assert_eq!(
            fields,
            [
                ("pos", "struct point ", 1),
                ("next", "struct node *", 1),
                ("id", "unsigned long ", 1),
                ("tag", "char ", 4)
            ]
        );
        let defs = definitions(&[text.to_string()], Platform::Linux);
        let node = &defs.structs["node"];
        assert_eq!((node.size, node.align), (32, 8));
        assert_eq!(defs.structs["complex_t"].size, 16);
        assert!(!defs.structs.contains_key("flags") && !defs.structs.contains_key("grid"));
        // Prototypes see through the typedef; a struct by value is skipped.
        let find = &defs.signatures["find"];
        assert_eq!(find.params[0].1.to_string(), "ptr<node>");
        assert_eq!(
            find.ret.as_ref().map(ToString::to_string).as_deref(),
            Some("ptr<node>")
        );
        assert!(!defs.signatures.contains_key("sum"));
    }

    #[test]
    fn libc_has_the_usual_functions() {
        let darwin = libc(Platform::Darwin);
        assert!(darwin.contains_key("_printf") && darwin.contains_key("_malloc"));
        let linux = libc(Platform::Linux);
        assert_eq!(
            linux["strlen"].ret,
            Some(Ty::Int {
                signed: false,
                bits: 64
            })
        );
        assert!(linux.len() > 40, "{}", linux.len());
    }
}
