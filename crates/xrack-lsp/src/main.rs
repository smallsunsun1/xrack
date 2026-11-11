mod analysis;
mod catalog;
mod completion;
mod editor_tokens;
mod protocol;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};
use std::{collections::HashMap, io, process::ExitCode};

struct Document {
    version: i64,
    text: String,
    symbols: Vec<Value>,
}

fn main() -> ExitCode {
    match serve(&mut io::stdin().lock(), &mut io::stdout().lock()) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("xrack-lsp: {error}");
            ExitCode::FAILURE
        }
    }
}

fn serve(input: &mut impl io::BufRead, output: &mut impl io::Write) -> io::Result<ExitCode> {
    let mut documents: HashMap<String, Document> = HashMap::new();
    let mut initialized = false;
    let mut shutdown = false;
    while let Some(message) = protocol::read_message(input)? {
        let Some(method) = message["method"].as_str() else {
            continue;
        };
        let params = &message["params"];
        let text_document = &params["textDocument"];
        let uri = text_document["uri"].as_str();

        if method == "exit" {
            return Ok(if shutdown {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        if let Some(id) = message.get("id") {
            let result = match method {
                _ if shutdown => Err((-32600, "server has shut down")),
                "initialize" if !initialized => {
                    initialized = true;
                    Ok(json!({
                        "capabilities": {
                            "positionEncoding": "utf-16",
                            "textDocumentSync": { "openClose": true, "change": 1 },
                            "documentSymbolProvider": true,
                            "definitionProvider": true,
                            "hoverProvider": true,
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
                    Ok(Value::Null)
                }
                "textDocument/documentSymbol" => match uri {
                    Some(uri) => Ok(json!(
                        documents.get(uri).map_or(&[][..], |doc| &doc.symbols)
                    )),
                    None => Err((-32602, "missing textDocument URI")),
                },
                "textDocument/definition"
                | "textDocument/hover"
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
                                    .map(|(uri, doc)| (uri.as_str(), doc.text.as_str())),
                            );
                            completion::query(method, uri, &doc.text, position, workspace.as_ref())
                        })),
                        None => Err((-32602, "missing textDocument URI")),
                    }
                }
                "textDocument/completion" => {
                    let position = &params["position"];
                    match (
                        uri,
                        position["line"].as_u64(),
                        position["character"].as_u64(),
                    ) {
                        (Some(uri), Some(line), Some(character)) => {
                            Ok(json!(documents.get(uri).map_or_else(Vec::new, |doc| {
                                let workspace = completion::Workspace::new(
                                    uri,
                                    documents
                                        .iter()
                                        .map(|(uri, doc)| (uri.as_str(), doc.text.as_str())),
                                );
                                completion::complete(
                                    &doc.text,
                                    line as usize,
                                    character as usize,
                                    workspace.as_ref(),
                                )
                            })))
                        }
                        _ => Err((-32602, "missing document or position")),
                    }
                }
                _ => Err((-32601, "method not found")),
            };
            let response = match result {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err((code, message)) => json!({
                    "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message },
                }),
            };
            protocol::write_message(output, &response)?;
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
                    let doc = publish_analysis(output, uri, version, text)?;
                    documents.insert(uri.into(), doc);
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
                    *document = publish_analysis(output, uri, version, text)?;
                }
            }
            "textDocument/didClose" => {
                if documents.remove(uri).is_some() {
                    publish_diagnostics(output, json!({ "uri": uri, "diagnostics": [] }))?;
                }
            }
            _ => {}
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn publish_analysis(
    output: &mut impl io::Write,
    uri: &str,
    version: i64,
    text: &str,
) -> io::Result<Document> {
    let analysis = analysis::analyze(uri, text);
    publish_diagnostics(
        output,
        json!({
            "uri": uri, "version": version, "diagnostics": analysis.diagnostics,
        }),
    )?;
    Ok(Document {
        version,
        text: text.to_owned(),
        symbols: analysis.symbols,
    })
}

fn publish_diagnostics(output: &mut impl io::Write, params: Value) -> io::Result<()> {
    protocol::write_message(
        output,
        &json!({
            "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params,
        }),
    )
}
