mod analysis;
mod catalog;
mod completion;
mod editor_tokens;
mod formatting;
mod protocol;
mod server_events;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};
use server_events::{CANCELLED, Cancellation, Event, IndexProgress, Reply};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io,
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};
use xrack_ir::modules::normalize_source_path;

struct Document {
    version: i64,
    text: Arc<str>,
    analysis: analysis::Analysis,
}

fn main() -> ExitCode {
    let (events, receiver) = mpsc::channel();
    let reader = events.clone();
    let result = thread::Builder::new()
        .name("xrack-lsp-input".into())
        .spawn(move || {
            server_events::read_input(io::stdin().lock(), reader);
        })
        .and_then(|_| serve(receiver, events, &mut io::stdout().lock()));
    match result {
        Ok(status) => status,
        Err(error) => {
            eprintln!("xrack-lsp: {error}");
            ExitCode::FAILURE
        }
    }
}

const EDIT_DELAY: Duration = Duration::from_millis(35);

fn serve(
    receiver: Receiver<Event>,
    events: Sender<Event>,
    output: &mut impl io::Write,
) -> io::Result<ExitCode> {
    let index = completion::BackgroundIndex::new(events)?;
    let mut documents: BTreeMap<String, Document> = BTreeMap::new();
    let mut published: BTreeMap<String, Value> = BTreeMap::new();
    let mut roots = BTreeSet::new();
    let mut initialized = false;
    let mut shutdown = false;
    let mut eof = false;
    let mut pending = HashMap::new();
    let mut edits = BTreeSet::new();
    let mut deadline: Option<Instant> = None;
    let mut progress = Progress::default();
    let mut index_started = false;
    loop {
        if eof && pending.is_empty() && progress.active.is_empty() {
            break;
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            flush_edits(output, &index, &mut documents, &mut published, &mut edits)?;
            deadline = None;
        }
        let event = if let Some(due) = deadline {
            match receiver.recv_timeout(due.saturating_duration_since(Instant::now())) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    flush_edits(output, &index, &mut documents, &mut published, &mut edits)?;
                    deadline = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            let Ok(event) = receiver.recv() else { break };
            event
        };
        let message = match event {
            Event::Input(message) => {
                let Some(message) = message? else {
                    flush_edits(output, &index, &mut documents, &mut published, &mut edits)?;
                    deadline = None;
                    eof = true;
                    continue;
                };
                message
            }
            Event::Reply {
                id,
                cancellation,
                reply,
            } => {
                if pending
                    .get(&id)
                    .is_some_and(|request: &Cancellation| request.same_request(&cancellation))
                {
                    pending.remove(&id);
                    respond(output, &id, reply)?;
                }
                continue;
            }
            Event::Progress(id, update) => {
                if !shutdown {
                    progress.update(output, id, update)?;
                }
                continue;
            }
        };
        let Some(method) = message["method"].as_str() else {
            progress.acknowledge(output, &message)?;
            continue;
        };
        let params = &message["params"];
        let text_document = &params["textDocument"];
        let uri = text_document["uri"].as_str();

        if method == "$/cancelRequest" {
            if let Some(request) = pending.remove(&params["id"]) {
                request.cancel();
                respond(output, &params["id"], Err(CANCELLED))?;
            }
            continue;
        }
        if method != "textDocument/didChange" && method != "exit" && method != "shutdown" {
            flush_edits(output, &index, &mut documents, &mut published, &mut edits)?;
            deadline = None;
        }
        if method == "exit" {
            if !shutdown {
                index.stop();
            }
            for request in pending.values() {
                request.cancel();
            }
            return Ok(if shutdown {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        if method == "initialized" && initialized && !shutdown && !index_started {
            index_started = true;
            if !roots.is_empty() {
                start_index(output, &index, &mut progress, roots.clone())?;
            }
            continue;
        }
        if let Some(id) = message.get("id") {
            if initialized
                && !shutdown
                && matches!(
                    method,
                    "workspace/symbol"
                        | "textDocument/definition"
                        | "textDocument/references"
                        | "textDocument/prepareRename"
                        | "textDocument/rename"
                        | "textDocument/completion"
                        | "textDocument/codeAction"
                )
            {
                if !index_started {
                    index.set_roots(roots.clone(), None)?;
                    index_started = true;
                }
                // Snapshot the open buffers at the request boundary. The index
                // processes the query after all previously queued updates.
                let needs_workspace = matches!(
                    method,
                    "textDocument/definition" | "textDocument/completion"
                );
                let buffers = if needs_workspace {
                    documents
                        .iter()
                        .map(|(uri, doc)| (uri.clone(), doc.text.clone()))
                        .collect()
                } else {
                    uri.and_then(|uri| {
                        documents
                            .get(uri)
                            .map(|doc| (uri.to_owned(), doc.text.clone()))
                    })
                    .into_iter()
                    .collect()
                };
                let version = uri
                    .and_then(|uri| documents.get(uri))
                    .map(|doc| doc.version);
                let diagnostics = uri
                    .and_then(|uri| published.get(uri))
                    .cloned()
                    .unwrap_or_default();
                let method = method.to_owned();
                let params = params.clone();
                let cancellation = Cancellation::default();
                index.request(
                    id.clone(),
                    cancellation.clone(),
                    move |index, cancellation| {
                        index_request(
                            index,
                            cancellation,
                            &method,
                            &params,
                            &buffers,
                            version,
                            &diagnostics,
                        )
                    },
                )?;
                pending.insert(id.clone(), cancellation);
                continue;
            }
            let result = match method {
                _ if shutdown => Err((-32600, "server has shut down")),
                "initialize" if !initialized => {
                    initialized = true;
                    roots = params["workspaceFolders"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|folder| completion::file_path(folder["uri"].as_str()?))
                        .map(|path| normalize_source_path(&path))
                        .collect();
                    if roots.is_empty()
                        && let Some(path) =
                            params["rootUri"].as_str().and_then(completion::file_path)
                    {
                        roots.insert(normalize_source_path(&path));
                    }
                    if let Some(path) = params["initializationOptions"]["indexCacheUri"]
                        .as_str()
                        .and_then(completion::file_path)
                    {
                        index.cache_directory(path)?;
                    }
                    progress.enabled = params["capabilities"]["window"]["workDoneProgress"] == true;
                    if !progress.enabled {
                        index.set_roots(roots.clone(), None)?;
                        index_started = true;
                    }
                    Ok(json!({
                        "capabilities": {
                            "positionEncoding": "utf-16",
                            "textDocumentSync": { "openClose": true, "change": 1 },
                            "documentSymbolProvider": true,
                            "workspaceSymbolProvider": true,
                            "definitionProvider": true,
                            "documentFormattingProvider": true,
                            "hoverProvider": true,
                            "signatureHelpProvider": { "triggerCharacters": ["(", ","], "retriggerCharacters": [")"] },
                            "documentLinkProvider": {},
                            "codeActionProvider": { "codeActionKinds": ["quickfix"] },
                            "referencesProvider": true,
                            "renameProvider": { "prepareProvider": true },
                            "workspace": { "workspaceFolders": { "supported": true, "changeNotifications": true } },
                            "documentHighlightProvider": true,
                            "semanticTokensProvider": { "legend": completion::legend(), "full": true },
                            "completionProvider": { "triggerCharacters": [".", "@", "<", ":"] },
                        },
                        "serverInfo": { "name": "xrack-lsp" },
                    }))
                }
                _ if !initialized => Err((-32002, "server is not initialized")),
                "shutdown" => {
                    shutdown = true;
                    deadline = None;
                    edits.clear();
                    progress.finish(output)?;
                    index.save_cache()?;
                    for (id, request) in pending.drain() {
                        request.cancel();
                        respond(output, &id, Err(CANCELLED))?;
                    }
                    Ok(Value::Null)
                }
                "textDocument/documentSymbol" => match uri {
                    Some(uri) => Ok(json!(
                        documents
                            .get(uri)
                            .map_or(&[][..], |doc| &doc.analysis.symbols)
                    )),
                    None => Err((-32602, "missing textDocument URI")),
                },
                "textDocument/formatting" => match uri {
                    Some(uri) => Ok(documents.get(uri).map_or_else(
                        || json!([]),
                        |doc| formatting::edits(&doc.text, &params["options"]),
                    )),
                    None => Err((-32602, "missing textDocument URI")),
                },
                "textDocument/documentLink" => match uri {
                    Some(uri) => Ok(documents
                        .get(uri)
                        .and_then(|doc| {
                            let workspace = completion::Workspace::new(
                                uri,
                                documents
                                    .iter()
                                    .map(|(uri, doc)| (uri.as_str(), doc.text.as_ref())),
                            )?;
                            Some(completion::document_links(&doc.text, &workspace))
                        })
                        .unwrap_or_else(|| json!([]))),
                    None => Err((-32602, "missing textDocument URI")),
                },
                "textDocument/signatureHelp" => {
                    let position = params["position"]["line"]
                        .as_u64()
                        .zip(params["position"]["character"].as_u64())
                        .map(|(line, column)| (line as usize, column as usize));
                    match uri.zip(position) {
                        Some((uri, position)) => {
                            Ok(documents.get(uri).map_or(Value::Null, |doc| {
                                let workspace = completion::Workspace::new(
                                    uri,
                                    documents
                                        .iter()
                                        .map(|(uri, doc)| (uri.as_str(), doc.text.as_ref())),
                                );
                                completion::signature_help(&doc.text, position, workspace.as_ref())
                            }))
                        }
                        None => Err((-32602, "missing document or position")),
                    }
                }
                "textDocument/hover"
                | "textDocument/documentHighlight"
                | "textDocument/semanticTokens/full" => {
                    let position = params["position"]["line"]
                        .as_u64()
                        .zip(params["position"]["character"].as_u64())
                        .map(|(line, column)| (line as usize, column as usize));
                    match uri {
                        Some(uri) => Ok(documents.get(uri).map_or(Value::Null, |doc| {
                            let workspace = completion::Workspace::new(
                                uri,
                                documents
                                    .iter()
                                    .map(|(uri, doc)| (uri.as_str(), doc.text.as_ref())),
                            );
                            completion::query(method, uri, &doc.text, position, workspace.as_ref())
                        })),
                        None => Err((-32602, "missing textDocument URI")),
                    }
                }
                _ => Err((-32601, "method not found")),
            };
            respond(output, id, result)?;
            continue;
        }
        if initialized && !shutdown && method == "workspace/didChangeWorkspaceFolders" {
            for (key, add) in [("removed", false), ("added", true)] {
                for folder in params["event"][key].as_array().into_iter().flatten() {
                    if let Some(path) = folder["uri"].as_str().and_then(completion::file_path) {
                        let path = normalize_source_path(&path);
                        if add {
                            roots.insert(path);
                        } else {
                            roots.remove(&path);
                        }
                    }
                }
            }
            start_index(output, &index, &mut progress, roots.clone())?;
            index_started = true;
            continue;
        }
        if initialized && !shutdown && method == "workspace/didChangeWatchedFiles" {
            let changes: Vec<_> = params["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|change| {
                    let path = completion::file_path(change["uri"].as_str()?)?;
                    Some((path, change["type"] == 3))
                })
                .collect();
            let paths = changes
                .iter()
                .map(|(path, _)| normalize_source_path(path))
                .collect();
            index.watched(changes)?;
            refresh_analyses(
                output,
                &mut documents,
                &mut published,
                &BTreeSet::new(),
                paths,
            )?;
            continue;
        }
        let Some(uri) = uri.filter(|_| initialized && !shutdown) else {
            continue;
        };
        match method {
            "textDocument/didOpen" => {
                if let (Some(text), Some(version)) = (
                    text_document["text"].as_str(),
                    text_document["version"].as_i64(),
                ) {
                    documents.insert(
                        uri.into(),
                        Document {
                            version,
                            text: text.into(),
                            analysis: analysis::Analysis::default(),
                        },
                    );
                } else {
                    continue;
                }
            }
            "textDocument/didChange" => {
                let change = &params["contentChanges"][0];
                if let (Some(document), Some(version), Some(text)) = (
                    documents.get_mut(uri),
                    text_document["version"].as_i64(),
                    change["text"].as_str(),
                ) && version > document.version
                    && change.get("range").is_none()
                {
                    document.version = version;
                    document.text = text.into();
                    edits.insert(uri.to_owned());
                    deadline.get_or_insert_with(|| Instant::now() + EDIT_DELAY);
                    continue;
                } else {
                    continue;
                }
            }
            "textDocument/didClose" => {
                if documents.remove(uri).is_none() {
                    continue;
                }
            }
            _ => continue,
        }
        let path = completion::file_path(uri).map(|path| normalize_source_path(&path));
        if let Some(document) = documents.get(uri) {
            index.open([(uri.to_owned(), path.clone(), document.text.clone())])?;
        } else {
            index.close(uri.to_owned())?;
        }
        let paths = path.into_iter().collect();
        refresh_analyses(
            output,
            &mut documents,
            &mut published,
            &BTreeSet::from([uri.to_owned()]),
            paths,
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

fn flush_edits(
    output: &mut impl io::Write,
    index: &completion::BackgroundIndex,
    documents: &mut BTreeMap<String, Document>,
    published: &mut BTreeMap<String, Value>,
    edits: &mut BTreeSet<String>,
) -> io::Result<()> {
    if edits.is_empty() {
        return Ok(());
    }
    let changed = std::mem::take(edits);
    let updates: Vec<_> = changed
        .iter()
        .map(|uri| {
            let path = completion::file_path(uri).map(|path| normalize_source_path(&path));
            (uri.clone(), path, documents[uri].text.clone())
        })
        .collect();
    let paths = updates
        .iter()
        .filter_map(|(_, path, _)| path.clone())
        .collect();
    index.open(updates)?;
    refresh_analyses(output, documents, published, &changed, paths)
}

#[cfg(test)]
fn refresh_analysis(
    output: &mut impl io::Write,
    documents: &mut BTreeMap<String, Document>,
    published: &mut BTreeMap<String, Value>,
    changed_uri: Option<&str>,
    paths: BTreeSet<PathBuf>,
) -> io::Result<()> {
    refresh_analyses(
        output,
        documents,
        published,
        &changed_uri.into_iter().map(str::to_owned).collect(),
        paths,
    )
}

fn refresh_analyses(
    output: &mut impl io::Write,
    documents: &mut BTreeMap<String, Document>,
    published: &mut BTreeMap<String, Value>,
    changed: &BTreeSet<String>,
    paths: BTreeSet<PathBuf>,
) -> io::Result<()> {
    // Each analysis records its transitive reads, including failed imports.
    let refreshed: Vec<_> = documents
        .iter()
        .filter(|(uri, doc)| {
            changed.contains(*uri)
                || paths
                    .iter()
                    .any(|path| doc.analysis.inputs.contains_key(path))
                || completion::file_path(uri)
                    .is_some_and(|path| paths.contains(&normalize_source_path(&path)))
        })
        .map(|(uri, doc)| {
            let workspace = completion::Workspace::new(
                uri,
                documents
                    .iter()
                    .map(|(uri, doc)| (uri.as_str(), doc.text.as_ref())),
            );
            (
                uri.clone(),
                analysis::analyze(uri, &doc.text, workspace.as_ref()),
            )
        })
        .collect();
    for (uri, analysis) in refreshed {
        documents.get_mut(&uri).expect("open document").analysis = analysis;
    }

    // Combine reports from all entry documents. A dependency may be shared by
    // several open editors, so one successful analysis cannot clear another's errors.
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for doc in documents.values() {
        for (uri, diagnostics) in &doc.analysis.diagnostics {
            let items = grouped.entry(uri.clone()).or_default();
            for diagnostic in diagnostics {
                if !items.contains(diagnostic) {
                    items.push(diagnostic.clone());
                }
            }
        }
    }
    for uri in documents.keys() {
        grouped.entry(uri.clone()).or_default();
    }
    let mut next = BTreeMap::new();
    for (uri, diagnostics) in grouped {
        let mut params = json!({ "uri": uri, "diagnostics": diagnostics });
        if let Some(doc) = documents.get(&uri) {
            params["version"] = json!(doc.version);
        }
        if published.get(&uri) != Some(&params) {
            publish_diagnostics(output, params.clone())?;
        }
        next.insert(uri, params);
    }
    for uri in published.keys().filter(|uri| !next.contains_key(*uri)) {
        publish_diagnostics(output, json!({ "uri": uri, "diagnostics": [] }))?;
    }
    *published = next;
    Ok(())
}

fn publish_diagnostics(output: &mut impl io::Write, params: Value) -> io::Result<()> {
    protocol::write_message(
        output,
        &json!({
            "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params,
        }),
    )
}

fn respond(output: &mut impl io::Write, id: &Value, reply: Reply) -> io::Result<()> {
    let response = match reply {
        Ok(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
        Err((code, message)) => {
            json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}})
        }
    };
    protocol::write_message(output, &response)
}

#[derive(Default)]
struct Progress {
    enabled: bool,
    next_id: u64,
    pending: BTreeMap<u64, Vec<IndexProgress>>,
    active: BTreeSet<u64>,
}

fn progress_id(id: u64) -> String {
    format!("xrack-index-{id}")
}

fn start_index(
    output: &mut impl io::Write,
    index: &completion::BackgroundIndex,
    progress: &mut Progress,
    roots: BTreeSet<PathBuf>,
) -> io::Result<()> {
    let id = if progress.enabled && !roots.is_empty() {
        let id = progress.next_id;
        progress.next_id += 1;
        let token = progress_id(id);
        protocol::write_message(
            output,
            &json!({
                "jsonrpc":"2.0", "id":token, "method":"window/workDoneProgress/create", "params":{"token":token},
            }),
        )?;
        progress.pending.insert(id, Vec::new());
        Some(id)
    } else {
        None
    };
    index.set_roots(roots, id)
}

impl Progress {
    fn finish(&mut self, output: &mut impl io::Write) -> io::Result<()> {
        self.pending.clear();
        let active = std::mem::take(&mut self.active);
        for id in active {
            protocol::write_message(
                output,
                &json!({
                    "jsonrpc":"2.0", "method":"$/progress", "params":{"token":progress_id(id), "value":{"kind":"end"}},
                }),
            )?;
        }
        Ok(())
    }

    fn acknowledge(&mut self, output: &mut impl io::Write, response: &Value) -> io::Result<()> {
        let Some(id) = response["id"]
            .as_str()
            .and_then(|id| id.strip_prefix("xrack-index-"))
            .and_then(|id| id.parse::<u64>().ok())
        else {
            return Ok(());
        };
        if let Some(updates) = self.pending.remove(&id)
            && response.get("result").is_some()
            && response.get("error").is_none()
        {
            self.active.insert(id);
            for update in updates {
                self.update(output, id, update)?;
            }
        }
        Ok(())
    }

    fn update(
        &mut self,
        output: &mut impl io::Write,
        id: u64,
        update: IndexProgress,
    ) -> io::Result<()> {
        if let Some(pending) = self.pending.get_mut(&id) {
            pending.push(update);
            return Ok(());
        }
        if !self.active.contains(&id) {
            return Ok(());
        }
        let value = match update {
            IndexProgress::Begin => {
                json!({"kind":"begin", "title":"Indexing XRack workspace", "message":"Discovering source files", "percentage":0})
            }
            IndexProgress::Report { done, total } => json!({
                "kind":"report","message":format!("{done}/{total} files"),
                "percentage": (done * 100).checked_div(total).unwrap_or(100)
            }),
            IndexProgress::End => {
                self.active.remove(&id);
                json!({"kind":"end", "message":"XRack workspace index ready"})
            }
        };
        protocol::write_message(
            output,
            &json!({
                "jsonrpc":"2.0", "method":"$/progress", "params":{"token":progress_id(id), "value":value},
            }),
        )
    }
}

fn index_request(
    index: &mut completion::WorkspaceIndex,
    cancellation: &Cancellation,
    method: &str,
    params: &Value,
    buffers: &BTreeMap<String, Arc<str>>,
    version: Option<i64>,
    diagnostics: &Value,
) -> Reply {
    if method == "workspace/symbol" {
        return params["query"]
            .as_str()
            .map(|query| index.workspace_symbols(query, cancellation))
            .ok_or((-32602, "missing symbol query"));
    }
    let uri = params["textDocument"]["uri"]
        .as_str()
        .ok_or((-32602, "missing textDocument URI"))?;
    let text = buffers.get(uri);
    let workspace = matches!(
        method,
        "textDocument/definition" | "textDocument/completion"
    )
    .then(|| {
        completion::Workspace::new(
            uri,
            buffers
                .iter()
                .map(|(uri, text)| (uri.as_str(), text.as_ref())),
        )
    })
    .flatten();
    let path = completion::file_path(uri).map(|path| normalize_source_path(&path));
    let exports = |prefix: &str, kind| {
        path.as_ref().map_or_else(Vec::new, |path| {
            index.auto_imports(path, prefix, kind, cancellation)
        })
    };
    if method == "textDocument/codeAction" {
        return Ok(json!(
            text.zip(version)
                .zip(diagnostics["diagnostics"].as_array())
                .map_or_else(Vec::new, |((text, version), diagnostics)| {
                    completion::import_fixes(uri, text, version, diagnostics, params, exports)
                })
        ));
    }
    let position = params["position"]["line"]
        .as_u64()
        .zip(params["position"]["character"].as_u64())
        .map(|(line, character)| (line as usize, character as usize))
        .ok_or((-32602, "missing document or position"))?;
    if method == "textDocument/completion" {
        return Ok(json!(text.map_or_else(Vec::new, |text| {
            completion::complete_with_imports(
                text,
                position.0,
                position.1,
                workspace.as_ref(),
                exports,
            )
        })));
    }
    let text = text.ok_or((-32602, "document is not open"))?;
    let result = index
        .refactor(method, uri, position, params, cancellation)
        .map_err(|message| (-32602, message))?;
    if method == "textDocument/definition" && result.is_null() && !cancellation.cancelled() {
        return Ok(workspace
            .as_ref()
            .and_then(|workspace| completion::import_definition(text, position, workspace))
            .unwrap_or_else(|| {
                completion::query(method, uri, text, Some(position), workspace.as_ref())
            }));
    }
    Ok(result)
}
