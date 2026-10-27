//! The `armls` contract: the real binary, driven over stdio the way an editor
//! drives it (docs/decisions/0007-testing.md). Every request has a timeout, so
//! a hung or crashed server fails the test instead of hanging it.

// The test client may panic on a broken server (AGENTS.md: expect is fine in tests).
#![allow(clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(10);
const URI: &str = "file:///tmp/test.s";

struct Client {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
    next_id: i64,
    /// Notifications received while waiting for a response.
    notifications: Vec<Value>,
    initialize: Value,
}

impl Client {
    /// Starts armls and completes the handshake. `utf8` offers UTF-8 positions.
    fn start(utf8: bool) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_armls"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("armls starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (sender, messages) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(message) = read_message(&mut reader) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        let mut client = Client {
            child,
            stdin,
            messages,
            next_id: 0,
            notifications: Vec::new(),
            initialize: Value::Null,
        };
        let encodings = if utf8 {
            json!(["utf-8", "utf-16"])
        } else {
            json!(["utf-16"])
        };
        let response = client.request(
            "initialize",
            json!({ "capabilities": { "general": { "positionEncodings": encodings } } }),
        );
        client.initialize = response["result"].clone();
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, message: Value) {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write");
        self.stdin.flush().expect("flush");
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Sends a request and returns the whole response object.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let message = self.receive();
            if message["id"] == json!(id) {
                return message;
            }
            self.notifications.push(message);
        }
    }

    fn receive(&mut self) -> Value {
        match self.messages.recv_timeout(TIMEOUT) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => panic!("armls didn't answer within {TIMEOUT:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("armls exited or crashed"),
        }
    }

    /// The next diagnostics published for `URI`.
    fn diagnostics(&mut self) -> Vec<Value> {
        let is_ours = |m: &Value| {
            m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == URI
        };
        if let Some(i) = self.notifications.iter().position(is_ours) {
            let message = self.notifications.remove(i);
            return message["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
        loop {
            let message = self.receive();
            if is_ours(&message) {
                return message["params"]["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
            }
            self.notifications.push(message);
        }
    }

    fn open(&mut self, text: &str) -> Vec<Value> {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": URI, "languageId": "asm", "version": 1, "text": text } }),
        );
        self.diagnostics()
    }

    fn change(&mut self, version: i64, text: &str) -> Vec<Value> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": URI, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        );
        self.diagnostics()
    }

    /// A request at a line and character in `URI`; returns `result`.
    fn at(&mut self, method: &str, line: u32, character: u32, extra: Value) -> Value {
        let mut params = json!({
            "textDocument": { "uri": URI },
            "position": { "line": line, "character": character },
        });
        if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
            params.extend(extra.clone());
        }
        let response = self.request(method, params);
        assert!(
            response.get("error").is_none(),
            "{method} failed: {response}"
        );
        response["result"].clone()
    }

    fn shutdown(mut self) -> ExitStatus {
        let response = self.request("shutdown", Value::Null);
        assert!(response.get("error").is_none(), "{response}");
        self.notify("exit", Value::Null);
        self.wait()
    }

    fn wait(mut self) -> ExitStatus {
        drop(self.stdin);
        self.child.wait().expect("armls exits")
    }
}

fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length: ") {
            length = n.parse().ok();
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn start(line: u32, character: u32) -> Value {
    json!({ "line": line, "character": character })
}

#[test]
fn version_and_help() {
    let version = Command::new(env!("CARGO_BIN_EXE_armls"))
        .arg("--version")
        .output()
        .expect("runs");
    assert!(version.status.success());
    let text = String::from_utf8_lossy(&version.stdout);
    assert!(
        text.starts_with("armls ") && text.contains("LLVM 21.1.8"),
        "{text}"
    );

    let help = Command::new(env!("CARGO_BIN_EXE_armls"))
        .arg("--help")
        .output()
        .expect("runs");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("language server"));

    let bad = Command::new(env!("CARGO_BIN_EXE_armls"))
        .arg("--frob")
        .output()
        .expect("runs");
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn advertises_capabilities_and_negotiates_encoding() {
    let client = Client::start(true);
    let caps = &client.initialize["capabilities"];
    assert_eq!(caps["positionEncoding"], "utf-8");
    for capability in [
        "hoverProvider",
        "completionProvider",
        "definitionProvider",
        "referencesProvider",
        "documentHighlightProvider",
        "documentSymbolProvider",
        "renameProvider",
        "semanticTokensProvider",
        "documentFormattingProvider",
        "codeActionProvider",
        "foldingRangeProvider",
    ] {
        assert!(!caps[capability].is_null(), "missing {capability}");
    }
    assert_eq!(client.initialize["serverInfo"]["name"], "armls");
    assert!(client.shutdown().success());

    let client = Client::start(false);
    assert_eq!(
        client.initialize["capabilities"]["positionEncoding"],
        "utf-16"
    );
    assert!(client.shutdown().success());
}

#[test]
fn exit_without_shutdown_is_exit_code_1() {
    let mut client = Client::start(true);
    client.notify("exit", Value::Null);
    assert_eq!(client.wait().code(), Some(1));
}

#[test]
fn publishes_diagnostics_on_open_and_change() {
    let mut client = Client::start(true);
    let diagnostics = client.open("_f: nop\n_f: frob x0\nmov x0, x1,\n");
    // Leave out lint hints (severity 4), like the unused-label hint on _f.
    let diagnostics: Vec<Value> = diagnostics
        .into_iter()
        .filter(|d| d["severity"] != 4)
        .collect();
    let summary: Vec<_> = diagnostics
        .iter()
        .map(|d| {
            (
                d["code"].as_str().unwrap_or_default().to_string(),
                d["range"]["start"]["line"].as_u64().unwrap_or(99),
                d["severity"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("duplicate-symbol".to_string(), 1, 1),
            ("unknown-instruction".to_string(), 1, 1),
            ("syntax".to_string(), 2, 1),
        ]
    );
    assert!(diagnostics.iter().all(|d| d["source"] == "armls"));
    assert!(
        diagnostics[1]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("unknown instruction")
    );

    // Fixing the file clears them.
    assert_eq!(
        client.change(2, ".globl _f\n_f: ret\n"),
        Vec::<Value>::new()
    );

    // Closing clears them too.
    client.notify(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": URI } }),
    );
    assert_eq!(client.diagnostics(), Vec::<Value>::new());
    assert!(client.shutdown().success());
}

/// Opens `text` as a file in a fresh directory, with an optional
/// armtool.toml beside it, and returns its diagnostics' codes and severities.
fn lint_codes(text: &str, config: Option<&str>) -> Vec<(String, u64)> {
    let dir = std::env::temp_dir().join(format!(
        "armls-lint-{}-{}",
        std::process::id(),
        config.map_or(0, str::len)
    ));
    std::fs::create_dir_all(&dir).expect("dir");
    if let Some(config) = config {
        std::fs::write(dir.join("armtool.toml"), config).expect("config");
    }
    let uri = format!("file://{}/a.s", dir.display());
    let mut client = Client::start(true);
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": uri, "languageId": "asm", "version": 1, "text": text } }),
    );
    let message = loop {
        let message = client.receive();
        if message["method"] == "textDocument/publishDiagnostics" && message["params"]["uri"] == uri
        {
            break message;
        }
    };
    assert!(client.shutdown().success());
    let _ = std::fs::remove_dir_all(&dir);
    message["params"]["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            (
                d["code"].as_str().unwrap_or_default().to_string(),
                d["severity"].as_u64().unwrap_or(0),
            )
        })
        .collect()
}

#[test]
fn lint_rules_run_with_the_nearest_config() {
    let text = ".globl _f\n_f:\n    mov x18, #1\n    ret\n";
    assert_eq!(
        lint_codes(text, None),
        [("abi/x18-reserved".to_string(), 2)]
    );
    let off = "[rules]\n\"abi/x18-reserved\" = \"off\"\n";
    assert_eq!(lint_codes(text, Some(off)), Vec::<(String, u64)>::new());
    let broken = "[rules]\n\"abi/x18-reserve\" = \"off\"\n";
    assert_eq!(lint_codes(text, Some(broken)), [("config".to_string(), 2)]);
}

#[test]
fn includes_resolve_across_files_and_unsaved_edits() {
    let dir = std::env::temp_dir().join(format!("armls-includes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("armtool.toml"), "").expect("config");
    let macros = ".macro zeroreg r\n    mov \\r, #0\n.endm\n";
    std::fs::write(dir.join("macros.s"), macros).expect("macros");
    let main_uri = format!("file://{}/main.s", dir.display());
    let macros_uri = format!("file://{}/macros.s", dir.display());
    let main = ".include \"macros.s\"\n.globl _f\n_f:\n    zeroreg x0\n    ret\n";
    let mut client = Client::start(true);
    let open = |client: &mut Client, uri: &str, text: &str| {
        client.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "asm", "version": 1, "text": text } }),
        );
    };
    let diagnostics_for = |client: &mut Client, uri: &str| loop {
        let message = client.receive();
        if message["method"] == "textDocument/publishDiagnostics" && message["params"]["uri"] == uri
        {
            return message["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
    };
    open(&mut client, &main_uri, main);
    assert_eq!(diagnostics_for(&mut client, &main_uri), Vec::<Value>::new());

    // Go to definition on the macro call lands in macros.s. (`zero` alone is
    // an SME instruction, hence the longer name.)
    let definition = client.request(
        "textDocument/definition",
        json!({ "textDocument": { "uri": main_uri }, "position": { "line": 3, "character": 5 } }),
    )["result"]
        .clone();
    assert_eq!(definition["uri"], macros_uri.as_str());
    assert_eq!(definition["range"]["start"], start(0, 7));

    // Editing macros.s without saving updates main.s.
    open(&mut client, &macros_uri, macros);
    let _ = diagnostics_for(&mut client, &macros_uri);
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": macros_uri, "version": 2 },
            "contentChanges": [{ "text": ".macro clear r\n.endm\n" }],
        }),
    );
    let _ = diagnostics_for(&mut client, &macros_uri);
    let main_now = diagnostics_for(&mut client, &main_uri);
    assert!(
        main_now.iter().any(|d| d["code"] == "unknown-instruction"),
        "{main_now:?}"
    );
    assert!(client.shutdown().success());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn formatting() {
    let mut client = Client::start(true);
    client.open(".globl _f\n_f:  mov x0,#1\n  ret\n");
    let edits = client.request(
        "textDocument/formatting",
        json!({ "textDocument": { "uri": URI }, "options": { "tabSize": 4, "insertSpaces": true } }),
    )["result"]
        .clone();
    let edits = edits.as_array().cloned().unwrap_or_default();
    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[0]["newText"],
        "    .globl  _f\n_f: mov     x0, #1\n    ret\n"
    );
    assert_eq!(edits[0]["range"]["start"], start(0, 0));
    // Already formatted: no edits.
    client.change(2, "    ret\n");
    let none = client.request(
        "textDocument/formatting",
        json!({ "textDocument": { "uri": URI }, "options": { "tabSize": 4, "insertSpaces": true } }),
    )["result"]
        .clone();
    assert_eq!(none, json!([]));
    assert!(client.shutdown().success());
}

#[test]
fn quick_fixes_and_folding() {
    let mut client = Client::start(true);
    client.open(
        ".globl _f\n_f:\n    movv x0, x1\n    sub sp, sp, #24\n    add sp, sp, #32\n    ret\n",
    );
    let actions = client.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": URI },
            "range": { "start": start(2, 4), "end": start(2, 4) },
            "context": { "diagnostics": [] },
        }),
    )["result"]
        .clone();
    let actions = actions.as_array().cloned().unwrap_or_default();
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0]["title"], "Change to 'mov'");
    assert_eq!(actions[0]["kind"], "quickfix");
    let edit = &actions[0]["edit"]["changes"][URI][0];
    assert_eq!(edit["newText"], "mov");
    assert_eq!(edit["range"]["start"], start(2, 4));

    let actions = client.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": URI },
            "range": { "start": start(3, 0), "end": start(3, 20) },
            "context": { "diagnostics": [] },
        }),
    )["result"]
        .clone();
    let titles: Vec<&str> = actions
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["title"].as_str())
        .collect();
    assert!(titles.contains(&"Round to 32"), "{titles:?}");

    let folds = client.request(
        "textDocument/foldingRange",
        json!({ "textDocument": { "uri": URI } }),
    )["result"]
        .clone();
    assert_eq!(folds[0]["startLine"], 1);
    assert_eq!(folds[0]["endLine"], 5);
    assert!(client.shutdown().success());
}

#[test]
fn inactive_branches_are_greyed_out() {
    let mut client = Client::start(true);
    let diagnostics = client.open(".if 0\nfrob\n.endif\n");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["code"], "inactive");
    assert_eq!(diagnostics[0]["severity"], 4);
    assert_eq!(diagnostics[0]["tags"], json!([1]));
    assert!(client.shutdown().success());
}

#[test]
fn hover() {
    let mut client = Client::start(true);
    client.open("_f: ldp x29, x30, [sp], #16\n    b _f\n");
    let mnemonic = client.at("textDocument/hover", 0, 5, json!({}));
    let text = mnemonic["contents"]["value"].as_str().unwrap_or_default();
    assert!(text.starts_with("**ldp**"), "{text}");
    assert_eq!(mnemonic["contents"]["kind"], "markdown");
    assert_eq!(mnemonic["range"]["start"], start(0, 4));

    let register = client.at("textDocument/hover", 0, 10, json!({}));
    let text = register["contents"]["value"].as_str().unwrap_or_default();
    assert!(text.contains("frame pointer"), "{text}");

    let label = client.at("textDocument/hover", 1, 7, json!({}));
    let text = label["contents"]["value"].as_str().unwrap_or_default();
    assert!(text.contains("**_f** (label, 1 use)"), "{text}");

    assert!(client.at("textDocument/hover", 0, 26, json!({})).is_null());
    assert!(client.shutdown().success());
}

#[test]
fn completion() {
    let mut client = Client::start(true);
    client.open("_start: nop\n    \n    b \n");
    let items = client.at("textDocument/completion", 1, 4, json!({}));
    let labels: Vec<&str> = items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["label"].as_str())
        .collect();
    assert!(
        labels.contains(&"ldr") && labels.contains(&".globl"),
        "{}",
        labels.len()
    );

    let items = client.at("textDocument/completion", 2, 6, json!({}));
    let labels: Vec<&str> = items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["label"].as_str())
        .collect();
    assert!(labels.contains(&"_start") && labels.contains(&"x0"));
    assert!(!labels.contains(&"ldr"));
    assert!(client.shutdown().success());
}

#[test]
fn definition_references_and_highlight() {
    let mut client = Client::start(true);
    client.open("    b _f\n_f: nop\n    bl _f\n");
    let definition = client.at("textDocument/definition", 0, 6, json!({}));
    assert_eq!(definition["uri"], URI);
    assert_eq!(definition["range"]["start"], start(1, 0));

    let references = client.at(
        "textDocument/references",
        0,
        6,
        json!({ "context": { "includeDeclaration": true } }),
    );
    let lines: Vec<u64> = references
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["range"]["start"]["line"].as_u64())
        .collect();
    assert_eq!(lines, [0, 1, 2]);

    let highlights = client.at("textDocument/documentHighlight", 1, 1, json!({}));
    assert_eq!(highlights.as_array().map(Vec::len), Some(3));

    // Nothing to find on a register.
    assert!(
        client
            .at("textDocument/definition", 0, 4, json!({}))
            .is_null()
    );
    assert!(client.shutdown().success());
}

#[test]
fn utf16_positions_count_code_units() {
    // The crab is 4 bytes and 2 UTF-16 units, so `_f` starts at byte 13 but
    // UTF-16 unit 11.
    let text = "; 🦀 comment\n🦀🦀; x\n_f: nop\n    b _f\n";
    let line = 3;
    for (utf8, character) in [(true, 6), (false, 6)] {
        let mut client = Client::start(utf8);
        client.open(text);
        let definition = client.at("textDocument/definition", line, character, json!({}));
        assert_eq!(definition["range"]["start"], start(2, 0));
        assert!(client.shutdown().success());
    }
    // On a line with a crab before the name, the encodings differ.
    let text = "/*🦀*/ _f: nop\n    b _f\n";
    let mut client = Client::start(true);
    client.open(text);
    let definition = client.at("textDocument/definition", 1, 6, json!({}));
    assert_eq!(definition["range"]["start"], start(0, 9));
    assert!(client.shutdown().success());
    let mut client = Client::start(false);
    client.open(text);
    let definition = client.at("textDocument/definition", 1, 6, json!({}));
    assert_eq!(definition["range"]["start"], start(0, 7));
    assert!(client.shutdown().success());
}

#[test]
fn document_symbols() {
    let mut client = Client::start(true);
    client.open(".globl _main\n_main:\n1: b 1b\nhelper:\n  ret\nlimit = 4\n");
    let symbols = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": URI } }),
    )["result"]
        .clone();
    let names: Vec<(&str, &str)> = symbols
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            (
                s["name"].as_str().unwrap_or(""),
                s["detail"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("_main", "global label"),
            ("helper", "label"),
            ("limit", "constant")
        ]
    );
    assert!(client.shutdown().success());
}

#[test]
fn semantic_tokens() {
    let mut client = Client::start(true);
    client.open("_f: add x0, x1, #4 ; hi\n");
    let result = client.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": URI } }),
    )["result"]
        .clone();
    let data: Vec<u64> = result["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect();
    assert_eq!(data.len() % 5, 0);
    // _f, add, x0, x1, 4, comment.
    assert_eq!(data.len() / 5, 6);
    // The first token is `_f` at 0:0, length 2, a function, declared.
    assert_eq!(&data[..5], &[0, 0, 2, 3, 1]);
    let legend = &client.initialize["capabilities"]["semanticTokensProvider"]["legend"];
    assert_eq!(legend["tokenTypes"][3], "function");
    assert!(client.shutdown().success());
}

#[test]
fn rename() {
    let mut client = Client::start(true);
    client.open("_f: nop\n    b _f\n    mov x0, x1\n");
    let prepared = client.at("textDocument/prepareRename", 1, 7, json!({}));
    assert_eq!(prepared["start"], start(1, 6));

    let edit = client.at("textDocument/rename", 1, 7, json!({ "newName": "_g" }));
    let edits = edit["changes"][URI].as_array().cloned().unwrap_or_default();
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|e| e["newText"] == "_g"));

    // A register can't be renamed; the error says why.
    let refused = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": URI },
            "position": { "line": 2, "character": 9 },
            "newName": "x5",
        }),
    );
    let message = refused["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("can be renamed"), "{refused}");
    assert!(client.shutdown().success());
}

#[test]
fn unknown_requests_and_bad_params_get_errors() {
    let mut client = Client::start(true);
    let unknown = client.request("textDocument/frobnicate", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    let bad = client.request("textDocument/hover", json!({ "nonsense": true }));
    assert_eq!(bad["error"]["code"], -32602);
    // Requests for documents that aren't open answer null.
    let missing = client.request(
        "textDocument/hover",
        json!({ "textDocument": { "uri": "file:///nope.s" }, "position": start(0, 0) }),
    );
    assert!(missing["result"].is_null(), "{missing}");
    assert!(client.shutdown().success());
}

/// A tiny deterministic generator, so a failure reproduces.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound.max(1)
    }
}

#[test]
fn survives_random_edits_and_requests() {
    let corpus = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../syntax/tests/data/corpus/macros-and-bodies.s"
    ))
    .expect("corpus file");
    let snippets = [
        "",
        "\n",
        "x",
        "[",
        "]!",
        "#",
        "\"",
        "/*",
        "1:",
        "b 1f",
        ".macro m",
        ".endm",
        "🦀",
        "ldp x0, x1, [sp, #-16]!",
        "%%",
        ":lo12:",
        "@PAGE",
        "\\a",
        ".rept 3",
        "\r\n",
        "é",
    ];
    let mut client = Client::start(false);
    let mut text = corpus;
    client.open(&text);
    let mut rng = Lcg(0x5eed);
    let methods = [
        "textDocument/hover",
        "textDocument/completion",
        "textDocument/definition",
        "textDocument/documentHighlight",
        "textDocument/prepareRename",
    ];
    for version in 2..300 {
        // Replace a random char-aligned range with a random snippet.
        let mut a = rng.next(text.len() + 1);
        let mut b = (a + rng.next(20)).min(text.len());
        while !text.is_char_boundary(a) {
            a -= 1;
        }
        while !text.is_char_boundary(b) {
            b += 1;
        }
        text.replace_range(a..b, snippets[rng.next(snippets.len())]);
        client.change(version, &text);
        let lines = text.lines().count() as u32 + 1;
        for method in methods {
            let params = json!({
                "textDocument": { "uri": URI },
                "position": { "line": rng.next(lines as usize), "character": rng.next(40) },
            });
            let response = client.request(method, params);
            assert!(response.get("result").is_some() || response.get("error").is_some());
        }
        if version % 25 == 0 {
            client.request(
                "textDocument/semanticTokens/full",
                json!({ "textDocument": { "uri": URI } }),
            );
            client.request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": URI } }),
            );
        }
    }
    assert!(client.shutdown().success());
}
