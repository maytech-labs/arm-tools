//! The protocol layer: LSP messages in, `ide` calls, LSP messages out
//! (docs/decisions/0009-lsp-library.md). One thread, whole-file reanalysis on
//! every change.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentHighlightRequest, DocumentSymbolRequest,
    FoldingRangeRequest, Formatting, GotoDefinition, HoverRequest, InlayHintRequest,
    PrepareRenameRequest, References, Rename, Request as _, SemanticTokensFullRequest,
};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DiagnosticSeverity, DocumentHighlight, DocumentHighlightParams, DocumentSymbol,
    DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse,
    HoverContents, HoverParams, HoverProviderCapability, InitializeParams, Location, MarkupContent,
    MarkupKind, NumberOrString, OneOf, Position, PositionEncodingKind, PrepareRenameResponse,
    PublishDiagnosticsParams, ReferenceParams, RenameOptions, RenameParams, SemanticToken,
    SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, SymbolKind as LspSymbolKind,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
    WorkspaceEdit,
};

use arm_sema::{Analysis, Encoding, LineCol, Severity, SymbolKind};

use crate::ide;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Runs the server on a connection until the client says exit. Returns
/// whether the client sent `shutdown` first, as the protocol asks.
pub fn run(connection: &Connection) -> Result<bool> {
    let (id, params) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let encoding = negotiate_encoding(&params);
    let result = serde_json::json!({
        "capabilities": capabilities(encoding),
        "serverInfo": ServerInfo {
            name: "armls".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        },
    });
    connection.initialize_finish(id, result)?;

    let mut server = Server {
        connection,
        encoding,
        documents: HashMap::new(),
    };
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    return Ok(true);
                }
                server.request(request)?;
            }
            Message::Notification(notification) => {
                if notification.method == "exit" {
                    return Ok(false);
                }
                server.notification(notification)?;
            }
            Message::Response(_) => {}
        }
    }
    Ok(false)
}

/// UTF-8 when the client offers it, else UTF-16, which every client supports.
fn negotiate_encoding(params: &InitializeParams) -> Encoding {
    let offered = params
        .capabilities
        .general
        .as_ref()
        .and_then(|g| g.position_encodings.as_ref());
    if offered.is_some_and(|e| e.contains(&PositionEncodingKind::UTF8)) {
        Encoding::Utf8
    } else {
        Encoding::Utf16
    }
}

fn capabilities(encoding: Encoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(match encoding {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
        }),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into()]),
            ..CompletionOptions::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(lsp_types::CodeActionProviderCapability::Simple(true)),
        folding_range_provider: Some(lsp_types::FoldingRangeProviderCapability::Simple(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: legend(),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                range: None,
                work_done_progress_options: Default::default(),
            },
        )),
        ..ServerCapabilities::default()
    }
}

fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: ide::TOKEN_TYPES
            .iter()
            .map(|t| match t {
                ide::TokenType::Keyword => SemanticTokenType::KEYWORD,
                ide::TokenType::Macro => SemanticTokenType::MACRO,
                ide::TokenType::Variable => SemanticTokenType::VARIABLE,
                ide::TokenType::Function => SemanticTokenType::FUNCTION,
                ide::TokenType::Parameter => SemanticTokenType::PARAMETER,
                ide::TokenType::Comment => SemanticTokenType::COMMENT,
                ide::TokenType::Number => SemanticTokenType::NUMBER,
                ide::TokenType::String => SemanticTokenType::STRING,
                ide::TokenType::Decorator => SemanticTokenType::DECORATOR,
            })
            .collect(),
        // Bit order matches ide::DECLARATION, READONLY, DEFAULT_LIBRARY.
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::DEFAULT_LIBRARY,
        ],
    }
}

struct Document {
    analysis: Analysis,
    /// What was published, with fixes, for code actions.
    diagnostics: Vec<arm_sema::Diagnostic>,
}

struct Server<'c> {
    connection: &'c Connection,
    encoding: Encoding,
    documents: HashMap<Uri, Document>,
}

impl Server<'_> {
    fn notification(&mut self, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: lsp_types::DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let document = params.text_document;
                self.update(document.uri, &document.text)?;
            }
            DidChangeTextDocument::METHOD => {
                let params: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                // Full sync: the last change is the whole text.
                if let Some(change) = params.content_changes.into_iter().last() {
                    self.update(params.text_document.uri, &change.text)?;
                }
            }
            DidCloseTextDocument::METHOD => {
                let params: lsp_types::DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.documents.remove(&params.text_document.uri);
                self.publish(params.text_document.uri, Vec::new())?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Reanalyzes a document and publishes its diagnostics, then those of
    /// open documents that include it, so they see its unsaved edits.
    fn update(&mut self, uri: Uri, text: &str) -> Result<()> {
        self.refresh(uri.clone(), text)?;
        let changed = file_path(&uri).map(|p| arm_sema::canonical(&p));
        let dependents: Vec<(Uri, String)> = self
            .documents
            .iter()
            .filter(|(other, doc)| {
                **other != uri
                    && doc.analysis.symbols().iter().any(|s| {
                        s.origin
                            .as_ref()
                            .is_some_and(|o| Some(arm_sema::canonical(&o.path)) == changed)
                    })
            })
            .map(|(other, doc)| (other.clone(), doc.analysis.text().to_string()))
            .collect();
        for (other, text) in dependents {
            self.refresh(other, &text)?;
        }
        Ok(())
    }

    fn refresh(&mut self, uri: Uri, text: &str) -> Result<()> {
        // The nearest armtool.toml sets lint levels and include paths; a
        // broken one is shown, not ignored.
        let path = file_path(&uri).map(|p| arm_sema::normalize(&p));
        let config = match path.as_ref().and_then(|p| p.parent()) {
            Some(dir) => arm_lint::Config::discover(dir),
            None => Ok(arm_lint::Config::default()),
        };
        let open = self
            .documents
            .iter()
            .filter_map(|(u, d)| Some((file_path(u)?, d.analysis.text().to_string())))
            .collect();
        // A broken config is shown, and the file is still checked with the
        // defaults, without lint rules.
        let (config, lint, config_error) = match config {
            Ok(config) => (config, true, None),
            Err(error) => (arm_lint::Config::default(), false, Some(error)),
        };
        let checked = arm_lint::check(text, path.as_deref(), &config, open, lint);
        let analysis = checked.analysis;
        let mut found = checked.diagnostics;
        if let Some(error) = config_error {
            found.push(arm_sema::Diagnostic {
                range: 0..0,
                severity: Severity::Warning,
                code: "config",
                message: format!("{error}. Lint rules are off until it's fixed."),
                fix: None,
            });
        }
        // Branches clang skips, for editors to grey out.
        for range in analysis.inactive() {
            found.push(arm_sema::Diagnostic {
                range: range.clone(),
                severity: Severity::Hint,
                code: "inactive",
                message: "inactive: the condition is false here, so clang skips these lines".into(),
                fix: None,
            });
        }
        found.sort_by_key(|d| (d.range.start, d.range.end));
        let diagnostics = found
            .iter()
            .map(|d| lsp_types::Diagnostic {
                range: self.range(&analysis, d.range.clone()),
                severity: Some(match d.severity {
                    Severity::Error => DiagnosticSeverity::ERROR,
                    Severity::Warning => DiagnosticSeverity::WARNING,
                    Severity::Info => DiagnosticSeverity::INFORMATION,
                    Severity::Hint => DiagnosticSeverity::HINT,
                }),
                code: Some(NumberOrString::String(d.code.into())),
                source: Some("armls".into()),
                message: d.message.clone(),
                tags: (d.code == "inactive").then(|| vec![lsp_types::DiagnosticTag::UNNECESSARY]),
                ..lsp_types::Diagnostic::default()
            })
            .collect();
        self.documents.insert(
            uri.clone(),
            Document {
                analysis,
                diagnostics: found,
            },
        );
        self.publish(uri, diagnostics)
    }

    fn publish(&self, uri: Uri, diagnostics: Vec<lsp_types::Diagnostic>) -> Result<()> {
        let params = PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        };
        let notification = Notification::new(PublishDiagnostics::METHOD.into(), params);
        self.connection.sender.send(notification.into())?;
        Ok(())
    }

    fn request(&mut self, request: Request) -> Result<()> {
        let id = request.id.clone();
        let response = match self.dispatch(request) {
            Ok(response) => response,
            Err(Failure::InvalidParams(message)) => {
                Response::new_err(id, ErrorCode::InvalidParams as i32, message)
            }
            Err(Failure::MethodNotFound(method)) => Response::new_err(
                id,
                ErrorCode::MethodNotFound as i32,
                format!("armls doesn't support {method}"),
            ),
            Err(Failure::Refused(message)) => {
                Response::new_err(id, ErrorCode::RequestFailed as i32, message)
            }
        };
        self.connection.sender.send(response.into())?;
        Ok(())
    }

    fn dispatch(&self, request: Request) -> std::result::Result<Response, Failure> {
        let id = request.id.clone();
        match request.method.as_str() {
            HoverRequest::METHOD => {
                let params: HoverParams = params(request)?;
                let at = &params.text_document_position_params;
                let result = self.at(at).and_then(|(doc, offset)| {
                    let hover = ide::hover(&doc.analysis, offset)?;
                    Some(lsp_types::Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: hover.markdown,
                        }),
                        range: Some(self.range(&doc.analysis, hover.range)),
                    })
                });
                Ok(ok(id, result))
            }
            Completion::METHOD => {
                let params: CompletionParams = params(request)?;
                let result = self
                    .at(&params.text_document_position)
                    .map(|(doc, offset)| {
                        let items = ide::completions(&doc.analysis, offset)
                            .into_iter()
                            .map(completion_item)
                            .collect();
                        CompletionResponse::Array(items)
                    });
                Ok(ok(id, result))
            }
            GotoDefinition::METHOD => {
                let params: GotoDefinitionParams = params(request)?;
                let at = &params.text_document_position_params;
                let result = self.at(at).and_then(|(doc, offset)| {
                    let definition = ide::definition(&doc.analysis, offset)?;
                    let location = match &definition.origin {
                        None => Location {
                            uri: at.text_document.uri.clone(),
                            range: self.range(&doc.analysis, definition.range),
                        },
                        Some(origin) => Location {
                            uri: path_uri(&origin.path)?,
                            range: self.range_in(&origin.text, definition.range),
                        },
                    };
                    Some(GotoDefinitionResponse::Scalar(location))
                });
                Ok(ok(id, result))
            }
            References::METHOD => {
                let params: ReferenceParams = params(request)?;
                let at = &params.text_document_position;
                let result = self.at(at).map(|(doc, offset)| {
                    let include = params.context.include_declaration;
                    ide::references(&doc.analysis, offset, include)
                        .into_iter()
                        .map(|range| Location {
                            uri: at.text_document.uri.clone(),
                            range: self.range(&doc.analysis, range),
                        })
                        .collect::<Vec<_>>()
                });
                Ok(ok(id, result))
            }
            DocumentHighlightRequest::METHOD => {
                let params: DocumentHighlightParams = params(request)?;
                let at = &params.text_document_position_params;
                let result = self.at(at).map(|(doc, offset)| {
                    ide::references(&doc.analysis, offset, true)
                        .into_iter()
                        .map(|range| DocumentHighlight {
                            range: self.range(&doc.analysis, range),
                            kind: None,
                        })
                        .collect::<Vec<_>>()
                });
                Ok(ok(id, result))
            }
            DocumentSymbolRequest::METHOD => {
                let params: DocumentSymbolParams = params(request)?;
                let result = self.documents.get(&params.text_document.uri).map(|doc| {
                    let symbols = ide::outline(&doc.analysis)
                        .into_iter()
                        .map(|item| self.document_symbol(&doc.analysis, item))
                        .collect();
                    DocumentSymbolResponse::Nested(symbols)
                });
                Ok(ok(id, result))
            }
            SemanticTokensFullRequest::METHOD => {
                let params: SemanticTokensParams = params(request)?;
                let result = self.documents.get(&params.text_document.uri).map(|doc| {
                    SemanticTokensResult::Tokens(SemanticTokens {
                        result_id: None,
                        data: self.encode_tokens(&doc.analysis),
                    })
                });
                Ok(ok(id, result))
            }
            PrepareRenameRequest::METHOD => {
                let params: TextDocumentPositionParams = params(request)?;
                let Some((doc, offset)) = self.at(&params) else {
                    return Ok(ok(id, None::<PrepareRenameResponse>));
                };
                let range = ide::prepare_rename(&doc.analysis, offset).map_err(Failure::Refused)?;
                let range = self.range(&doc.analysis, range);
                Ok(ok(id, Some(PrepareRenameResponse::Range(range))))
            }
            Rename::METHOD => {
                let params: RenameParams = params(request)?;
                let at = &params.text_document_position;
                let Some((doc, offset)) = self.at(at) else {
                    return Ok(ok(id, None::<WorkspaceEdit>));
                };
                let edits = ide::rename(&doc.analysis, offset, &params.new_name)
                    .map_err(Failure::Refused)?
                    .into_iter()
                    .map(|(range, new_text)| TextEdit {
                        range: self.range(&doc.analysis, range),
                        new_text,
                    })
                    .collect();
                // Uri's interior is never mutated; lsp-types requires it as the key.
                #[allow(clippy::mutable_key_type)]
                let changes = HashMap::from([(at.text_document.uri.clone(), edits)]);
                Ok(ok(
                    id,
                    Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..WorkspaceEdit::default()
                    }),
                ))
            }
            Formatting::METHOD => {
                let params: lsp_types::DocumentFormattingParams = params(request)?;
                let uri = &params.text_document.uri;
                let Some(doc) = self.documents.get(uri) else {
                    return Ok(ok(id, None::<Vec<TextEdit>>));
                };
                // The nearest armtool.toml's [format] settings, as armtool fmt uses.
                let config = file_path(uri)
                    .as_deref()
                    .and_then(Path::parent)
                    .map_or(Ok(arm_lint::Config::default()), arm_lint::Config::discover)
                    .map_err(Failure::Refused)?;
                let text = doc.analysis.text();
                let formatted = arm_syntax::format(text, &config.format);
                let edits = if formatted == text {
                    Vec::new()
                } else {
                    vec![TextEdit {
                        range: self.range(&doc.analysis, 0..text.len()),
                        new_text: formatted,
                    }]
                };
                Ok(ok(id, Some(edits)))
            }
            CodeActionRequest::METHOD => {
                let params: lsp_types::CodeActionParams = params(request)?;
                let uri = &params.text_document.uri;
                let Some(doc) = self.documents.get(uri) else {
                    return Ok(ok(id, None::<lsp_types::CodeActionResponse>));
                };
                let wanted = &params.range;
                let actions: lsp_types::CodeActionResponse = doc
                    .diagnostics
                    .iter()
                    .filter_map(|d| {
                        let fix = d.fix.as_ref()?;
                        let range = self.range(&doc.analysis, d.range.clone());
                        let overlaps = range.start <= wanted.end && wanted.start <= range.end;
                        if !overlaps {
                            return None;
                        }
                        let edits = fix
                            .edits
                            .iter()
                            .map(|(r, new_text)| TextEdit {
                                range: self.range(&doc.analysis, r.clone()),
                                new_text: new_text.clone(),
                            })
                            .collect();
                        Some(lsp_types::CodeActionOrCommand::CodeAction(
                            lsp_types::CodeAction {
                                title: fix.title.clone(),
                                kind: Some(lsp_types::CodeActionKind::QUICKFIX),
                                edit: Some(WorkspaceEdit {
                                    changes: Some(HashMap::from([(uri.clone(), edits)])),
                                    ..WorkspaceEdit::default()
                                }),
                                is_preferred: Some(true),
                                ..lsp_types::CodeAction::default()
                            },
                        ))
                    })
                    .collect();
                Ok(ok(id, Some(actions)))
            }
            InlayHintRequest::METHOD => {
                let params: lsp_types::InlayHintParams = params(request)?;
                let result = self.documents.get(&params.text_document.uri).map(|doc| {
                    ide::inlay_hints(&doc.analysis)
                        .into_iter()
                        .map(|(offset, label)| (self.position(&doc.analysis, offset), label))
                        .filter(|(at, _)| params.range.start <= *at && *at <= params.range.end)
                        .map(|(position, label)| lsp_types::InlayHint {
                            position,
                            label: lsp_types::InlayHintLabel::String(label),
                            kind: None,
                            text_edits: None,
                            tooltip: None,
                            padding_left: Some(true),
                            padding_right: None,
                            data: None,
                        })
                        .collect::<Vec<_>>()
                });
                Ok(ok(id, result))
            }
            FoldingRangeRequest::METHOD => {
                let params: lsp_types::FoldingRangeParams = params(request)?;
                let result = self.documents.get(&params.text_document.uri).map(|doc| {
                    ide::folding_ranges(&doc.analysis)
                        .into_iter()
                        .map(|(start, end)| lsp_types::FoldingRange {
                            start_line: start,
                            end_line: end,
                            ..lsp_types::FoldingRange::default()
                        })
                        .collect::<Vec<_>>()
                });
                Ok(ok(id, result))
            }
            method => Err(Failure::MethodNotFound(method.to_string())),
        }
    }

    /// The open document and byte offset a position points at.
    fn at(&self, at: &TextDocumentPositionParams) -> Option<(&Document, usize)> {
        let doc = self.documents.get(&at.text_document.uri)?;
        let analysis = &doc.analysis;
        let position = LineCol {
            line: at.position.line,
            col: at.position.character,
        };
        let offset = analysis
            .line_index()
            .offset(analysis.text(), position, self.encoding);
        Some((doc, offset))
    }

    fn position(&self, analysis: &Analysis, offset: usize) -> Position {
        let at = analysis
            .line_index()
            .line_col(analysis.text(), offset, self.encoding);
        Position::new(at.line, at.col)
    }

    /// A range in some other file's text, like an included file.
    fn range_in(&self, text: &str, range: Range<usize>) -> lsp_types::Range {
        let index = arm_sema::LineIndex::new(text);
        let at = |offset| {
            let p = index.line_col(text, offset, self.encoding);
            Position::new(p.line, p.col)
        };
        lsp_types::Range::new(at(range.start), at(range.end))
    }

    fn range(&self, analysis: &Analysis, range: Range<usize>) -> lsp_types::Range {
        lsp_types::Range::new(
            self.position(analysis, range.start),
            self.position(analysis, range.end),
        )
    }

    #[allow(deprecated)] // `DocumentSymbol::deprecated` must still be set.
    fn document_symbol(&self, analysis: &Analysis, item: ide::OutlineItem) -> DocumentSymbol {
        let (kind, detail) = match item.kind {
            SymbolKind::Label if item.exported => (LspSymbolKind::FUNCTION, "global label"),
            SymbolKind::Label | SymbolKind::LocalLabel => (LspSymbolKind::FUNCTION, "label"),
            SymbolKind::Constant => (LspSymbolKind::CONSTANT, "constant"),
            SymbolKind::RegisterAlias => (LspSymbolKind::VARIABLE, "register alias"),
            SymbolKind::Macro => (LspSymbolKind::OPERATOR, "macro"),
        };
        DocumentSymbol {
            name: item.name,
            detail: Some(detail.into()),
            kind,
            tags: None,
            deprecated: None,
            range: self.range(analysis, item.range),
            selection_range: self.range(analysis, item.selection),
            children: None,
        }
    }

    /// LSP's relative encoding: each token as deltas from the previous one.
    fn encode_tokens(&self, analysis: &Analysis) -> Vec<SemanticToken> {
        let mut out = Vec::new();
        let (mut line, mut col) = (0, 0);
        for token in ide::semantic_tokens(analysis) {
            let start = self.position(analysis, token.range.start);
            let end = self.position(analysis, token.range.end);
            let delta_line = start.line - line;
            let delta_start = if delta_line == 0 {
                start.character - col
            } else {
                start.character
            };
            let kind = ide::TOKEN_TYPES
                .iter()
                .position(|t| *t == token.kind)
                .unwrap_or_default();
            out.push(SemanticToken {
                delta_line,
                delta_start,
                length: end.character.saturating_sub(start.character),
                token_type: u32::try_from(kind).unwrap_or_default(),
                token_modifiers_bitset: token.modifiers,
            });
            (line, col) = (start.line, start.character);
        }
        out
    }
}

/// A `file:` URI for a path, percent-encoding what URIs don't allow.
fn path_uri(path: &Path) -> Option<Uri> {
    let mut text = String::from("file://");
    for byte in path.to_str()?.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            text.push(byte as char);
        } else {
            text.push_str(&format!("%{byte:02X}"));
        }
    }
    text.parse().ok()
}

/// The local path of a `file:` URI, percent-decoded.
fn file_path(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.as_str().strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (
            bytes[i],
            bytes.get(i + 1).copied().and_then(hex),
            bytes.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(u8::try_from(hi * 16 + lo).ok()?);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

enum Failure {
    InvalidParams(String),
    MethodNotFound(String),
    /// The request was understood but can't be done, like renaming a register.
    Refused(String),
}

fn params<P: serde::de::DeserializeOwned>(request: Request) -> std::result::Result<P, Failure> {
    serde_json::from_value(request.params)
        .map_err(|e| Failure::InvalidParams(format!("{}: {e}", request.method)))
}

fn ok<R: serde::Serialize>(id: RequestId, result: R) -> Response {
    Response::new_ok(id, result)
}

fn completion_item(completion: ide::Completion) -> CompletionItem {
    use ide::CompletionKind as K;
    let kind = match completion.kind {
        K::Instruction | K::Condition => CompletionItemKind::KEYWORD,
        K::Macro => CompletionItemKind::FUNCTION,
        K::Directive => CompletionItemKind::KEYWORD,
        K::Register => CompletionItemKind::VARIABLE,
        K::Label => CompletionItemKind::REFERENCE,
        K::Constant => CompletionItemKind::CONSTANT,
        K::RegisterAlias => CompletionItemKind::VARIABLE,
    };
    CompletionItem {
        label: completion.label,
        kind: Some(kind),
        detail: completion.detail,
        ..CompletionItem::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn paths_round_trip_through_uris() {
        let path = PathBuf::from("/Users/me/My Code/a+b.s");
        let uri = path_uri(&path).expect("uri");
        assert_eq!(uri.as_str(), "file:///Users/me/My%20Code/a%2Bb.s");
        assert_eq!(file_path(&uri), Some(path));
    }

    #[test]
    fn file_uris_become_paths() {
        let uri = Uri::from_str("file:///Users/me/My%20Code/a.s").expect("uri");
        assert_eq!(
            file_path(&uri),
            Some(PathBuf::from("/Users/me/My Code/a.s"))
        );
        let other = Uri::from_str("untitled:Untitled-1").expect("uri");
        assert_eq!(file_path(&other), None);
    }
}
