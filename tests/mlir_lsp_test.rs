use runfiles::Runfiles;
use serde_json::{Value, json};
use std::{
    env,
    io::{BufRead, Cursor, Read, Write},
    process::{Command, Stdio},
};
use xrack_compiler::{lower, mlir, parse};

const URI: &str = "file:///xrack-test/generated.mlir";
const SOURCE: &str = r#"component Child inherits Rectangle {
    in-out property <int> count: 0;
    callback bump();
}
component Host inherits Window {
    in-out property <int> count: 1;
    out property <int> observed: item.count + root.count;
    item := Child { count: root.count; }
    field := LineEdit {}
    button := Button {
        clicked => { item.bump(); root.count += 1; field.focus(); }
    }
}"#;

fn message(method: &str, params: Value, id: Option<u32>) -> Value {
    let mut message = json!({"jsonrpc": "2.0", "method": method, "params": params});
    if let Some(id) = id {
        message["id"] = json!(id);
    }
    message
}

fn position(text: &str, operation: &str, attribute: &str) -> Value {
    let (line, text) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(operation) && line.contains(attribute))
        .unwrap_or_else(|| panic!("missing {operation} with {attribute}"));
    let start = text.find(attribute).unwrap() + attribute.find('"').unwrap() + 1;
    json!({"line": line, "character": start})
}

fn at(method: &str, id: u32, position: &Value, include_declaration: bool) -> Value {
    message(
        method,
        json!({"textDocument": {"uri": URI}, "position": position,
               "context": {"includeDeclaration": include_declaration}}),
        Some(id),
    )
}

fn change(version: u32, text: &str) -> Value {
    message(
        "textDocument/didChange",
        json!({"textDocument": {"uri": URI, "version": version},
               "contentChanges": [{"text": text}]}),
        None,
    )
}

fn exchange(mut messages: Vec<Value>) -> Vec<Value> {
    messages.insert(
        0,
        message(
            "initialize",
            json!({"capabilities": {"textDocument": {
                "documentSymbol": {"hierarchicalDocumentSymbolSupport": true}
            }}}),
            Some(0),
        ),
    );
    messages.push(message("shutdown", Value::Null, Some(99)));
    messages.push(message("exit", Value::Null, None));
    let executable = Runfiles::create()
        .unwrap()
        .rlocation(env::var("XRACK_MLIR_LSP").unwrap())
        .unwrap();
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let sender = std::thread::spawn(move || {
        for message in messages {
            let body = serde_json::to_vec(&message).unwrap();
            write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
            input.write_all(&body).unwrap();
        }
    });
    let output = child.wait_with_output().unwrap();
    sender.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut response = Cursor::new(output.stdout);
    let mut messages = Vec::new();
    loop {
        let mut line = String::new();
        if response.read_line(&mut line).unwrap() == 0 {
            return messages;
        }
        let length = line.split_once(':').unwrap().1.trim().parse().unwrap();
        line.clear();
        assert_eq!(response.read_line(&mut line).unwrap(), 2);
        let mut body = vec![0; length];
        response.read_exact(&mut body).unwrap();
        messages.push(serde_json::from_slice(&body).unwrap());
    }
}

fn hierarchies(symbols: &[Value]) -> Vec<&str> {
    symbols
        .iter()
        .flat_map(|symbol| {
            let children = symbol["children"].as_array().map_or(&[][..], Vec::as_slice);
            std::iter::once(symbol["name"].as_str().unwrap()).chain(hierarchies(children))
        })
        .collect()
}

#[test]
fn generated_mlir_supports_scoped_navigation_and_live_module_diagnostics() {
    let original_source = env::current_dir().unwrap().join("original.xr");
    let original_source = original_source.to_str().unwrap();
    let document = parse(SOURCE).unwrap();
    let raw = mlir::lower(&document, original_source).unwrap();
    let optimized = lower(&document, original_source).unwrap().to_mlir();
    for text in [raw, optimized] {
        let read = position(&text, "\"xr.read\"", "owner = \"item\"");
        let mut member = read.clone();
        let line = text
            .lines()
            .nth(read["line"].as_u64().unwrap() as usize)
            .unwrap();
        member["character"] = json!(line.find("value = \"count\"").unwrap() + 9);
        let invoke_owner = position(&text, "\"xr.invoke\"", "owner = \"item\"");
        let callback = position(&text, "\"xr.invoke\"", "name = \"bump\"");
        let callback_decl = position(&text, "\"xr.callback\"", "name = \"bump\"");
        let property_decl = position(&text, "\"xr.property\"", "name = \"count\"");
        let instance_decl = position(&text, "\"xr.instance\"", "id = \"item\"");
        let element_decl = position(&text, "\"xr.element\"", "id = \"field\"");
        let focus_target = position(&text, "\"xr.focus_command\"", "target = \"field\"");
        let component_decl = position(&text, "\"xr.component\"", "sym_name = \"Child\"");
        let invalid = text.replacen("component = @Child", "component = @Missing", 1);
        assert_ne!(invalid, text);
        let invalid_decl = position(&invalid, "\"xr.instance\"", "id = \"item\"");
        let messages = exchange(vec![
            message(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": URI, "languageId": "mlir",
                                         "text": text, "version": 1}}),
                None,
            ),
            at("textDocument/definition", 1, &member, false),
            at("textDocument/definition", 2, &read, false),
            at("textDocument/definition", 3, &callback, false),
            at("textDocument/references", 4, &instance_decl, false),
            at("textDocument/references", 5, &callback_decl, true),
            at("textDocument/definition", 8, &focus_target, false),
            at("textDocument/references", 9, &component_decl, false),
            message(
                "textDocument/documentSymbol",
                json!({"textDocument": {"uri": URI}}),
                Some(6),
            ),
            change(2, &invalid),
            at("textDocument/definition", 7, &read, false),
            change(3, &text),
        ]);
        let result = |id| {
            &messages
                .iter()
                .find(|message| message["id"] == json!(id))
                .unwrap()["result"]
        };
        let location = |start| json!({"uri": URI, "range": {"start": start}});
        let assert_start = |actual: &Value, expected: &Value| {
            assert_eq!(actual["uri"], expected["uri"]);
            assert_eq!(actual["range"]["start"], expected["range"]["start"]);
        };
        for (id, declaration) in [
            (1, &property_decl),
            (2, &instance_decl),
            (3, &callback_decl),
            (7, &invalid_decl),
            (8, &element_decl),
        ] {
            assert_start(&result(id)[0], &location(declaration));
            assert!(
                result(id)[1]["uri"]
                    .as_str()
                    .is_some_and(|uri| uri.ends_with("/original.xr"))
            );
        }
        let owner_uses = result(4).as_array().unwrap();
        assert_eq!(owner_uses.len(), 2);
        for (actual, expected) in owner_uses.iter().zip([&read, &invoke_owner]) {
            assert_start(actual, &location(expected));
        }
        assert_eq!(result(5).as_array().unwrap().len(), 2);
        assert_start(&result(5)[0], &location(&callback_decl));
        assert_start(&result(5)[1], &location(&callback));
        let component_uses = result(9).as_array().unwrap();
        assert_eq!(component_uses.len(), 1);
        assert_eq!(
            component_uses[0]["range"]["start"]["line"],
            instance_decl["line"]
        );

        let names = hierarchies(result(6).as_array().unwrap());
        for name in [
            "Child", "Host", "count", "observed", "bump", "item", "button",
        ] {
            assert!(names.contains(&name), "{names:?}");
        }
        let diagnostics: Vec<_> = messages
            .iter()
            .filter(|message| message["method"] == "textDocument/publishDiagnostics")
            .map(|message| &message["params"]["diagnostics"])
            .collect();
        assert_eq!(diagnostics.len(), 3);
        assert_eq!(*diagnostics[0], json!([]));
        let diagnostic = &diagnostics[1][0];
        assert!(
            diagnostic["message"]
                .as_str()
                .unwrap()
                .contains("unknown component Missing")
        );
        assert_eq!(diagnostic["range"]["start"]["line"], instance_decl["line"]);
        assert!(
            diagnostic["relatedInformation"]
                .as_array()
                .unwrap()
                .iter()
                .any(|note| note["location"]["uri"]
                    .as_str()
                    .unwrap()
                    .ends_with("/original.xr"))
        );
        assert_eq!(*diagnostics[2], json!([]));
    }
}
