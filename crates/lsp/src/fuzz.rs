//! A fuzzed edit session: the real server, in memory, takes thousands of
//! random edits to real files and every request at random places after each
//! one. It must answer them all and never panic (AGENTS.md rule 4).

use std::path::{Path, PathBuf};
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{Value, json};

/// A small deterministic generator, so a failure replays from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Text that tends to break parsers: half-written operands, unbalanced
/// brackets and blocks, odd numbers, wide characters.
const PIECES: &[&str] = &[
    "\n",
    " ",
    ",",
    "[",
    "]",
    "]!",
    "{",
    "}",
    "#",
    "#-",
    "#0x",
    ":",
    "::",
    ";",
    "//",
    "/*",
    "*/",
    "\"",
    "'",
    "@",
    "@PAGE",
    "@PAGEOFF",
    "%%",
    "\\",
    "1b",
    "1f",
    "1:",
    "x0",
    "w31",
    "sp",
    "v0.16b",
    "z0.s",
    "p0/z",
    "za0h.s[w12, 0]",
    "{v0.4s, v1.4s}",
    "lsl #",
    "uxtw",
    "ldr x0, [",
    "ldp x29, x30, [sp, #-16]!",
    "bl _f",
    "b.",
    "b.eq",
    "adrp x0, _s@PAGE",
    "add",
    "mov x16, #4",
    "svc #0x80",
    ".macro m a\n",
    ".endm\n",
    ".if 1\n",
    ".else\n",
    ".endif\n",
    ".rept 3\n",
    ".endr\n",
    ".irp r, x0, x1\n",
    ".include \"nowhere.s\"\n",
    ".globl _f\n",
    "_f:",
    ".set n, 1\n",
    "n = n + 1\n",
    ".req",
    "tmp .req x9\n",
    ".unreq tmp\n",
    ".arch armv9-a\n",
    ".section __TEXT,__text\n",
    ".asciz \"é\"",
    "\u{e9}",
    "\u{1d11e}",
    "\t",
    "\r\n",
    "0b102",
    "99999999999999999999",
    "-",
    "(",
    ")",
    "~",
    "<<",
];

/// Requests at a position, with the params each needs beyond it.
const POSITIONAL: &[(&str, &str)] = &[
    ("textDocument/hover", "{}"),
    ("textDocument/completion", "{}"),
    ("textDocument/signatureHelp", "{}"),
    ("textDocument/definition", "{}"),
    (
        "textDocument/references",
        r#"{"context":{"includeDeclaration":true}}"#,
    ),
    ("textDocument/documentHighlight", "{}"),
    ("textDocument/prepareRename", "{}"),
    ("textDocument/rename", r#"{"newName":"_renamed"}"#),
    ("textDocument/prepareCallHierarchy", "{}"),
];

/// Requests on the whole document.
const WHOLE: &[(&str, &str)] = &[
    ("textDocument/documentSymbol", "{}"),
    ("textDocument/semanticTokens/full", "{}"),
    (
        "textDocument/formatting",
        r#"{"options":{"tabSize":4,"insertSpaces":true}}"#,
    ),
    ("textDocument/foldingRange", "{}"),
    ("textDocument/documentLink", "{}"),
];

struct Session {
    client: Connection,
    server: Option<std::thread::JoinHandle<bool>>,
    next_id: i32,
    /// Notifications from the server, in order.
    notifications: Vec<Notification>,
}

impl Session {
    fn start(utf8: bool) -> Session {
        let (client, server_side) = Connection::memory();
        let server = std::thread::spawn(move || {
            crate::server::run(&server_side).expect("the server runs to shutdown")
        });
        let mut session = Session {
            client,
            server: Some(server),
            next_id: 0,
            notifications: Vec::new(),
        };
        let encodings = if utf8 {
            json!(["utf-8"])
        } else {
            json!(["utf-16"])
        };
        session.request(
            "initialize",
            json!({ "capabilities": {
                "general": { "positionEncodings": encodings },
                "textDocument": { "completion": { "completionItem": { "snippetSupport": true } } },
            } }),
        );
        session.notify("initialized", json!({}));
        session
    }

    fn notify(&self, method: &str, params: Value) {
        let message = Message::Notification(Notification::new(method.into(), params));
        self.client
            .sender
            .send(message)
            .expect("server is listening");
    }

    /// Sends a request and waits for its response, keeping notifications.
    fn response(&mut self, method: &str, params: Value) -> lsp_server::Response {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        let request = Request::new(id.clone(), method.into(), params.clone());
        self.client
            .sender
            .send(Message::Request(request))
            .expect("server is listening");
        loop {
            let message = self
                .client
                .receiver
                .recv_timeout(Duration::from_secs(20))
                .unwrap_or_else(|e| panic!("no answer to {method} {params}: {e}"));
            match message {
                Message::Response(response) if response.id == id => return response,
                Message::Notification(notification) => self.notifications.push(notification),
                _ => {}
            }
        }
    }

    /// Sends a request and returns its result.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let response = self.response(method, params.clone());
        self.notifications.clear();
        match response.response_result {
            Ok(value) => value,
            // Refusals (renaming a register) are answers; malformed params
            // or unknown methods mean this test is wrong.
            Err(e) => {
                assert!(
                    e.code != -32602 && e.code != -32601,
                    "{method} {params}: {}",
                    e.message
                );
                Value::Null
            }
        }
    }

    fn finish(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let server = self.server.take().expect("started");
        assert!(server.join().expect("no panic"), "clean shutdown");
    }
}

/// A random edit, kept on character boundaries.
fn mutate(rng: &mut Rng, text: &mut String) {
    let boundary = |text: &str, mut at: usize| {
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let at = boundary(text, rng.below(text.len() + 1));
    match rng.below(6) {
        0 | 1 => text.insert_str(at, rng.pick(PIECES)),
        2 => {
            let end = boundary(text, (at + rng.below(24)).min(text.len()));
            text.replace_range(at..end, "");
        }
        3 => {
            // Duplicate a line, like a paste.
            let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
            let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
            let line = format!("{}\n", &text[start..end]);
            text.insert_str(start, &line);
        }
        4 => text.truncate(at),
        _ => {
            let piece = rng.pick(PIECES);
            let end = boundary(text, (at + piece.len()).min(text.len()));
            text.replace_range(at..end, piece);
        }
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else if path.extension().is_some_and(|e| e == "s") {
            out.push(path);
        }
    }
}

fn seeds() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    files(&root.join("examples"), &mut paths);
    files(&root.join("crates/syntax/tests/data/corpus"), &mut paths);
    paths.sort();
    let texts: Vec<String> = paths
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    assert!(texts.len() >= 10, "found the example and corpus files");
    texts
}

fn session(utf8: bool, seed: u64, edits: usize) {
    let mut rng = Rng(seed);
    let mut session = Session::start(utf8);
    let uri = "file:///armls-fuzz/edit.s";
    for original in seeds() {
        let mut text = original;
        session.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "asm", "version": 1, "text": text } }),
        );
        for version in 0..edits {
            for _ in 0..1 + rng.below(3) {
                mutate(&mut rng, &mut text);
            }
            session.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": version + 2 },
                    "contentChanges": [{ "text": text }],
                }),
            );
            let lines = text.lines().count() as u32 + 2;
            let position = |rng: &mut Rng| json!({ "line": rng.below(lines as usize), "character": rng.below(90) });
            for (method, extra) in POSITIONAL {
                let mut params: Value = serde_json::from_str(extra).expect("json");
                params["textDocument"] = json!({ "uri": uri });
                params["position"] = position(&mut rng);
                let result = session.request(method, params);
                if *method == "textDocument/prepareCallHierarchy"
                    && let Some(item) = result.get(0)
                {
                    session.request("callHierarchy/incomingCalls", json!({ "item": item }));
                    session.request("callHierarchy/outgoingCalls", json!({ "item": item }));
                }
            }
            for (method, extra) in WHOLE {
                let mut params: Value = serde_json::from_str(extra).expect("json");
                params["textDocument"] = json!({ "uri": uri });
                session.request(method, params);
            }
            let range = json!({ "start": position(&mut rng), "end": position(&mut rng) });
            session.request(
                "textDocument/inlayHint",
                json!({ "textDocument": { "uri": uri }, "range": range }),
            );
            session.request(
                "textDocument/codeAction",
                json!({
                    "textDocument": { "uri": uri },
                    "range": range,
                    "context": { "diagnostics": [] },
                }),
            );
            session.request("workspace/symbol", json!({ "query": "_" }));
        }
        session.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        );
    }
    session.finish();
}

#[test]
fn fuzzed_edit_session_utf8() {
    session(true, 0x5eed_0001, 40);
}

#[test]
fn fuzzed_edit_session_utf16() {
    session(false, 0x5eed_0002, 20);
}

/// Formatting a mutated file changes only whitespace, and formatting the
/// result changes nothing.
fn format_session(seed: u64, edits: usize) {
    let mut rng = Rng(seed);
    let options = arm_syntax::FormatOptions::default();
    let squeeze = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).collect() };
    for original in seeds() {
        let mut text = original;
        for _ in 0..edits {
            mutate(&mut rng, &mut text);
            let once = arm_syntax::format(&text, &options);
            assert_eq!(
                squeeze(&once),
                squeeze(&text),
                "seed {seed}: format changed code:\n{text}"
            );
            let twice = arm_syntax::format(&once, &options);
            assert_eq!(twice, once, "seed {seed}: format isn't stable on:\n{text}");
        }
    }
}

#[test]
fn formatting_mutated_files_is_safe_and_stable() {
    format_session(0x5eed_0003, 200);
}

/// A longer soak, off by default: `ARMLS_FUZZ=200 cargo test -p arm-lsp
/// --release soak` runs 200 more seeds. A failure names its seed.
#[test]
fn soak() {
    let runs: u64 = std::env::var("ARMLS_FUZZ")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    for seed in 1..=runs {
        eprintln!("seed {seed}");
        let seed = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        format_session(seed, 100);
        session(seed % 2 == 0, seed, 30);
    }
}

#[test]
fn bugs_are_answered_not_fatal() {
    let mut session = Session::start(true);
    // A panic in a request is an error response.
    let response = session.response("armls/testPanic", json!({}));
    let error = response.response_result.expect_err("an error");
    assert_eq!(error.code, -32603);
    assert!(error.message.contains("a test panic"), "{}", error.message);
    // A panic in a notification, and a malformed one, are logged.
    session.notify("armls/testPanic", json!({}));
    session.notify("textDocument/didOpen", json!({ "textDocument": 5 }));
    let uri = "file:///armls-fuzz/ok.s";
    session.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "asm", "version": 1, "text": "_f: ret\n" } }),
    );
    let hover = session.response(
        "textDocument/hover",
        json!({ "textDocument": { "uri": uri }, "position": { "line": 0, "character": 5 } }),
    );
    let logged: Vec<String> = session
        .notifications
        .iter()
        .filter(|n| n.method == "window/logMessage")
        .map(|n| n.params["message"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(logged.len(), 2, "{logged:?}");
    assert!(
        logged[0].contains("bug handling armls/testPanic"),
        "{logged:?}"
    );
    assert!(
        logged[1].starts_with("armls: textDocument/didOpen"),
        "{logged:?}"
    );
    // The session goes on.
    let value = hover.response_result.expect("hover");
    assert!(
        value["contents"]["value"]
            .as_str()
            .is_some_and(|t| t.contains("ret"))
    );
    session.finish();
}

#[test]
fn a_burst_of_edits_is_analyzed_once() {
    let mut session = Session::start(true);
    let uri = "file:///armls-fuzz/burst.s";
    // Big enough that analysis takes longer than sending the burst.
    let body = "_f:\n    add x0, x0, #1\n".repeat(2000);
    session.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "asm", "version": 1, "text": body } }),
    );
    for version in 2..52 {
        let text = format!("{body}    mov x{}, #0\n", version % 28);
        session.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        );
    }
    let last_line = body.lines().count();
    let hover = session.response(
        "textDocument/hover",
        json!({ "textDocument": { "uri": uri }, "position": { "line": last_line, "character": 9 } }),
    );
    let published = session
        .notifications
        .iter()
        .filter(|n| n.method == "textDocument/publishDiagnostics")
        .count();
    assert!(published < 25, "{published} analyses for 51 edits");
    // The last edit wins: x23 is the register in version 51.
    let value = hover.response_result.expect("hover");
    let text = value["contents"]["value"].as_str().unwrap_or_default();
    assert!(text.contains("x23"), "{text}");
    session.finish();
}
