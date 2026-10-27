//! Signatures from C: prototypes in the project's headers and sources, and
//! common C library functions, turned into register signatures by the
//! AAPCS64 rules (docs/decisions/0040-c-signatures.md).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arm_isa::Platform;

use crate::types::{Signature, Ty};

/// The C type of a parameter or result, as far as registers care.
fn c_type(text: &str) -> Option<Option<Ty>> {
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
        // A struct or a typedef we don't know: only its pointers are useful.
        _ => None,
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
    ) || (name_start == 0 && c_type(name).is_some());
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
    let ret = c_type(&head[..name_start])?;
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
            let ty = c_type(param_type(param))?;
            // void as a parameter type: not a function we can place.
            let ty = ty?;
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
pub fn prototypes(text: &str, platform: Platform) -> Vec<Signature> {
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
                    finish(&statement, platform, &mut out);
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
                finish(&statement, platform, &mut out);
                statement.clear();
            }
            _ if depth == 0 => statement.push(c),
            _ => {}
        }
        line_start = c == '\n' || (line_start && c.is_whitespace());
    }
    out
}

fn finish(statement: &str, platform: Platform, out: &mut Vec<Signature>) {
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
    if let Some(signature) = parse_prototype(statement, platform) {
        out.push(signature);
    }
}

/// Signatures from the C files under a project root, cached for a few
/// seconds so an editor can ask on every edit.
pub fn project(root: &Path, platform: Platform) -> HashMap<String, Signature> {
    type Cache = HashMap<(PathBuf, Platform), (Instant, HashMap<String, Signature>)>;
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
    let mut found = HashMap::new();
    for file in files {
        if let Ok(text) = std::fs::read_to_string(&file) {
            for signature in prototypes(&text, platform) {
                found.entry(signature.name.clone()).or_insert(signature);
            }
        }
    }
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, (Instant::now(), found.clone()));
    }
    found
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
        assert_eq!(darwin("int f(struct point *p)").params[0].1, Ty::Ptr(None));
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
