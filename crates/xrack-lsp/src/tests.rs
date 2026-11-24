use super::*;

const URI: &str = "file:///demo.xr";

#[test]
fn edit_buffer_updates_diagnostics_and_outline_over_stdio() {
    let bad =
        "export component Demo inherits Window {\r\n    Text { text: \"中🙂\" + missing; }\r\n}";
    let good = bad.replace(" + missing", "").replace("Demo", "Renamed");
    let mut input = Vec::new();
    let request = |id: i32, method: &str, params: Value| json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let notify = |method: &str, params: Value| json!({ "jsonrpc": "2.0", "method": method, "params": params });
    let change = |version: i32, text: &str| {
        notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": URI, "version": version }, "contentChanges": [{ "text": text }],
            }),
        )
    };
    let document = json!({ "textDocument": { "uri": URI } });
    for message in [
        request(1, "initialize", json!({})),
        notify("initialized", json!({})),
        notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": URI, "languageId": "xrack", "version": 1, "text": bad,
            }}),
        ),
        change(2, &good),
        change(1, bad),
        request(2, "textDocument/documentSymbol", document.clone()),
        notify("textDocument/didClose", document.clone()),
        request(3, "textDocument/documentSymbol", document),
        request(4, "shutdown", Value::Null),
        notify("exit", Value::Null),
    ] {
        protocol::write_message(&mut input, &message).unwrap();
    }

    let mut output = Vec::new();
    assert_eq!(serve_bytes(&input, &mut output).unwrap(), ExitCode::SUCCESS);
    let mut output = output.as_slice();
    let mut messages = Vec::new();
    while let Some(message) = protocol::read_message(&mut output).unwrap() {
        messages.push(message);
    }

    assert_eq!(messages.len(), 7);
    assert_eq!(
        messages[0]["result"]["capabilities"]["textDocumentSync"]["change"],
        1
    );
    let capabilities = &messages[0]["result"]["capabilities"];
    assert_eq!(capabilities["definitionProvider"], true);
    assert_eq!(capabilities["hoverProvider"], true);
    assert_eq!(capabilities["documentHighlightProvider"], true);
    assert_eq!(
        capabilities["semanticTokensProvider"]["legend"],
        completion::legend()
    );
    let problem = &messages[1]["params"]["diagnostics"][0];
    let column = "    Text { text: \"中🙂\" + ".encode_utf16().count();
    assert_eq!(problem["code"], "semantic");
    assert!(problem["message"].as_str().unwrap().contains("missing"));
    assert_eq!(
        problem["range"],
        json!({
            "start": { "line": 1, "character": column },
            "end": { "line": 1, "character": column + 7 },
        })
    );
    assert_eq!(
        messages[2]["params"],
        json!({ "uri": URI, "version": 2, "diagnostics": [] })
    );
    assert_eq!(messages[3]["result"][0]["name"], "Renamed");
    assert_eq!(
        messages[4]["params"],
        json!({ "uri": URI, "diagnostics": [] })
    );
    assert_eq!(messages[5]["result"], json!([]));
    assert_eq!(messages[6]["result"], Value::Null);
}

#[test]
fn incomplete_source_and_imports_follow_the_frontend_boundaries() {
    let incomplete = analysis::analyze(URI, "component Demo inherits Window {\n", None);
    assert_eq!(incomplete.diagnostics[URI][0]["code"], "syntax");
    assert!(incomplete.symbols.is_empty());

    let imported = analysis::analyze(
        URI,
        concat!(
            "import { Card } from \"widgets.xr\";\n",
            "export /* 中文 */ component\nDemo inherits Card {}",
        ),
        None,
    );
    assert_eq!(imported.diagnostics[URI][0]["code"], "semantic");
    assert_eq!(imported.symbols[0]["name"], "Demo");
    assert_eq!(
        imported.symbols[0]["location"]["range"],
        json!({
            "start": { "line": 2, "character": 0 },
            "end": { "line": 2, "character": 4 },
        })
    );
}

fn complete_at(marker: &str) -> Vec<Value> {
    complete_in(marker, None)
}

fn complete_in(marker: &str, workspace: Option<&completion::Workspace<'_>>) -> Vec<Value> {
    let (before, after) = marker.split_once('|').expect("cursor marker");
    let line = before.bytes().filter(|&b| b == b'\n').count();
    let column = before.rsplit('\n').next().unwrap().encode_utf16().count();
    completion::complete(&format!("{before}{after}"), line, column, workspace)
}

#[test]
fn completion_suggests_inherits_after_the_component_name() {
    for marker in [
        "component Demo |",
        "export component Demo inh|",
        "import { Card } from \"widgets.xr\";\nexport component Demo inh|erits Window {}",
    ] {
        let items = complete_at(marker);
        assert!(
            items.iter().any(|item| item["label"] == "inherits"
                && item["kind"] == 14
                && item["textEdit"]["newText"] == "inherits"),
            "missing inherits in {marker:?}: {items:?}"
        );
    }
    assert!(complete_at("global Theme inh|").is_empty());
    let bases = complete_at("export component Demo inherits W|");
    assert!(bases.iter().any(|item| item["label"] == "Window"));
    let import = complete_at("import { Card } fr|");
    assert_eq!(import[0]["label"], "from");
}

#[test]
fn completion_suggests_window_height_while_typing() {
    for marker in [
        "export component Demo inherits Window {\n    h|",
        "export component Demo inherits Window {\n    width: 400px;\n    h|\n}",
        "export component Demo inherits Window {\n    title: \"Demo\"\n    h|\n}",
    ] {
        let items = complete_at(marker);
        assert!(
            items.iter().any(|item| item["label"] == "height"
                && item["kind"] == 10
                && item["detail"] == "length"),
            "missing height in {marker:?}: {items:?}"
        );
    }
    for marker in [
        "component Demo inherits Window { width:\n    h|\n}",
        "component Demo inherits Window { width: (1px +\n    h|\n}",
        "component Demo inherits Window { width: true ? 1px\n    h|\n}",
    ] {
        assert!(
            !complete_at(marker)
                .iter()
                .any(|item| item["label"] == "height"),
            "offered a member inside an expression in {marker:?}"
        );
    }
}

#[test]
fn completion_suggests_timer_after_a_named_instance() {
    for marker in [
        "component Demo inherits Window {\n    endup := |\n}",
        "component Demo inherits Window {\n    endup := Ti|\n}",
        "component Demo inherits Window {\n    title: \"Demo\"\n    endup := Ti|\n}",
    ] {
        let items = complete_at(marker);
        assert!(
            items
                .iter()
                .any(|item| item["label"] == "Timer" && item["kind"] == 7),
            "missing Timer in {marker:?}: {items:?}"
        );
    }
    let items = complete_at("component Demo inherits Window {\n    endup ：= Ti|\n}");
    let timer = items.iter().find(|item| item["label"] == "Timer").unwrap();
    assert_eq!(timer["textEdit"]["newText"], "Timer");
    assert_eq!(
        timer["textEdit"]["range"]["start"],
        json!({"line":1,"character":13})
    );
    assert_eq!(
        timer["additionalTextEdits"],
        json!([{
            "range":{"start":{"line":1,"character":10},"end":{"line":1,"character":11}},
            "newText":":"
        }])
    );
    for marker in [
        "component Demo inherits Ti|",
        "component Demo inherits Window { Ti| }",
        "component Demo inherits Window { Rectangle { endup := Ti| } }",
    ] {
        assert!(
            !complete_at(marker)
                .iter()
                .any(|item| item["label"] == "Timer")
        );
    }
    let body = complete_at("component Demo inherits Window { endup := Timer { | } }");
    for label in ["interval", "repeat", "triggered"] {
        assert!(body.iter().any(|item| item["label"] == label));
    }
    assert!(!body.iter().any(|item| item["label"] == "Text"));
    let derived = complete_at(
        "component Base inherits Window {} component Demo inherits Base { child := Sec| }",
    );
    assert!(
        derived
            .iter()
            .any(|item| item["label"] == "SecondaryWindow")
    );
}

#[test]
fn completion_triggers_for_the_named_instance_operator_over_stdio() {
    let uri = "untitled:timer.xr";
    let text = "component Demo inherits Window { endup := }";
    let invalid_trigger = "component Demo inherits Window { property <bool> active: 1 == }";
    let request = |id, text: &str| {
        let offset = text.rfind('=').unwrap() + 1;
        let (line, character) = analysis::Source::new(text).coordinates(offset);
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/completion","params":{
            "textDocument":{"uri":uri}, "position":{"line":line,"character":character},
            "context":{"triggerKind":2,"triggerCharacter":"="}
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":uri,"version":1,"text":text}
        }}),
        request(2, text),
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
            "textDocument":{"uri":uri,"version":2}, "contentChanges":[{"text":invalid_trigger}]
        }}),
        request(3, invalid_trigger),
    ]);
    let result = |id| &messages.iter().find(|message| message["id"] == id).unwrap()["result"];
    assert!(
        result(1)["capabilities"]["completionProvider"]["triggerCharacters"]
            .as_array()
            .unwrap()
            .contains(&json!("="))
    );
    assert!(
        result(2)
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "Timer")
    );
    assert_eq!(result(3), &json!([]));
}

#[test]
fn tree_completions_expose_queries_and_effectful_expansion_commands() {
    let properties = complete_at("component Demo inherits Window { TreeView { | } }");
    assert!(properties.iter().any(|item| item["label"] == "root-key"));
    let items = complete_at(
        "component Demo inherits Window { tree := TreeView {} Button { clicked => { tree.| } } }",
    );
    for method in [
        "expand",
        "collapse",
        "toggle-expanded",
        "expand-recursively",
        "collapse-recursively",
        "expand-to",
        "is-expanded",
        "depth",
        "row-for-key",
    ] {
        assert!(
            items.iter().any(|item| item["label"] == method),
            "missing {method}"
        );
    }
}

#[test]
fn stack_page_completions_offer_lifecycle_and_read_only_state() {
    let state = complete_at(concat!(
        "component Demo inherits Window { nav := StackView { home := StackPage {} } ",
        "Text { text: home.| } }",
    ));
    for property in ["active", "stack-index"] {
        assert!(
            state.iter().any(|item| item["label"] == property),
            "{state:?}"
        );
    }
    let members = complete_at(
        "component Demo inherits Window { nav := StackView { home := StackPage { | } } }",
    );
    for event in ["leave-requested", "activated", "deactivated", "removed"] {
        assert!(members.iter().any(|item| item["label"] == event));
    }
    for property in ["active", "stack-index"] {
        assert!(!members.iter().any(|item| item["label"] == property));
    }
}

fn fixture_uri(name: &str) -> String {
    let drive = if cfg!(windows) { "C:/" } else { "" };
    format!("file:///{drive}xrack-completion-fixture/{name}")
}

fn workspace_file_uri(path: &std::path::Path) -> String {
    completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty())
        .unwrap()
        .uri(path)
        .unwrap()
}

#[test]
fn imported_aliases_resolve_exported_members_and_bases_from_open_buffers() {
    let entry = fixture_uri("Main%20View.xr");
    let shared = concat!(
        "import { Foundation as Parent } from \"base.xr\";\n",
        "export component Card inherits Parent {\n",
        "  in property <string> title; property <int> hidden;\n",
        "  out property <int> size; callback activate();\n",
        "  Text { text: root.",
    );
    let base = "export component Foundation inherits Rectangle { in-out property <int> enabled-count; callback reset; }";
    let unrelated = "export component Card inherits Rectangle { in property <int> different; }";
    let buffers = [
        (fixture_uri("Card%20%E4%B8%AD.xr"), shared),
        (fixture_uri("base.xr"), base),
        (fixture_uri("other.xr"), unrelated),
    ];
    let workspace = completion::Workspace::new(
        &entry,
        buffers.iter().map(|(uri, text)| (uri.as_str(), *text)),
    )
    .unwrap();
    let prefix = concat!(
        "import \"fonts/demo.ttc\";\n",
        "import { Card as Tile } from \"Card 中.xr\";\n",
        "import { Card as Other } from \"other.xr\";\n",
    );
    let labels_at = |line: &str| {
        let text = format!("{prefix}{line}");
        completion::complete(
            &text,
            prefix.lines().count(),
            line.encode_utf16().count(),
            Some(&workspace),
        )
        .into_iter()
        .map(|item| item["label"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>()
    };
    let labels = labels_at("component Demo inherits Window { card := Tile {} Text { text: card.");
    for member in ["title", "size", "activate", "enabled-count", "reset"] {
        assert!(
            labels.iter().any(|label| label == member),
            "missing {member}"
        );
    }
    assert!(
        !labels
            .iter()
            .any(|label| matches!(label.as_str(), "hidden" | "different"))
    );

    let inputs = labels_at("component Demo inherits Window { card := Tile { ");
    assert!(inputs.iter().any(|label| label == "title"));
    assert!(inputs.iter().any(|label| label == "enabled-count"));
    assert!(!inputs.iter().any(|label| label == "size"));
    assert!(inputs.iter().any(|label| label == "activate"));

    let derived = labels_at("component Demo inherits Tile { Text { text: root.");
    assert!(derived.iter().any(|label| label == "activate"));
    let other = labels_at("component Demo inherits Window { card := Other {} Text { text: card.");
    assert!(other.iter().any(|label| label == "layout-width"));
    assert_eq!(
        other
            .iter()
            .filter(|label| xrack_ir::semantic::geometry_property(label).is_none())
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["different"]
    );
}

#[test]
fn completion_ignores_non_exported_imports_and_terminates_on_inheritance_cycles() {
    let uri = fixture_uri("main.xr");
    let buffers = [
        (
            fixture_uri("a.xr"),
            concat!(
                "import { B } from \"b.xr\"; ",
                "export component A inherits B { in property <int> first; } ",
                "component Hidden inherits Rectangle { in property <int> secret; }",
            ),
        ),
        (
            fixture_uri("b.xr"),
            "import { A } from \"a.xr\"; export component B inherits A { callback call; }",
        ),
    ];
    let workspace = completion::Workspace::new(
        &uri,
        buffers.iter().map(|(uri, text)| (uri.as_str(), *text)),
    )
    .unwrap();
    let prefix = "import { A as Visible, Hidden } from \"a.xr\";\n";
    let complete = |kind| {
        let line =
            format!("component Demo inherits Window {{ node := {kind} {{}} Text {{ text: node.");
        completion::complete(&format!("{prefix}{line}"), 1, line.len(), Some(&workspace))
    };
    assert_eq!(
        complete("Visible")
            .iter()
            .map(|item| item["label"].as_str().unwrap())
            .filter(|label| xrack_ir::semantic::geometry_property(label).is_none())
            .collect::<Vec<_>>(),
        ["first", "call"]
    );
    assert!(complete("Hidden").is_empty());
}

#[test]
fn named_import_completion_offers_unused_exports_from_the_open_module() {
    let entry = fixture_uri("main.xr");
    let module = fixture_uri("widgets.xr");
    let source = concat!(
        "export component Card inherits Rectangle {}\n",
        "export struct CardData { title: string }\n",
        "component CardPrivate inherits Rectangle {}\n",
        "struct CardInternal { id: int }",
    );
    let workspace =
        completion::Workspace::new(&entry, [(module.as_str(), source)].into_iter()).unwrap();
    let complete = |text| complete_in(text, Some(&workspace));
    let items =
        complete("import \"demo.ttc\";\nimport { Card as Tile, Ca|rdData } from \"widgets.xr\";");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "CardData");
    assert_eq!(items[0]["kind"], 22);
    assert_eq!(
        items[0]["textEdit"]["range"],
        json!({
            "start": { "line": 1, "character": 23 },
            "end": { "line": 1, "character": 31 },
        })
    );
    let items = complete("import { Ca| } from \"widgets.xr\"");
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Card", "CardData"]
    );
    assert!(complete("import { Card as Ca| } from \"widgets.xr\";").is_empty());
}

#[test]
fn packaged_imports_complete_symbols_and_open_the_packaged_source() {
    let root = workspace_fixture("module-package");
    let folder = root.join("package");
    std::fs::create_dir(&folder).unwrap();
    let entry = folder.join("library.xr");
    std::fs::write(&entry, "export component Panel inherits Rectangle {}").unwrap();
    std::fs::write(
        folder.join("xrack-module.json"),
        json!({
            "version": 1, "entry": "library.xr", "mlir": "library.xr.mlir",
            "rust": "module.rs", "components": ["Panel"], "inputs": ["library.xr"],
        })
        .to_string(),
    )
    .unwrap();
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let uri = workspace.uri(&root.join("app.xr")).unwrap();
    let target = workspace.uri(&entry).unwrap();
    let workspace = completion::Workspace::new(
        &uri,
        [(
            target.as_str(),
            "export component PendingPanel inherits Rectangle {}",
        )]
        .into_iter(),
    )
    .unwrap();
    let text = r#"import { PendingPanel } from "package/xrack-module.json";"#;
    let items = complete_in(
        r#"import { Pend| } from "package/xrack-module.json";"#,
        Some(&workspace),
    );
    assert!(items.iter().any(|item| item["label"] == "PendingPanel"));
    let position = text.find("xrack-module.json").unwrap() + 1;
    assert_eq!(
        completion::import_definition(text, (0, position), &workspace).unwrap()["uri"],
        target
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeat_record_fields_and_indices_use_the_declared_model_types() {
    let source = concat!(
        "struct Address { city: string }\n",
        "struct Person { address: Address, age: int }\n",
        "component App inherits Window { in property <[Person]> people;\n",
        "  for item[index] in root.people : VerticalLayout { Text { text: "
    );
    let labels = |suffix| complete_at(&format!("{source}{suffix}"));
    assert!(
        labels("item.address.ci|")
            .iter()
            .any(|item| item["label"] == "city" && item["detail"] == "string")
    );
    assert!(
        labels("ind|")
            .iter()
            .any(|item| item["label"] == "index" && item["detail"] == "int")
    );
    assert!(
        complete_at(
            "component App inherits Window { in property <bool> mobile; states [ small when mo|"
        )
        .iter()
        .any(|item| item["label"] == "mobile")
    );
}

#[test]
fn completion_resolves_editing_scopes_and_replaces_a_partial_name() {
    let items = complete_at(concat!(
        "component Other inherits Rectangle { property <int> hidden: 0; }\n",
        "export component Demo inherits Window {\n",
        "  label := Text { text: \"🙂\" + root.co|unt; }\n",
        "  in-out property <int> count: 0;\n",
        "}",
    ));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "count");
    assert_eq!(items[0]["detail"], "int");
    let start = "  label := Text { text: \"🙂\" + root."
        .encode_utf16()
        .count();
    assert_eq!(
        items[0]["textEdit"]["range"],
        json!({
            "start": { "line": 2, "character": start },
            "end": { "line": 2, "character": start + 5 },
        })
    );

    let items = complete_at(concat!(
        "struct Person { age: int, name: string }\n",
        "component Child inherits Rectangle { out property <int> score: 0; property <int> secret: 1; }\n",
        "component Demo inherits Window { property <Person> person; child := Child {}\n",
        "  Text { text: root.person.|",
    ));
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "age" && item["kind"] == 5)
    );
    assert!(items.iter().any(|item| item["label"] == "name"));

    let items = complete_at(concat!(
        "component Child inherits Rectangle { out property <int> score: 0; property <int> secret: 1; }\n",
        "component Demo inherits Window { child := Child {} Text { text: child.|",
    ));
    assert!(items.iter().any(|item| item["label"] == "score"));
    assert!(!items.iter().any(|item| item["label"] == "secret"));
}

#[test]
fn completion_offers_contextual_properties_types_and_events() {
    for (source, expected) in [
        (
            "component Demo inherits Window { Slider { ori|",
            "orientation",
        ),
        (
            "component Demo inherits Window { Slider { orientation: ver|",
            "vertical",
        ),
        (
            "component Demo inherits Window { ok := Button { cli|",
            "clicked",
        ),
        (
            "component Demo inherits Window { property <Tex|",
            "TextWrap",
        ),
        (
            "component Demo inherits Window { background: @lin|",
            "@linear-gradient",
        ),
        (
            "component Demo inherits Window { Button { clicked => { Log.deb|",
            "debug",
        ),
        (
            "component Demo inherits Window { title: Strings.for|",
            "format",
        ),
        (
            "component Demo inherits Window { Button { clicked => { Log.write(LogLevel.in|",
            "info",
        ),
        (
            "component Demo inherits Window { for item in 3 : Tex|",
            "Text",
        ),
        (
            "import { Card as Tile } from \"ui.xr\"; component Demo inherits Til|",
            "Tile",
        ),
        (
            "component Demo inherits Window { callback log(step: int); log(step) => { ste|",
            "step",
        ),
    ] {
        assert!(
            complete_at(source)
                .iter()
                .any(|item| item["label"] == expected),
            "{source}"
        );
    }
    assert!(complete_at("component Demo inherits Window { Text { row-he| ").is_empty());
    assert!(complete_at("component Demo inherits Window { Text { text: \"root.| ").is_empty());
    assert!(complete_at("component Demo inherits Window { // root.|").is_empty());
}

#[test]
fn table_completions_include_read_only_selection_state_and_commands() {
    let items = complete_at(
        "component Demo inherits Window { table := TableView {} Button { clicked => { table.| } } }",
    );
    for name in [
        "current-column",
        "selection-count",
        "has-selection",
        "select-cell",
        "set-current-cell",
        "select-range",
        "select-all",
        "clear-selection",
    ] {
        assert!(
            items.iter().any(|item| item["label"] == name),
            "missing {name}"
        );
    }
    let bindings = complete_at("component Demo inherits Window { TableView { | } }");
    assert!(
        !bindings
            .iter()
            .any(|item| item["label"] == "current-column")
    );
}

fn request_at(
    method: &str,
    uri: &str,
    marker: &str,
    workspace: Option<&completion::Workspace<'_>>,
) -> Value {
    let (before, after) = marker.split_once('|').unwrap();
    let line = before.bytes().filter(|&byte| byte == b'\n').count();
    let character = before.rsplit('\n').next().unwrap().encode_utf16().count();
    completion::query(
        method,
        uri,
        &format!("{before}{after}"),
        Some((line, character)),
        workspace,
    )
}

fn location_at(uri: &str, marker: &str, name: &str) -> Value {
    let (before, after) = marker.split_once('|').unwrap();
    assert!(after.starts_with(name));
    let text = format!("{before}{after}");
    let range = analysis::Source::new(&text).range(before.len(), before.len() + name.len());
    json!({ "uri": uri, "range": range })
}

#[test]
fn builtin_functions_expose_documentation_and_contextual_completions() {
    const SOURCE: &str = r#"component Demo inherits Window {
        callback log();
        log => {}
        title: Strings.format("{}", 1);
        button := Button {
            clicked => {
                root.log();
                Log.debug(Strings.format("{}", 2));
                Log.write(LogLevel.info, "first");
                Log.write(LogLevel.warn, "second");
            }
        }
    }"#;
    assert!(analysis::analyze(URI, SOURCE, None).diagnostics.is_empty());
    let hover = |from, to| {
        request_at(
            "textDocument/hover",
            URI,
            &SOURCE.replacen(from, to, 1),
            None,
        )["contents"]["value"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(hover("title: Strings.format", "title: Strings.for|mat").contains("sequential"));
    assert!(hover("Log.debug(", "Log.de|bug(").contains("Log.write(LogLevel.debug, message)"));
    assert!(hover("LogLevel.info", "LogLevel.in|fo").contains("selects the info level"));
    assert!(hover("root.log()", "root.lo|g()").contains("callback log"));
    assert_eq!(
        request_at(
            "textDocument/definition",
            URI,
            &SOURCE.replace("root.log()", "root.lo|g()"),
            None
        ),
        location_at(URI, &SOURCE.replace("callback log", "callback |log"), "log"),
    );
    assert!(
        request_at(
            "textDocument/definition",
            URI,
            &SOURCE.replace("Log.write(LogLevel.info", "Log.wr|ite(LogLevel.info"),
            None
        )
        .is_null()
    );
    let highlights = request_at(
        "textDocument/documentHighlight",
        URI,
        &SOURCE.replace("Log.write(LogLevel.info", "Log.wr|ite(LogLevel.info"),
        None,
    );
    assert_eq!(highlights.as_array().unwrap().len(), 2);

    let binding = complete_at("component Demo inherits Window { title: Strings.| }");
    assert!(
        binding.iter().any(|item| item["label"] == "format"
            && item["detail"].as_str().unwrap().ends_with("-> string"))
    );
    assert!(!binding.iter().any(|item| item["label"] == "debug"));
    let argument =
        complete_at("component Demo inherits Window { Button { clicked => { Log.debug(Strings.|");
    assert!(argument.iter().any(|item| item["label"] == "format"));
    assert!(!argument.iter().any(|item| item["label"] == "write"));
    let level =
        complete_at("component Demo inherits Window { Button { clicked => { Log.write(LogLevel.|");
    assert_eq!(level.len(), 5);
}

#[test]
fn global_properties_are_completed_and_resolve_to_their_declarations() {
    let source = "export global Session { in-out property <int> count: 1; private property <int> hidden: 0; } component App inherits Window { out property <int> seen: Session.|; }";
    let items = complete_at(source);
    assert!(items.iter().any(|item| item["label"] == "count"));
    assert!(!items.iter().any(|item| item["label"] == "hidden"));
    let source = source.replace("Session.|", "Session.co|unt");
    let result = request_at("textDocument/definition", URI, &source, None);
    assert_eq!(result["uri"], URI);
    let invalid = source.replace("co|unt", "hidden");
    assert!(
        analysis::analyze(URI, &invalid, None).diagnostics[URI][0]["message"]
            .as_str()
            .unwrap()
            .contains("private")
    );
}

#[test]
fn imported_global_properties_complete_through_an_alias() {
    let root = workspace_fixture("imported-global-completion");
    std::fs::write(root.join("lighting.xr"),
        "export global BakedLightmaps { out property <float> intensity: 1; private property <float> hidden: 0; }"
    ).unwrap();
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let uri = workspace.uri(&root.join("app.xr")).unwrap();
    let text = r#"import { BakedLightmaps as Atlas } from "lighting.xr";
        struct Material { intensity: float }
        component App inherits Window {
            property <Material> material: Material { intensity: Atlas.| };
        }"#;
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let (before, after) = text.split_once('|').unwrap();
    let line = before.bytes().filter(|&byte| byte == b'\n').count();
    let character = before.rsplit('\n').next().unwrap().encode_utf16().count();
    let items = completion::complete(
        &format!("{before}{after}"),
        line,
        character,
        Some(&workspace),
    );
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "intensity" && item["detail"] == "float")
    );
    assert!(!items.iter().any(|item| item["label"] == "hidden"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn list_selection_api_completes_and_has_typed_signatures() {
    let items = complete_at(
        "component App inherits Window { list := GridView {} out property <int> count: list.selection-|",
    );
    assert!(items.iter().any(|item| item["label"] == "selection-count"));
    let items = complete_at(
        "component App inherits Window { list := GridView {} button := Button { clicked => { list.toggle-| } } }",
    );
    assert!(items.iter().any(|item| item["label"] == "toggle-row"));
    let line = "component App inherits Window { list := GridView {} out property <bool> found: list.is-row-selected(";
    let signature = completion::signature_help(line, (0, line.len()), None);
    assert_eq!(
        signature["signatures"][0]["label"],
        "is-row-selected(row: int) -> bool"
    );
}

#[test]
fn tree_selection_api_completes_key_queries_and_commands() {
    let items = complete_at(
        "component App inherits Window { tree := TreeView {} out property <bool> can-clear: tree.|",
    );
    for expected in [
        "selection-count",
        "has-selection",
        "selected-keys",
        "is-key-selected",
        "is-row-selected",
    ] {
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{expected}"
        );
    }
    assert!(!items.iter().any(|item| item["label"] == "select-key"));

    let items = complete_at(
        "component App inherits Window { tree := TreeView {} Button { clicked => { tree.| } } }",
    );
    for expected in [
        "select-key",
        "select-keys",
        "select-row",
        "toggle-row",
        "select-all",
        "clear-selection",
    ] {
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{expected}"
        );
    }
    assert!(!items.iter().any(|item| item["label"] == "select-column"));

    let line = "component App inherits Window { tree := TreeView {} out property <bool> found: tree.is-key-selected(";
    let signature = completion::signature_help(line, (0, line.len()), None);
    assert_eq!(
        signature["signatures"][0]["label"],
        "is-key-selected(key: int) -> bool"
    );
    let line = "component App inherits Window { tree := TreeView {} Button { clicked => { tree.select-keys(";
    let signature = completion::signature_help(line, (0, line.len()), None);
    assert_eq!(
        signature["signatures"][0]["label"],
        "select-keys(keys: [int])"
    );
}

#[test]
fn keyed_list_selection_batch_completes_for_all_list_kinds() {
    for kind in ["ListView", "GridView", "StandardListView"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ view := {kind} {{ row-key-field: \"id\"; }} out property <[int]> saved: view.selected-|"
        ));
        assert!(
            items.iter().any(|item| item["label"] == "selected-keys"),
            "{kind}"
        );
        let line = format!(
            "component App inherits Window {{ view := {kind} {{ row-key-field: \"id\"; }} Button {{ clicked => {{ view.select-keys("
        );
        let signature = completion::signature_help(&line, (0, line.len()), None);
        assert_eq!(
            signature["signatures"][0]["label"], "select-keys(keys: [int])",
            "{kind}"
        );
    }
}

#[test]
fn keyed_tables_offer_stable_selection_and_navigation() {
    for kind in ["TableView", "StandardTableView"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ table := {kind} {{ row-key-field: \"id\"; }} out property <int> row: table.|"
        ));
        for property in ["selected-keys", "view-index-for-key", "current-column"] {
            assert!(
                items.iter().any(|item| item["label"] == property),
                "{kind} {property}"
            );
        }
        assert!(!items.iter().any(|item| item["label"] == "select-key"));
        let line = format!(
            "component App inherits Window {{ table := {kind} {{ row-key-field: \"id\"; }} Button {{ clicked => {{ table.position-view-at-key("
        );
        let signature = completion::signature_help(&line, (0, line.len()), None);
        assert_eq!(
            signature["signatures"][0]["label"],
            "position-view-at-key(key: int, alignment: ScrollAlignment)"
        );
    }
}

#[test]
fn radio_group_completes_read_only_focus_and_commands() {
    let read = complete_at(
        "component App inherits Window { options := RadioGroup {} out property <bool> active: options.| }",
    );
    assert!(
        read.iter()
            .any(|item| item["label"] == "has-focus" && item["detail"] == "read-only bool")
    );
    assert!(!read.iter().any(|item| item["label"] == "focus"));
    let binding = complete_at("component App inherits Window { options := RadioGroup { | } }");
    assert!(!binding.iter().any(|item| item["label"] == "has-focus"));
    let command = complete_at(
        "component App inherits Window { options := RadioGroup {} action := Button { clicked => { options.| } } }",
    );
    for name in ["focus", "clear-focus"] {
        assert!(command.iter().any(|item| item["label"] == name));
    }
    let line = "component App inherits Window { options := RadioGroup {} action := Button { clicked => { options.focus(";
    let signature = completion::signature_help(line, (0, line.len()), None);
    assert_eq!(signature["signatures"][0]["label"], "focus()");
}

#[test]
fn radio_group_completes_options_in_conditional_and_repeated_declarations() {
    for prefix in ["", "if true: ", "for option in 3: ", "option := "] {
        let options = complete_at(&format!(
            "component App inherits Window {{ RadioGroup {{ {prefix}| }} }}"
        ));
        assert!(options.iter().any(|item| item["label"] == "RadioButton"));
        assert!(!options.iter().any(|item| item["label"] == "Button"));
    }
}

#[test]
fn push_button_click_completes_in_callbacks_and_has_a_signature() {
    for kind in ["Button", "ToolButton", "RoundButton"] {
        let prefix = format!(
            "component App inherits Window {{ target := {kind} {{}} trigger := Button {{ clicked => {{ target."
        );
        let items = complete_at(&format!("{prefix}| }} }} }}"));
        for name in ["click", "animate-click", "toggle"] {
            assert!(items.iter().any(|item| item["label"] == name));
            let line = format!("{prefix}{name}(");
            let signature = completion::signature_help(&line, (0, line.len()), None);
            assert_eq!(signature["signatures"][0]["label"], format!("{name}()"));
        }
    }
    let value = complete_at(
        "component App inherits Window { target := Button {} out property <bool> result: target.| }",
    );
    assert!(!value.iter().any(|item| item["label"] == "click"));
    assert!(!value.iter().any(|item| item["label"] == "toggle"));
}

#[test]
fn button_appearance_flags_complete_for_controls_and_visual_states() {
    for kind in [
        "Button",
        "ToolButton",
        "RoundButton",
        "DelayButton",
        "DialogButton",
        "StandardButton",
    ] {
        let members = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ | }} }}"
        ));
        for name in ["primary", "flat"] {
            assert!(
                members
                    .iter()
                    .any(|item| item["label"] == name && item["detail"] == "bool"),
                "{kind}.{name}"
            );
        }
    }
    let state = complete_at(
        "component App inherits Window { Button { states [ normal when true: { fla| } ] } }",
    );
    assert!(state.iter().any(|item| item["label"] == "flat"));
}

#[test]
fn button_display_completes_members_and_enum_values() {
    for kind in ["Button", "ToolButton", "RoundButton", "DelayButton"] {
        let source = format!("component App inherits Window {{ {kind} {{ | }} }}");
        let members = complete_at(&source);
        for property in ["display", "spacing", "colorize-icon", "ButtonContent"] {
            assert!(
                members.iter().any(|item| item["label"] == property),
                "{kind}.{property}"
            );
        }
        let source =
            format!("component App inherits Window {{ {kind} {{ display: ButtonDisplay.| }} }}");
        let values = complete_at(&source);
        for value in [
            "icon-only",
            "text-only",
            "text-beside-icon",
            "text-under-icon",
        ] {
            assert!(
                values.iter().any(|item| item["label"] == value),
                "{kind}.{value}"
            );
        }
    }
}

#[test]
fn scene_viewport_options_have_viewport_specific_types() {
    for (prefix, name, ty) in [
        ("supersam", "supersampling", "float"),
        ("progressive-a", "progressive-aa", "int"),
        ("temporal-a", "temporal-aa", "bool"),
        ("temporal-aa-m", "temporal-aa-mode", "string"),
        ("screen-space-refl", "screen-space-reflections", "bool"),
        ("ssr-st", "ssr-steps", "int"),
        ("ssr-th", "ssr-thickness", "float"),
        (
            "screen-space-global",
            "screen-space-global-illumination",
            "bool",
        ),
        ("ssgi-ra", "ssgi-radius", "float"),
        ("ssgi-ra", "ssgi-rays", "int"),
        ("temporal-aa-s", "temporal-aa-strength", "float"),
        ("texture-w", "texture-width", "int"),
        ("texture-h", "texture-height", "int"),
        ("render-sc", "render-scale", "float"),
        ("layer-ma", "layer-mask", "int"),
        ("cache-rend", "cache-rendering", "bool"),
        ("imported-li", "imported-lights", "bool"),
        ("specular-", "specular-aa", "bool"),
        ("particle-t", "particle-time", "float"),
    ] {
        let members = complete_at(&format!(
            "component App inherits Window {{ SceneView3D {{ {prefix}| }} }}"
        ));
        let member = members
            .iter()
            .find(|item| item["label"] == name)
            .expect("typed SceneView3D property");
        assert_eq!(member["detail"], ty);
        let outside = complete_at(&format!(
            "component App inherits Window {{ Rectangle {{ {prefix}| }} }}"
        ));
        assert!(!outside.iter().any(|item| item["label"] == name));
    }
}

#[test]
fn scene_particle_binding_completes_a_user_defined_record_list() {
    let source = "struct Sparks { position: point3d, size: float, lifetime: float, color: color }
        component App inherits Window {
            property <[Sparks]> emitters;
            SceneView3D { particle-em| }
        }";
    let items = complete_at(source);
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "particle-emitters")
    );
    let outside = complete_at("component App inherits Window { Rectangle { particle-em| } }");
    assert!(
        !outside
            .iter()
            .any(|item| item["label"] == "particle-emitters")
    );
}

#[test]
fn sprite_playback_values_complete_in_emitter_records() {
    let source = r#"struct Emitter {
            position: point3d, size: float, lifetime: float, color: color, sprite-playback: string,
        }
        component App inherits Window {
            property <[Emitter]> emitters: [
                Emitter {
                    position: Point3D.new(0, 0, 0), size: 1, color: #fff, lifetime: 2,
                    sprite-playback: "ping-|",
                },
            ];
            SceneView3D { particle-emitters: root.emitters; }
        }"#;
    let items = complete_at(source);
    assert!(items.iter().any(|item| item["label"] == "ping-pong"));
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "ping-pong-reverse")
    );
}

#[test]
fn model_particle_fields_complete_in_typed_emitter_records() {
    let items = complete_at(
        r#"struct Emitter { particle-model: string, particle-angular-velocity: vector3d }
        component App inherits Window {
            property <Emitter> body: Emitter { particle-angular-v| };
        }"#,
    );
    assert!(
        items.iter().any(
            |item| item["label"] == "particle-angular-velocity" && item["detail"] == "vector3d"
        )
    );
}

#[test]
fn model_particle_lighting_flags_complete_as_booleans() {
    let items = complete_at(
        r#"struct Emitter {
            particle-casts-shadow: bool, particle-casts-reflections: bool, particle-receives-shadow: bool,
        }
        component App inherits Window {
            property <Emitter> body: Emitter { particle-casts-| };
        }"#,
    );
    for field in ["particle-casts-shadow", "particle-casts-reflections"] {
        assert!(
            items
                .iter()
                .any(|item| item["label"] == field && item["detail"] == "bool")
        );
    }
    let items = complete_at(
        r#"struct Emitter { particle-receives-shadow: bool }
        component App inherits Window {
            property <Emitter> body: Emitter { particle-receives-s| };
        }"#,
    );
    assert!(
        items.iter().any(|item| {
            item["label"] == "particle-receives-shadow" && item["detail"] == "bool"
        })
    );
}

#[test]
fn particle_trail_fields_complete_in_typed_emitter_records() {
    let items = complete_at(
        r#"struct Emitter {
            trail-segments: int, trail-duration: float, trail-end-scale: float,
        }
        component App inherits Window {
            property <Emitter> tail: Emitter { trail-seg| };
        }"#,
    );
    let segment = items.iter().find(|item| item["label"] == "trail-segments");
    let segment = segment.expect("typed segment count completion");
    assert_eq!(segment["detail"], "int");
}

#[test]
fn particle_affectors_complete_list_and_typed_record_kinds() {
    let members = complete_at("component App inherits Window { SceneView3D { particle-af| } }");
    assert!(
        members
            .iter()
            .any(|item| item["label"] == "particle-affectors")
    );
    let outside = complete_at("component App inherits Window { Rectangle { particle-af| } }");
    assert!(
        !outside
            .iter()
            .any(|item| item["label"] == "particle-affectors")
    );
    let kinds = complete_at(
        r#"struct Field { affector-kind: string }
           component App inherits Window {
               property <Field> effect: Field { affector-kind: "point-|"; };
           }"#,
    );
    assert!(kinds.iter().any(|item| item["label"] == "point-rotator"));
}

#[test]
fn particle_blend_values_complete_in_emitter_records() {
    let items = complete_at(
        r#"struct Emitter { particle-blend-mode: string }
           component App inherits Window {
               property <Emitter> effect: Emitter { particle-blend-mode: "mu|"; };
           }"#,
    );
    assert!(items.iter().any(|item| item["label"] == "multiply"));
}

#[test]
fn particle_shape_and_direction_complete_in_emitter_records() {
    for (field, prefix, expected) in [
        ("emitter-shape", "cy", "cylinder"),
        ("emitter-shape", "me", "mesh"),
        ("velocity-mode", "ta", "target"),
        ("velocity-mode", "norm", "normalized-vector"),
        ("velocity-mode", "sur", "surface-normal"),
    ] {
        let source = format!(
            "struct Emitter {{ {field}: string }} component App inherits Window {{
                property <Emitter> effect: Emitter {{ {field}: \"{prefix}|\"; }};
            }}"
        );
        assert!(
            complete_at(&source)
                .iter()
                .any(|item| item["label"] == expected)
        );
    }
}

#[test]
fn lightmap_modes_complete_in_model_records() {
    let source = r#"struct Object { lightmap: image, lightmap-mode: string }
        component App inherits Window {
            property <Object> item: Object { lightmap-mode: "ind|"; };
        }"#;
    let items = complete_at(source);
    assert!(items.iter().any(|item| item["label"] == "indirect"));
}

#[test]
fn temporal_modes_complete_in_scene_viewport_string_values() {
    let items = complete_at(
        "component App inherits Window { SceneView3D { temporal-aa-mode: \"motion-|\"; } }",
    );
    assert!(items.iter().any(|item| item["label"] == "motion-vectors"));
    let outside = complete_at(
        "component App inherits Window { Rectangle { temporal-aa-mode: \"motion-|\"; } }",
    );
    assert!(!outside.iter().any(|item| item["label"] == "motion-vectors"));
}

#[test]
fn reflection_probe_refresh_modes_complete_inside_record_values() {
    let source = "struct Probe { refresh-mode: string } component App inherits Window {
        property <Probe> probe: Probe { refresh-mode: \"first-f|\" };
    }";
    let items = complete_at(source);
    assert!(items.iter().any(|item| {
        item["label"] == "first-frame"
            && item["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("capture-revision"))
    }));
    let prefix = source.replace("first-f|", "on-ch|");
    assert!(
        complete_at(&prefix)
            .iter()
            .any(|item| item["label"] == "on-change")
    );
}

#[test]
fn reflection_probe_time_slicing_completes_inside_probe_records() {
    let source = "struct Probe { time-slicing: string } component App inherits Window {
        property <Probe> probe: Probe { time-slicing: \"indiv|\" };
    }";
    assert!(
        complete_at(source)
            .iter()
            .any(|item| item["label"] == "individual-faces")
    );
    assert!(
        complete_at(&source.replace("indiv|", "all-f|"))
            .iter()
            .any(|item| item["label"] == "all-faces-at-once")
    );
}

#[test]
fn scene_tone_map_completes_typed_curves_and_color_adjustments() {
    let members = complete_at("component App inherits Window { SceneView3D { scene-| } }");
    for expected in [
        "scene-tone-map",
        "scene-white-point",
        "scene-brightness",
        "scene-contrast",
        "scene-saturation",
    ] {
        assert!(
            members.iter().any(|item| item["label"] == expected),
            "{expected}"
        );
    }
    let values = complete_at(
        "component App inherits Window { SceneView3D { scene-tone-map: SceneToneMap.| } }",
    );
    for expected in [
        "none",
        "linear",
        "reinhard",
        "aces",
        "hejl-dawson",
        "filmic",
    ] {
        assert!(
            values.iter().any(|item| item["label"] == expected),
            "{expected}"
        );
    }
}

#[test]
fn push_button_group_completion_finds_component_local_groups() {
    for kind in ["Button", "ToolButton", "RoundButton"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ choices := ButtonGroup {{}} flags := ButtonGroup {{ exclusive: false; }} command := Action {{}} picker := {kind} {{ group: | }} }}"
        ));
        for name in ["choices", "flags"] {
            assert!(
                items.iter().any(|item| item["label"] == name),
                "{kind}.{name}"
            );
        }
        assert!(!items.iter().any(|item| item["label"] == "command"));
    }
}

#[test]
fn push_buttons_complete_checkable_state_and_toggle_handler() {
    for kind in ["Button", "ToolButton", "RoundButton"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ pin := {kind} {{ | }} }}"
        ));
        for name in ["checkable", "checked", "toggled"] {
            assert!(
                items.iter().any(|item| item["label"] == name),
                "{kind}.{name}"
            );
        }
    }
}

#[test]
fn push_buttons_complete_repeat_timing_bindings() {
    for kind in ["Button", "RoundButton", "ToolButton"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ auto-re|"
        ));
        for name in ["auto-repeat", "auto-repeat-delay", "auto-repeat-interval"] {
            assert!(
                items.iter().any(|item| item["label"] == name),
                "{kind} {name}"
            );
        }
    }
    let options = complete_at("component App inherits Window { CheckBox { auto-re|");
    assert!(
        !options
            .iter()
            .any(|item| item["label"] == "auto-repeat-interval")
    );
}

#[test]
fn push_buttons_complete_press_and_hold_handler_and_interval() {
    for kind in ["Button", "RoundButton", "ToolButton", "TouchArea"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ press-and-|"
        ));
        for name in ["press-and-hold", "press-and-hold-interval"] {
            assert!(
                items.iter().any(|item| item["label"] == name),
                "{kind} {name}"
            );
        }
    }
    let options = complete_at("component App inherits Window { CheckBox { press-and-|");
    assert!(!options.iter().any(|item| item["label"] == "press-and-hold"));
}

#[test]
fn double_click_completion_includes_push_buttons_and_touch_areas() {
    for kind in ["Button", "RoundButton", "ToolButton", "TouchArea"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ target := {kind} {{ double-cl|"
        ));
        assert!(
            items.iter().any(|item| item["label"] == "double-clicked"),
            "{kind}"
        );
    }
    let items = complete_at("component App inherits Window { target := CheckBox { double-cl|");
    assert!(!items.iter().any(|item| item["label"] == "double-clicked"));
}

#[test]
fn button_pressed_completes_as_a_read_only_binding() {
    let reads = complete_at(
        "component App inherits Window { button := Button {} out property <bool> down: button.pre|",
    );
    assert!(reads.iter().any(|item| item["label"] == "pressed"));
    let writes = complete_at("component App inherits Window { button := Button { pre|");
    assert!(
        !writes
            .iter()
            .any(|item| item["label"] == "pressed" && item["kind"] == 10)
    );
}

#[test]
fn button_checked_completes_in_an_expression_and_as_a_binding() {
    for kind in ["Button", "RoundButton", "ToolButton"] {
        let reads = complete_at(&format!(
            "component App inherits Window {{ button := {kind} {{}} out property <bool> selected: button.che|"
        ));
        assert!(
            reads.iter().any(|item| item["label"] == "checked"),
            "{kind}"
        );
        let bindings = complete_at(&format!("component App inherits Window {{ {kind} {{ che|"));
        assert!(
            bindings.iter().any(|item| item["label"] == "checked"),
            "{kind}"
        );
    }
    let rows = complete_at(
        "component App inherits Window { for label in [\"item\"]: RoundButton { checkable: true; text: self.che|",
    );
    assert!(
        rows.iter().any(|item| item["label"] == "checked"),
        "{rows:?}"
    );
}

#[test]
fn range_controls_complete_normalized_positional_reads() {
    for (kind, prefix, property) in [
        ("Slider", "visual-p", "visual-position"),
        ("Dial", "pos", "position"),
        ("RangeSlider", "upper-visual-p", "upper-visual-position"),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ slider := {kind} {{}} Label {{ text: slider.{prefix}|"
        ));
        assert!(
            items.iter().any(|item| item["label"] == property
                && item["detail"] == "read-only float · normalized position"),
            "{items:?}"
        );
    }
}

#[test]
fn numeric_controls_offer_typed_committed_values() {
    for (kind, prefix, property, ty) in [
        ("Slider", "val", "value", "float"),
        ("Dial", "val", "value", "float"),
        ("SpinBox", "val", "value", "int"),
        ("RangeSlider", "upper-v", "upper-value", "float"),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ editor := {kind} {{}} Label {{ text: editor.{prefix}|"
        ));
        assert!(
            items.iter().any(
                |item| item["label"] == property && item["detail"] == format!("committed {ty}")
            ),
            "{items:?}"
        );
    }
}

#[test]
fn navigation_items_complete_actual_selection_state() {
    for (source, property) in [
        (
            "component App inherits Window { TabBar { for label in labels: TabButton { text: self.che|",
            "checked",
        ),
        (
            "component App inherits Window { TabWidget { page := Tab {} } Label { text: page.ac|",
            "active",
        ),
        (
            "component App inherits Window { SwipeView { page := SwipePage {} } Label { text: page.ac|",
            "active",
        ),
    ] {
        let items = complete_at(source);
        assert!(
            items.iter().any(|item| item["label"] == property),
            "{source}: {items:?}"
        );
    }
}

#[test]
fn radio_options_offer_checked_from_their_group_or_delegate() {
    for source in [
        "component App inherits Window { RadioGroup { choice := RadioButton {} } Label { text: choice.che|",
        "component App inherits Window { RadioGroup { for name in names: RadioButton { color: self.che|",
        "component App inherits Window { ListView { for item in rows: RadioDelegate { background: self.che|",
    ] {
        let items = complete_at(source);
        assert!(
            items.iter().any(|item| item["label"] == "checked"),
            "{source}: {items:?}"
        );
    }
}

#[test]
fn toggle_states_complete_with_their_effective_property_types() {
    for (control, property, ty) in [
        ("CheckBox", "check-state", "checkstate"),
        ("CheckBox", "checked", "bool"),
        ("Switch", "checked", "bool"),
    ] {
        let source = format!(
            "component App inherits Window {{ toggle := {control} {{}} Label {{ text: toggle.che|"
        );
        let items = complete_at(&source);
        assert!(
            items.iter().any(
                |item| item["label"] == property && item["detail"] == format!("effective {ty}")
            ),
            "{items:?}"
        );
    }
    let rows = complete_at(
        "component App inherits Window { ListView { for row in rows: CheckDelegate { toggled => { self.che|",
    );
    assert!(
        rows.iter().any(|item| item["label"] == "check-state"),
        "{rows:?}"
    );
}

#[test]
fn repeated_button_handlers_complete_toggle_and_resolve_its_signature() {
    let text = "component App inherits Window { for item in 3: Button { checkable: false; clicked => { self.";
    let items = complete_at(&format!("{text}|"));
    assert!(
        items.iter().any(|item| item["label"] == "toggle"),
        "{items:?}"
    );
    assert!(
        !items
            .iter()
            .any(|item| matches!(item["label"].as_str(), Some("click" | "animate-click")))
    );
    let call = format!("{text}toggle(");
    let signature = completion::signature_help(&call, (0, call.len()), None);
    assert_eq!(signature["signatures"][0]["label"], "toggle()");
}

#[test]
fn virtual_lists_offer_multi_select_and_the_selection_event() {
    for kind in ["ListView", "GridView", "StandardListView"] {
        let items = complete_at(&format!("component App inherits Window {{ {kind} {{ |"));
        for name in ["multi-select", "selection-changed"] {
            assert!(
                items.iter().any(|item| item["label"] == name),
                "{kind} {name}"
            );
        }
    }
}

#[test]
fn typeahead_properties_complete_on_lists_and_grids() {
    for kind in ["ListView", "StandardListView", "GridView"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ typeahead-|"
        ));
        for property in ["typeahead-field", "typeahead-enabled"] {
            assert!(
                items.iter().any(|item| item["label"] == property),
                "{kind} {property}"
            );
        }
    }
    let items = complete_at("component App inherits Window { TreeView { typeahead-|");
    assert!(!items.iter().any(|item| item["label"] == "typeahead-field"));
}

#[test]
fn list_section_properties_complete_for_both_list_views() {
    for kind in ["ListView", "StandardListView"] {
        let items = complete_at(&format!("component App inherits Window {{ {kind} {{ |"));
        for property in ["section-field", "section-height", "sticky-section-headers"] {
            assert!(
                items.iter().any(|item| item["label"] == property),
                "{kind} {property}"
            );
        }
    }
    let grid = complete_at("component App inherits Window { GridView { |");
    assert!(!grid.iter().any(|item| item["label"] == "section-field"));
}

#[test]
fn section_header_parameters_complete_inside_the_visual_template() {
    for kind in ["ListView", "StandardListView"] {
        let items = complete_at(&format!("component App inherits Window {{ {kind} {{ Sect|"));
        assert!(items.iter().any(|item| item["label"] == "SectionHeader"));
    }
    let inside = complete_at(
        "component App inherits Window { ListView { SectionHeader { Text { text: section-|",
    );
    for name in [
        "section-label",
        "section-first-index",
        "section-count",
        "section-pinned",
    ] {
        assert!(inside.iter().any(|item| item["label"] == name), "{name}");
    }
    let outside = complete_at("component App inherits Window { ListView { Text { text: section-|");
    assert!(!outside.iter().any(|item| item["label"] == "section-label"));
}

#[test]
fn repeated_tabs_offer_model_fields_and_a_read_only_count() {
    let source = r#"
        struct Document { id: string, label: string }
        component App inherits Window {
            in property <[Document]> docs;
            tabs := TabWidget {
                current-value: "one";
                for doc in root.docs : Tab { value: doc.id; title: doc.label; }
            }
            PageIndicator { count: tabs.count; }
        }
    "#;
    let analysis = analysis::analyze(URI, source, None);
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let entries = complete_at(&source.replace("doc.label", "doc.|"));
    assert!(entries.iter().any(|item| item["label"] == "label"));
    let count = complete_at(&source.replace("tabs.count", "tabs.cou|"));
    assert!(count.iter().any(|item| {
        item["label"] == "count"
            && item["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("read-only int"))
    }));
    let bindings = complete_at("component App inherits Window { TabWidget { co| } }");
    assert!(!bindings.iter().any(|item| item["label"] == "count"));
}

#[test]
fn swipe_page_inherits_typed_record_completion_from_its_model() {
    let source = r#"
        struct Document { id: string, title: string }
        component App inherits Window {
            in property <[Document]> documents;
            SwipeView {
                current-value: "initial";
                for document in root.documents : SwipePage {
                    value: document.id;
                    Label { text: document.title; }
                }
            }
        }
    "#;
    let analysis = analysis::analyze(URI, source, None);
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let entries = complete_at(&source.replace("document.title", "document.|"));
    assert!(entries.iter().any(|item| item["label"] == "id"));
    assert!(entries.iter().any(|item| item["label"] == "title"));
}

#[test]
fn model_edits_complete_only_as_statements_with_writable_model_signatures() {
    let statement =
        complete_at("component App inherits Window { Button { clicked => { Model.| } } }");
    for method in ["append", "insert", "set", "remove", "move", "clear"] {
        assert!(
            statement.iter().any(|item| item["label"] == method),
            "{method}"
        );
    }
    let binding = complete_at("component App inherits Window { property <int> rows: Model.|; }");
    assert!(binding.iter().any(|item| item["label"] == "count"));
    assert!(!binding.iter().any(|item| item["label"] == "append"));
    let call = "component App inherits Window { Button { clicked => { Model.insert(root.rows, 1, ";
    let hint = completion::signature_help(call, (0, call.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Model.insert(model: writable [Row], index: int, value: Row)"
    );
    assert_eq!(hint["activeParameter"], 2);
}

#[test]
fn selection_gesture_completion_offers_typed_modes_and_read_only_state() {
    let options =
        complete_at("component App inherits Window { TableView { selection-gesture: |; } }");
    for mode in ["auto", "drag", "press-and-hold"] {
        assert!(options.iter().any(|item| item["label"] == mode), "{mode}");
    }
    let items = complete_at(
        "component App inherits Window { table := TableView {} Text { text: table.selection-dr|; } }",
    );
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "selection-dragging")
    );
}

#[test]
fn table_selection_handle_parts_complete_only_in_table_contexts() {
    for view in ["TableView", "StandardTableView"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {view} {{ Selection| }} }}"
        ));
        assert!(
            items
                .iter()
                .any(|item| item["label"] == "SelectionStartHandle")
        );
        assert!(
            items
                .iter()
                .any(|item| item["label"] == "SelectionEndHandle")
        );
    }
    let items = complete_at("component App inherits Window { Rectangle { Selection| } }");
    assert!(
        !items
            .iter()
            .any(|item| item["label"] == "SelectionStartHandle")
    );
}

#[test]
fn stack_layout_completes_index_and_read_only_selection_properties() {
    let bindings = complete_at("component App inherits Window { StackLayout { current-i| } }");
    assert!(
        bindings
            .iter()
            .any(|item| item["label"] == "current-index" && item["detail"] == "int")
    );
    let fields = complete_at(
        "component App inherits Window { layers := StackLayout {} Label { text: layers.active-i|; } }",
    );
    assert!(fields.iter().any(|item| item["label"] == "active-index"));
    let bindings = complete_at("component App inherits Window { StackLayout { active-i| } }");
    assert!(!bindings.iter().any(|item| item["label"] == "active-index"));
    let fields = complete_at(
        "component App inherits Window { StackLayout { page := StackItem {} Rectangle { visible: page.ac|; } } }",
    );
    assert!(fields.iter().any(|item| item["label"] == "active"));
}

#[test]
fn swipe_view_completes_an_integer_transition_duration() {
    for (control, available) in [
        ("SwipeView", true),
        ("StackView", true),
        ("TabWidget", false),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {control} {{ transition-d| }} }}"
        ));
        assert_eq!(
            items
                .iter()
                .any(|item| item["label"] == "transition-duration" && item["detail"] == "int"),
            available,
            "{control}"
        );
    }
}

#[test]
fn model_navigation_commands_offer_typed_signatures_and_moved_events() {
    for (kind, event) in [
        ("TabBar", "tab-moved"),
        ("TabWidget", "tab-moved"),
        ("SwipeView", "page-moved"),
    ] {
        let start = format!(
            "component App inherits Window {{ nav := {kind} {{}} tap := Button {{ clicked => {{ nav."
        );
        let call = complete_at(&format!("{start}move-i| }} }} }}"));
        assert!(call.iter().any(|item| item["label"] == "move-item"));
        let signature = format!("{start}move-item(0, ");
        let hint = completion::signature_help(&signature, (0, signature.len()), None);
        assert_eq!(
            hint["signatures"][0]["label"],
            "move-item(from: int, to: int)"
        );
        assert_eq!(hint["activeParameter"], 1);
        let members = complete_at(&format!(
            "component App inherits Window {{ nav := {kind} {{ {event}| }} }}"
        ));
        assert!(members.iter().any(|item| item["label"] == event));
    }
}

#[test]
fn tab_close_properties_and_handler_complete_in_both_tab_containers() {
    for (group, tab) in [("TabBar", "TabButton"), ("TabWidget", "Tab")] {
        let binding = complete_at(&format!(
            "component App inherits Window {{ tabs := {group} {{ tabs-clos| }} }}"
        ));
        assert!(
            binding
                .iter()
                .any(|item| item["label"] == "tabs-closable" && item["detail"] == "bool")
        );
        let binding = complete_at(&format!(
            "component App inherits Window {{ {group} {{ {tab} {{ clos| }} }} }}"
        ));
        assert!(
            binding
                .iter()
                .any(|item| item["label"] == "closable" && item["detail"] == "bool")
        );
        let event = complete_at(&format!(
            "component App inherits Window {{ tabs := {group} {{ tab-close-r| }} }}"
        ));
        assert!(
            event
                .iter()
                .any(|item| item["label"] == "tab-close-requested")
        );
    }
}

#[test]
fn tab_reordering_completion_and_diagnostics_use_compiler_types() {
    let widget_property = complete_at("component App inherits Window { TabWidget { reorder| } }");
    assert!(
        widget_property
            .iter()
            .any(|item| item["label"] == "reorderable" && item["detail"] == "bool")
    );
    let property = complete_at("component App inherits Window { TabBar { reorder| } }");
    assert!(
        property
            .iter()
            .any(|item| item["label"] == "reorderable" && item["detail"] == "bool")
    );
    let event = complete_at("component App inherits Window { TabBar { tab-mov| } }");
    assert!(event.iter().any(|item| item["label"] == "tab-moved"));
    let valid = r#"component App inherits Window {
        in-out property <[string]> entries: ["a", "b"];
        out property <int> last-move: 0;
        tabs := TabBar {
            reorderable: true;
            for label in root.entries: TabButton { text: label; }
            tab-moved(from, to) => { root.last-move = from * 10 + to; }
        }
    }"#;
    assert!(
        analysis::analyze(URI, valid, None)
            .diagnostics
            .get(URI)
            .is_none_or(Vec::is_empty)
    );
    let literal = valid.replace("root.entries:", r#"["a", "b"]:"#);
    assert!(
        analysis::analyze(URI, &literal, None).diagnostics[URI]
            .iter()
            .any(|item| {
                item["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("writable list property"))
            })
    );
}

#[test]
fn navigation_value_completion_shows_the_derived_index() {
    for group in ["TabBar", "TabWidget", "SwipeView"] {
        let values = complete_at(&format!(
            "component App inherits Window {{ {group} {{ current-v| }} }}"
        ));
        assert!(
            values
                .iter()
                .any(|item| item["label"] == "current-value" && item["detail"] == "string"),
            "{group}"
        );
        let bindings = complete_at(&format!(
            "component App inherits Window {{ {group} {{ current-value: \"a\"; current-i| }} }}"
        ));
        assert!(
            !bindings.iter().any(|item| item["label"] == "current-index"),
            "{group}"
        );
        let read = complete_at(&format!(
            "component App inherits Window {{ nav := {group} {{ current-value: \"a\"; }} Text {{ text: nav.current-i| }} }}"
        ));
        assert!(
            read.iter().any(|item| item["label"] == "current-index"
                && item["detail"]
                    .as_str()
                    .is_some_and(|text| text.contains("read-only"))),
            "{group}"
        );
    }
}

#[test]
fn path_view_completes_the_row_template_and_typed_progress() {
    let source = r#"
        struct Entry { title: string, number: int }
        component App inherits Window {
            in property <[Entry]> entries;
            PathView {
                path: "M 0 0.5 L 1 0.5";
                model: root.entries;
                text-role: "title";
                item-width: 90px;
                PathViewOption {
                    opacity: option-progress;
                    Text { text: option-data.title; }
                }
            }
        }
    "#;
    assert!(analysis::analyze(URI, source, None).diagnostics.is_empty());
    let parts = complete_at("component App inherits Window { PathView { PathViewO| } }");
    assert!(parts.iter().any(|item| item["label"] == "PathViewOption"));
    let progress = complete_at(&source.replace("option-progress", "option-pro|"));
    assert!(
        progress
            .iter()
            .any(|item| item["label"] == "option-progress"
                && item["detail"].as_str().is_some_and(|s| s.contains("float")))
    );
    let typed = complete_at(&source.replace("option-data.title", "option-data.|"));
    assert!(typed.iter().any(|item| item["label"] == "number"));
}

#[test]
fn tumbler_completes_deceleration_and_read_only_motion() {
    let bindings = complete_at("component App inherits Window { Tumbler { | } }");
    assert!(
        bindings
            .iter()
            .any(|item| item["label"] == "flick-deceleration" && item["detail"] == "float")
    );
    assert!(!bindings.iter().any(|item| item["label"] == "moving"));
    let reads = complete_at(
        "component App inherits Window { wheel := Tumbler {} Text { visible: wheel.| } }",
    );
    for property in ["moving", "dragging", "flicking"] {
        assert!(
            reads
                .iter()
                .any(|item| item["label"] == property && item["detail"] == "read-only bool")
        );
    }
    assert!(!reads.iter().any(|item| item["label"] == "rebounding"));
}

#[test]
fn tumbler_option_completes_record_fields_and_displacement() {
    let source = r#"
        struct Entry { label: string, offset: int }
        component App inherits Window {
            in property <[Entry]> entries;
            Tumbler {
                model: root.entries;
                text-role: "label";
                TumblerOption {
                    opacity: Math.abs(option-displacement);
                    Text { text: option-data.label; }
                }
            }
        }
    "#;
    let analyzed = analysis::analyze(URI, source, None);
    assert!(
        analyzed.diagnostics.is_empty(),
        "{:?}",
        analyzed.diagnostics
    );
    let part = complete_at("component App inherits Window { Tumbler { TumblerO| } }");
    assert!(part.iter().any(|item| item["label"] == "TumblerOption"));
    let record = complete_at(&source.replace("option-data.label", "option-data.|"));
    assert!(record.iter().any(|item| item["label"] == "offset"));
    let displacement = complete_at(&source.replace("option-displacement", "option-dis|"));
    assert!(displacement.iter().any(|item| {
        item["label"] == "option-displacement"
            && item["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("float"))
    }));
    let number = complete_at(
        "component App inherits Window { Tumbler { count: 12; TumblerOption { Text { text: option-dat|; } } } }",
    );
    assert!(
        number
            .iter()
            .any(|item| item["label"] == "option-data" && item["detail"] == "int")
    );
}

#[test]
fn tumbler_completion_uses_the_record_value_role_type() {
    let source = r#"
        struct Entry { label: string, key: int }
        component App inherits Window {
            in-out property <[Entry]> choices;
            in-out property <int> chosen: 2;
            wheel := Tumbler {
                model: root.choices;
                text-role: "label";
                value-role: "key";
                current-value <=> root.chosen;
                value-selected(value) => { root.chosen = value; }
            }
            Text { text: wheel.current-text; }
        }
    "#;
    let analyzed = analysis::analyze(URI, source, None);
    assert!(
        analyzed.diagnostics.is_empty(),
        "{:?}",
        analyzed.diagnostics
    );

    let detail = |items: &[Value], label| {
        items
            .iter()
            .find(|item| item["label"] == label)
            .map(|item| item["detail"].clone())
    };
    let bindings = complete_at(&source.replace("current-value <=> root.chosen;", "cur|"));
    assert_eq!(detail(&bindings, "current-value"), Some(json!("int")));
    assert_eq!(detail(&bindings, "current-text"), None);
    let usage = complete_at(&source.replace("wheel.current-text", "wheel.curr|"));
    assert_eq!(
        detail(&usage, "current-value"),
        Some(json!("effective int · model value-role"))
    );
    let handler = complete_at(&source.replace(
        "value-selected(value) => { root.chosen = value; }",
        "value-sel|",
    ));
    assert_eq!(
        detail(&handler, "value-selected"),
        Some(json!("event handler (int)"))
    );
}

#[test]
fn combo_option_data_has_model_field_completion_and_hover() {
    let source = "struct State { online: bool } struct Entry { title: string, state: State } component App inherits Window { in property <[Entry]> choices; ComboBox { model: root.choices; text-role: \"title\"; ComboOption { Label { text: option-data.| } } } }";
    let fields = complete_at(source);
    assert!(
        fields.iter().any(|item| item["label"] == "title"),
        "{fields:?}"
    );
    let nested = complete_at(&source.replace("option-data.|", "option-data.state.|"));
    assert!(
        nested.iter().any(|item| item["label"] == "online"),
        "{nested:?}"
    );
    let hover = request_at(
        "textDocument/hover",
        URI,
        &source.replace("option-data.|", "option-da|ta.title;"),
        None,
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .is_some_and(|text| text.contains("Entry")),
        "{hover}"
    );
    let strings = complete_at(
        "component App inherits Window { ComboBox { model: [\"Hi\"]; ComboOption { Label { text: option-da|",
    );
    assert!(
        strings
            .iter()
            .any(|item| item["label"] == "option-data" && item["detail"] == "string"),
        "{strings:?}"
    );
}

#[test]
fn combo_option_completion_and_hover_use_the_template_scope() {
    assert!(
        complete_at("component App inherits Window { ComboBox { ComboOpt|")
            .iter()
            .any(|item| item["label"] == "ComboOption")
    );
    let options = complete_at(
        "component App inherits Window { ComboBox { ComboOption { Label { text: option-|",
    );
    for name in [
        "option-text",
        "option-index",
        "option-enabled",
        "option-selected",
        "option-highlighted",
    ] {
        assert!(options.iter().any(|item| item["label"] == name), "{name}");
    }
    let outside = complete_at("component App inherits Window { Label { text: option-|");
    assert!(!outside.iter().any(|item| item["label"] == "option-text"));
    let hover = request_at(
        "textDocument/hover",
        URI,
        "component App inherits Window { ComboBox { ComboOption { Label { text: option-te|xt; } } } }",
        None,
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .is_some_and(|text| text.contains("string")),
        "{hover}"
    );
}

#[test]
fn combo_popup_commands_and_state_complete_on_the_receiver() {
    let methods = complete_at(
        "component App inherits Window { choice := ComboBox {} Button { clicked => { choice.| } } }",
    );
    for name in [
        "open-popup",
        "close-popup",
        "popup-open",
        "highlighted-index",
    ] {
        assert!(
            methods.iter().any(|item| item["label"] == name),
            "{name}: {methods:?}"
        );
    }
    let bindings = complete_at("component App inherits Window { ComboBox { highlight-| } }");
    assert!(
        !bindings
            .iter()
            .any(|item| item["label"] == "highlighted-index")
    );
}

#[test]
fn combo_popup_completes_only_supported_properties() {
    assert!(
        complete_at("component App inherits Window { ComboBox { ComboPop|")
            .iter()
            .any(|item| item["label"] == "ComboPopup")
    );
    let inside = complete_at("component App inherits Window { ComboBox { ComboPopup { |");
    for property in ["row-height", "max-height", "padding", "scroll-indicator"] {
        assert!(
            inside.iter().any(|item| item["label"] == property),
            "{property}"
        );
    }
    assert!(!inside.iter().any(|item| item["label"] == "x"));
}

#[test]
fn progress_bounds_complete_with_read_only_normalized_positions() {
    let properties = complete_at("component App inherits Window { ProgressIndicator { | } }");
    for name in ["minimum", "maximum", "progress"] {
        assert!(
            properties
                .iter()
                .any(|item| item["label"] == name && item["detail"] == "float"),
            "{name}"
        );
    }
    assert!(
        !properties
            .iter()
            .any(|item| item["label"] == "visual-position")
    );
    let members = complete_at(
        "component App inherits Window { meter := ProgressIndicator {} Label { text: meter.| } }",
    );
    for name in ["position", "visual-position"] {
        assert!(
            members.iter().any(|item| item["label"] == name
                && item["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.contains("read-only float"))),
            "{name}"
        );
    }
}

#[test]
fn content_slot_and_control_parts_offer_contextual_completion() {
    for (text, expected) in [
        (
            "component Panel inherits VerticalLayout { @ch|",
            "@children",
        ),
        (
            "component Demo inherits Window { Button { ButtonCo|",
            "ButtonContent",
        ),
        (
            "component Demo inherits Window { Slider { SliderHa|",
            "SliderHandle",
        ),
        (
            "component Demo inherits Window { ProgressIndicator { ProgressTr|",
            "ProgressTrack",
        ),
        (
            "component Demo inherits Window { ProgressIndicator { ProgressFi|",
            "ProgressFill",
        ),
        (
            "component Demo inherits Window { CheckBox { ToggleInd|",
            "ToggleIndicator",
        ),
        (
            "component Demo inherits Window { RadioGroup { RadioButton { ToggleInd|",
            "ToggleIndicator",
        ),
        (
            "component Demo inherits Window { Switch { ToggleInd|",
            "ToggleIndicator",
        ),
        (
            "component Demo inherits Window { Switch { SwitchTh|",
            "SwitchThumb",
        ),
        (
            "component Demo inherits Window { ListView { for name in [\"one\"] : SwitchDelegate { SwitchTh|",
            "SwitchThumb",
        ),
        (
            "component Demo inherits Window { SpinBox { SpinUpInd|",
            "SpinUpIndicator",
        ),
        (
            "component Demo inherits Window { ComboBox { ComboInd|",
            "ComboIndicator",
        ),
        (
            "component Demo inherits Window { ComboBox { ComboCon|",
            "ComboContent",
        ),
        (
            "component Demo inherits Window { DoubleSpinBox { SpinDownInd|",
            "SpinDownIndicator",
        ),
        (
            "component Demo inherits Window { RangeSlider { RangeLowerH|",
            "RangeLowerHandle",
        ),
        (
            "component Demo inherits Window { RangeSlider { RangeUpperH|",
            "RangeUpperHandle",
        ),
        (
            "component Demo inherits Window { TabBar { TabButton { ButtonCo|",
            "ButtonContent",
        ),
        (
            "component Demo inherits Window { TreeView { for branch in root.branches : TreeRow { TreeDisc|",
            "TreeDisclosure",
        ),
    ] {
        assert!(
            complete_at(text)
                .iter()
                .any(|item| item["label"] == expected),
            "{text}"
        );
    }
    let hover = request_at(
        "textDocument/hover",
        URI,
        "component Panel inherits VerticalLayout { @child|ren }",
        None,
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("caller's scope")
    );
    assert!(
        !complete_at("component Demo inherits Window { CheckBox { SwitchTh|")
            .iter()
            .any(|item| item["label"] == "SwitchThumb")
    );
    assert!(
        !complete_at("component Demo inherits Window { TreeView { TreeDisc|")
            .iter()
            .any(|item| item["label"] == "TreeDisclosure")
    );
    assert!(
        !complete_at("component Demo inherits Window { ComboInd|")
            .iter()
            .any(|item| item["label"] == "ComboIndicator")
    );
    assert!(
        !complete_at("component Demo inherits Window { ComboCon|")
            .iter()
            .any(|item| item["label"] == "ComboContent")
    );
    let invalid = "component Panel inherits VerticalLayout { @children @children }";
    assert!(
        analysis::analyze(URI, invalid, None).diagnostics[URI][0]["message"]
            .as_str()
            .unwrap()
            .contains("only once")
    );
}

#[test]
fn geometry_values_complete_types_namespaces_and_typed_fields() {
    for (source, expected) in [
        (
            "component Demo inherits Window { property <tra|",
            "transform2d",
        ),
        (
            "component Demo inherits Window { Rectangle { transf|",
            "transform",
        ),
        (
            "component Demo inherits Window { property <transform2d> t: Transform2D.co|",
            "compose",
        ),
        (
            "component Demo inherits Window { property <point2d> p; out property <length> x: root.p.|",
            "x",
        ),
        (
            "component Demo inherits Window { property <vector2d> v; out property <length> y: v.|",
            "y",
        ),
    ] {
        assert!(
            complete_at(source)
                .iter()
                .any(|item| item["label"] == expected),
            "{source}"
        );
    }
    let source = "component Demo inherits Window { property <point2d> p: Point2D.new(0px, ";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Point2D.new(x: length, y: length) -> point2d"
    );
}

#[test]
fn scene3d_completes_viewport_properties_and_geometry() {
    for (source, expected) in [
        (
            "component App inherits Window { SceneView3D { scene-effect-st| } }",
            "scene-effect-strength",
        ),
        (
            "component App inherits Window { SceneView3D { scene-effect-d| } }",
            "scene-effect-depth",
        ),
        (
            "component App inherits Window { SceneView3D { scene-effec| } }",
            "scene-effects",
        ),
        (
            "component App inherits Window { SceneView3D { texture-in| } }",
            "texture-input",
        ),
        (
            "component App inherits Window { SceneView3D { scene-ba| } }",
            "scene-background",
        ),
        (
            "component App inherits Window { SceneView3D { camer| } }",
            "camera",
        ),
        (
            "component App inherits Window { property <string> id: Scene3D.pick-ca|; }",
            "pick-camera",
        ),
        (
            "component App inherits Window { SceneView3D { camera-| } }",
            "camera-yaw",
        ),
        (
            "component App inherits Window { SceneView3D { mo| } }",
            "model",
        ),
        (
            "component App inherits Window { SceneView3D { lig| } }",
            "lights",
        ),
        (
            "component App inherits Window { SceneView3D { ambient-| } }",
            "ambient-intensity",
        ),
        (
            "component App inherits Window { SceneView3D { ambient-oc| } }",
            "ambient-occlusion",
        ),
        (
            "component App inherits Window { SceneView3D { ao-s| } }",
            "ao-strength",
        ),
        (
            "component App inherits Window { SceneView3D { ao-d| } }",
            "ao-distance",
        ),
        (
            "component App inherits Window { SceneView3D { ao-b| } }",
            "ao-bias",
        ),
        (
            "component App inherits Window { SceneView3D { scene-h| } }",
            "scene-hdr",
        ),
        (
            "component App inherits Window { SceneView3D { blo| } }",
            "bloom",
        ),
        (
            "component App inherits Window { SceneView3D { bloom-str| } }",
            "bloom-strength",
        ),
        (
            "component App inherits Window { SceneView3D { bloom-thr| } }",
            "bloom-threshold",
        ),
        (
            "component App inherits Window { SceneView3D { bloom-le| } }",
            "bloom-levels",
        ),
        (
            "component App inherits Window { SceneView3D { depth-of-f| } }",
            "depth-of-field",
        ),
        (
            "component App inherits Window { SceneView3D { dof-blur-| } }",
            "dof-blur-radius",
        ),
        (
            "component App inherits Window { SceneView3D { dof-focus-d| } }",
            "dof-focus-distance",
        ),
        (
            "component App inherits Window { SceneView3D { dof-focus-r| } }",
            "dof-focus-range",
        ),
        (
            "component App inherits Window { SceneView3D { shad| } }",
            "shadows",
        ),
        (
            "component App inherits Window { SceneView3D { shadow-| } }",
            "shadow-map-size",
        ),
        (
            "component App inherits Window { SceneView3D { shadow-cas| } }",
            "shadow-cascades",
        ),
        (
            "component App inherits Window { SceneView3D { shadow-cascade-| } }",
            "shadow-cascade-blend",
        ),
        (
            "component App inherits Window { SceneView3D { shadow-dis| } }",
            "shadow-distance",
        ),
        (
            "component App inherits Window { SceneView3D { shadow-spl| } }",
            "shadow-split-balance",
        ),
        (
            "component App inherits Window { SceneView3D { environment-s| } }",
            "environment-skybox",
        ),
        (
            "component App inherits Window { SceneView3D { reflection-pr| } }",
            "reflection-probes",
        ),
        (
            "component App inherits Window { SceneView3D { fru| } }",
            "frustum-culling",
        ),
        (
            "component App inherits Window { SceneView3D { multi| } }",
            "multisampling",
        ),
        (
            "component App inherits Window { SceneView3D { order-independent-tr| } }",
            "order-independent-transparency",
        ),
        (
            "component App inherits Window { SceneView3D { fog-c| } }",
            "fog-color",
        ),
        (
            "component App inherits Window { SceneView3D { fog-d| } }",
            "fog-density",
        ),
        (
            "component App inherits Window { SceneView3D { fog-f| } }",
            "fog-far",
        ),
        (
            "component App inherits Window { SceneView3D { fog-vol| } }",
            "fog-volumes",
        ),
        (
            "component App inherits Window { SceneView3D { fog-volume-sc| } }",
            "fog-volume-scattering",
        ),
        (
            "component App inherits Window { SceneView3D { fog-volume-sa| } }",
            "fog-volume-samples",
        ),
        (
            "component App inherits Window { SceneView3D { fog-volume-de| } }",
            "fog-volume-density-samples",
        ),
        (
            "component App inherits Window { SceneView3D { fog-volume-an| } }",
            "fog-volume-anisotropy",
        ),
        (
            "component App inherits Window { SceneView3D { fog-n| } }",
            "fog-near",
        ),
        (
            "component App inherits Window { SceneView3D { fog-depth-c| } }",
            "fog-depth-curve",
        ),
        (
            "component App inherits Window { SceneView3D { fog-depth-e| } }",
            "fog-depth-enabled",
        ),
        (
            "component App inherits Window { SceneView3D { fog-height-e| } }",
            "fog-height-enabled",
        ),
        (
            "component App inherits Window { SceneView3D { fog-height-en| } }",
            "fog-height-end",
        ),
        (
            "component App inherits Window { SceneView3D { fog-height-st| } }",
            "fog-height-start",
        ),
        (
            "component App inherits Window { SceneView3D { fog-height-c| } }",
            "fog-height-curve",
        ),
        (
            "component App inherits Window { SceneView3D { fog-transmit-e| } }",
            "fog-transmit-enabled",
        ),
        (
            "component App inherits Window { SceneView3D { fog-transmit-cu| } }",
            "fog-transmit-curve",
        ),
        (
            "component App inherits Window { property <string> id: Scene3D.pi|; }",
            "pick",
        ),
    ] {
        assert!(
            complete_at(source)
                .iter()
                .any(|item| item["label"] == expected),
            "{source}",
        );
    }
    let source = "component App inherits Window { property <string> item: Scene3D.pick(";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Scene3D.pick(objects: [record], yaw: float, pitch: float, distance: float, viewport: point2d, pointer: point2d, layer-mask?: int) -> string"
    );
    let source = "component App inherits Window { property <string> item: Scene3D.pick-camera(";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Scene3D.pick-camera(objects: [record], camera: string, viewport: point2d, pointer: point2d, layer-mask?: int) -> string"
    );
}

#[test]
fn scene_projection_types_fields_and_functions_complete_in_incomplete_source() {
    for (source, expected, detail) in [
        (
            "component App inherits Window { property <point3d> aim: Point3D.ne|",
            "new",
            "point3d",
        ),
        (
            "component App inherits Window { property <point3d> aim; property <float> z: root.aim.|",
            "z",
            "float",
        ),
        (
            "component App inherits Window { property <projectedpoint3d> aim; property <length> x: root.aim.point.|",
            "x",
            "length",
        ),
        (
            "component App inherits Window { property <projectedpoint3d> aim; property <bool> visible: root.aim.in|",
            "inside",
            "bool",
        ),
        (
            "component App inherits Window { property <projectedpoint3d> aim: Scene3D.project-c|",
            "project-camera",
            "projectedpoint3d",
        ),
        (
            "component App inherits Window { property <point3d> aim: Scene3D.unpr|",
            "unproject",
            "point3d",
        ),
    ] {
        assert!(
            complete_at(source)
                .iter()
                .any(|item| item["label"] == expected
                    && item["detail"]
                        .as_str()
                        .is_some_and(|value| value.contains(detail))),
            "{source}"
        );
    }

    let source = "component App inherits Window { property <point3d> p: Scene3D.unproject-camera(";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Scene3D.unproject-camera(objects: [record], camera: string, viewport: point2d, pointer: point2d, depth: float, fallback: point3d) -> point3d"
    );
}

#[test]
fn hit3d_completion_exposes_world_local_and_nested_fields() {
    for (source, expected, detail) in [
        (
            "component App inherits Window { property <hit3d> hit: Scene3D.hit-c|",
            "hit-camera",
            "hit3d",
        ),
        (
            "component App inherits Window { property <hit3d> hit; property <string> id: root.hit.obj|",
            "object-id",
            "string",
        ),
        (
            "component App inherits Window { property <hit3d> hit; property <float> z: root.hit.normal.|",
            "z",
            "float",
        ),
        (
            "component App inherits Window { property <hit3d> hit; property <point3d> position: root.hit.loc|",
            "local-position",
            "point3d",
        ),
        (
            "component App inherits Window { property <vector3d> direction: Vector3D.ne|",
            "new",
            "vector3d",
        ),
    ] {
        assert!(
            complete_at(source)
                .iter()
                .any(|item| item["label"] == expected
                    && item["detail"]
                        .as_str()
                        .is_some_and(|value| value.contains(detail))),
            "{source}"
        );
    }
    let source = "component App inherits Window { property <hit3d> p: Scene3D.ray-hit(";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Scene3D.ray-hit(objects: [record], origin: point3d, direction: vector3d, max-distance: float, layer-mask?: int) -> hit3d"
    );
}

#[test]
fn hit_list_completion_and_signature_help_are_typed() {
    let type_template = "component App inherits Window { property <[hit3d]> hits: []; BODY";
    for (use_site, label, detail) in [
        (
            "out property <[hit3d]> picked: Scene3D.hit-all-c|",
            "hit-all-camera",
            "[hit3d]",
        ),
        (
            "VerticalLayout { for contact in root.hits: Text { text: contact.row-|",
            "row-index",
            "int",
        ),
        (
            "VerticalLayout { for contact in root.hits: Text { text: contact.instance-|",
            "instance-index",
            "int",
        ),
        (
            "VerticalLayout { for contact in root.hits: Text { text: contact.mesh-node-|",
            "mesh-node-index",
            "int",
        ),
        (
            "VerticalLayout { for contact in root.hits: Text { text: contact.normal.|",
            "z",
            "float",
        ),
    ] {
        let items = complete_at(&type_template.replace("BODY", use_site));
        assert!(
            items.iter().any(|item| item["label"] == label
                && item["detail"]
                    .as_str()
                    .is_some_and(|ty| ty.contains(detail))),
            "{items:?}"
        );
    }
    let source = "component App inherits Window { property <[hit3d]> hits: Scene3D.ray-hit-all(";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Scene3D.ray-hit-all(objects: [record], origin: point3d, direction: vector3d, max-distance: float, layer-mask?: int) -> [hit3d]"
    );
}

#[test]
fn scene_hierarchy_record_fields_complete_with_their_types() {
    for (prefix, field, ty) in [
        ("par", "parent", "string"),
        ("vis", "visible", "bool"),
        ("pivot-", "pivot-x", "float"),
        ("camera-look-", "camera-look-at", "string"),
        ("camera-le", "camera-left", "float"),
        ("camera-ri", "camera-right", "float"),
        ("camera-bo", "camera-bottom", "float"),
        ("camera-to", "camera-top", "float"),
        ("ani", "animation", "string"),
        ("animation-t", "animation-time", "float"),
        ("animation-l", "animation-loop", "bool"),
        ("animation-t", "animation-to", "string"),
        ("animation-to-t", "animation-to-time", "float"),
        ("animation-to-l", "animation-to-loop", "bool"),
        ("animation-b", "animation-blend", "float"),
        ("lod-o", "lod-of", "string"),
        ("lod-max", "lod-max-size", "length"),
        ("camera-lod-", "camera-lod-bias", "float"),
        ("morph-w", "morph-weights", "PanelWeights"),
    ] {
        let source = format!("struct PanelWeights {{ extend: float }} struct Actor {{ parent: string, visible: bool, pivot-x: float, camera-look-at: string, camera-left: float, camera-right: float, camera-bottom: float, camera-top: float, animation: string, animation-time: float, animation-loop: bool, animation-to: string, animation-to-time: float, animation-to-loop: bool, animation-blend: float, lod-of: string, lod-max-size: length, camera-lod-bias: float, morph-weights: PanelWeights }} component App inherits Window {{
            property <Actor> node: Actor {{ {prefix}| }}
        }}");
        let items = complete_at(&source);
        let item = items
            .iter()
            .find(|item| item["label"] == field)
            .unwrap_or_else(|| panic!("missing {field}: {items:?}"));
        assert!(
            item["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains(ty)),
            "{item}"
        );
    }
}

#[test]
fn completes_authored_camera_kinds_in_partial_string_literals() {
    for source in [
        "struct Actor { camera-kind: string } component App { property <Actor> eye: Actor { camera-kind: \"fr|",
        "struct Actor { camera-kind: string } component App { property <Actor> eye: Actor { camera-kind: \"fr|ustum\" }; }",
    ] {
        let items = complete_at(source);
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["label"], "frustum");
        assert_eq!(items[0]["textEdit"]["newText"], "frustum");
    }
    assert!(complete_at("component App { property <string> camera-kind: \"fr|\"; }").is_empty());
}

#[test]
fn completes_a_partially_typed_named_morph_target() {
    let items = complete_at(
        "struct Weights { extend: float, target-0: float }
         struct Actor { morph-weights: Weights }
         component App inherits Window {
             property <Actor> model: Actor { morph-weights: Weights { ext| } };
         }",
    );
    assert!(items.iter().any(|item| {
        item["label"] == "extend"
            && item["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("float"))
    }));
}

#[test]
fn fog_volume_shapes_complete_in_record_literals() {
    let items = complete_at(
        r#"struct Mist { fog-shape: string }
        component App inherits Window {
            property <Mist> mist: Mist { fog-shape: "sp|" };
        }"#,
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "sphere");
    assert_eq!(items[0]["textEdit"]["newText"], "sphere");
}

#[test]
fn fog_volume_records_complete_height_and_orientation_fields() {
    let source = r#"struct Mist {
            position: point3d, size: vector3d, density: float, color: color,
            yaw: float, pitch: float, roll: float,
            fog-height-enabled: bool, fog-height-start: float, fog-height-end: float,
        }
        component App inherits Window {
            property <Mist> mist: Mist { PREFIX };
        }"#;
    for (prefix, field, ty) in [
        ("fog-height-st|", "fog-height-start", "float"),
        ("fog-height-e|", "fog-height-enabled", "bool"),
        ("rol|", "roll", "float"),
    ] {
        let items = complete_at(&source.replace("PREFIX", prefix));
        assert!(
            items
                .iter()
                .any(|item| item["label"] == field && item["detail"] == ty),
            "{field}: {items:?}"
        );
    }
}

#[test]
fn fog_volume_records_complete_spatial_noise_with_types() {
    let source = r#"struct Mist {
            position: point3d, size: vector3d, density: float, color: color,
            fog-noise-amount: float, fog-noise-scale: float, fog-noise-offset: vector3d,
        }
        component App inherits Window {
            property <Mist> mist: Mist { PREFIX };
        }"#;
    for (prefix, field, ty) in [
        ("fog-noise-a|", "fog-noise-amount", "float"),
        ("fog-noise-s|", "fog-noise-scale", "float"),
        ("fog-noise-o|", "fog-noise-offset", "vector3d"),
    ] {
        let items = complete_at(&source.replace("PREFIX", prefix));
        assert!(
            items
                .iter()
                .any(|item| item["label"] == field && item["detail"] == ty),
            "{field}: {items:?}"
        );
    }
}

#[test]
fn built_in_primitive_keys_and_material_slots_complete() {
    for field in ["mesh", "emitter-mesh", "particle-model"] {
        let source = format!(
            r#"struct Actor {{ FIELD: string }} component App inherits Window {{
                property <Actor> actor: Actor {{ FIELD: "primitive:c|" }};
            }}"#,
        )
        .replace("FIELD", field);
        let candidates = complete_at(&source);
        assert!(
            candidates
                .iter()
                .any(|item| item["label"] == "primitive:cylinder")
        );
        assert!(
            candidates
                .iter()
                .any(|item| item["label"] == "primitive:cone")
        );
    }
    let directory = workspace_fixture("primitive-slots");
    let file = directory.join("main.xr");
    let uri = workspace_file_uri(&file);
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let marked = r#"component App inherits Window { property <Actor> object: Actor {
        mesh: "primitive:cylinder",
        material-slots: Palette { cap: Style { slot: "t|" } }
    } }"#;
    let source = marked.replace('|', "");
    let (line, column) = analysis::Source::new(&source).coordinates(marked.find('|').unwrap());
    let values = completion::complete(&source, line, column, Some(&workspace));
    assert!(values.iter().any(|item| item["label"] == "top"));
}

#[test]
fn material_slot_selector_completion_uses_the_enclosing_mesh() {
    let directory =
        std::env::temp_dir().join(format!("xrack-material-slots-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("sample.gltf"),
        r##"{
        "asset":{"version":"2.0"},
        "materials":[{"name":"Trim"},{"name":"Body finish"},{"name":"#custom"}],
        "meshes":[{"primitives":[{"material":1},{}]}]
    }"##,
    )
    .unwrap();
    let file = directory.join("app.xr");
    let uri = workspace_file_uri(&file);
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let prefix = r#"component App inherits Window { property <Actor> model: Actor {
        mesh: @mesh-url("sample.gltf"),
        material-slots: Palette { paint: Style { slot: ""#;
    let options = completion::complete(
        prefix,
        2,
        prefix.lines().last().unwrap().len(),
        Some(&workspace),
    );
    for expected in ["#0", "#1", "#default", "Body finish", "name:#custom"] {
        assert!(
            options.iter().any(|item| item["label"] == expected),
            "{expected}: {options:?}"
        );
    }
    let asset_cursor =
        analysis::Source::new(prefix).coordinates(prefix.find("sample.gltf").unwrap());
    let hover = completion::query(
        "textDocument/hover",
        &uri,
        prefix,
        Some(asset_cursor),
        Some(&workspace),
    );
    let details = hover["contents"]["value"].as_str().unwrap();
    assert!(details.contains("#1  Body finish") && details.contains("#default  implicit material"));
    let independent = r#"component App inherits Window { property <Style> chosen: Style { slot: ""#;
    assert!(completion::complete(independent, 0, independent.len(), Some(&workspace)).is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn authored_texture_maps_complete_wrapping_and_sampler_filtering() {
    let source = r#"struct Map { source: string, wrap-u: string, mip-filter: string }
        struct Material { base-color-map: Map }
        component App inherits Window {
            property <Material> style: Material { base-color-map: Map { PREFIX } };
        }"#;
    let wrapping = complete_at(&source.replace("PREFIX", r#"wrap-u: "mi|""#));
    assert!(wrapping.iter().any(|item| item["label"] == "mirror"));
    let filtering = complete_at(&source.replace("PREFIX", r#"mip-filter: "no|""#));
    assert!(filtering.iter().any(|item| item["label"] == "none"));
    let expression = "component App inherits Window { property <string> map: @texture-url(";
    let help = completion::signature_help(expression, (0, expression.len()), None);
    assert_eq!(
        help["signatures"][0]["label"],
        "@texture-url(path: string literal) -> string"
    );
}

#[test]
fn authored_scene_material_uses_typed_fields_and_alpha_options() {
    let source = r#"struct SurfaceStyle { roughness: float, alpha-mode: string }
        struct Actor { material: SurfaceStyle }
        component App inherits Window {
            property <Actor> model: Actor { material: SurfaceStyle { PREFIX } };
        }"#;
    let fields = complete_at(&source.replace("PREFIX", "roug|"));
    assert!(
        fields
            .iter()
            .any(|item| item["label"] == "roughness" && item["detail"] == "float")
    );
    let modes = complete_at(&source.replace("PREFIX", r#"alpha-mode: "bl|""#));
    assert!(modes.iter().any(|item| item["label"] == "blend"));
}

#[test]
fn filtered_picking_completes_row_and_subset_signatures() {
    let items =
        complete_at("component App inherits Window { property <hit3d> hit: Scene3D.hit-|; }");
    for name in [
        "hit-row",
        "hit-row-camera",
        "hit-subset",
        "hit-subset-camera",
    ] {
        assert!(items.iter().any(|item| item["label"] == name));
    }
    let source = "component App inherits Window { property <[hit3d]> hits: Scene3D.ray-hit-subset(";
    let signature = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        signature["signatures"][0]["label"],
        "Scene3D.ray-hit-subset(objects: [record], origin: point3d, direction: vector3d, max-distance: float, rows: [int], layer-mask?: int) -> [hit3d]"
    );
}

#[test]
fn rectangle_picking_completes_both_camera_variants() {
    let items =
        complete_at("component App inherits Window { property <[int]> rows: Scene3D.pick-in-|; }");
    for name in ["pick-in-rect", "pick-in-rect-camera"] {
        assert!(items.iter().any(|item| item["label"] == name));
    }
    let source =
        "component App inherits Window { property <[int]> rows: Scene3D.pick-in-rect-camera(";
    let help = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        help["signatures"][0]["label"],
        "Scene3D.pick-in-rect-camera(objects: [record], camera: string, viewport: point2d, start: point2d, end: point2d, layer-mask?: int) -> [int]"
    );
}

#[test]
fn closest_point_queries_offer_signatures_with_target_and_radius() {
    let items =
        complete_at("component App inherits Window { property <hit3d> hit: Scene3D.closest-|; }");
    for (member, target) in [
        ("closest-point", ""),
        ("closest-point-on", "object-id: string, "),
    ] {
        assert!(items.iter().any(|item| item["label"] == member));
        let source =
            format!("component App inherits Window {{ property <hit3d> hit: Scene3D.{member}(");
        let signature = completion::signature_help(&source, (0, source.len()), None);
        assert_eq!(
            signature["signatures"][0]["label"],
            format!(
                "Scene3D.{member}(objects: [record], {target}origin: point3d, radius: float, layer-mask?: int) -> hit3d"
            )
        );
    }
}

#[test]
fn sphere_queries_complete_and_show_radius_and_layer_signatures() {
    let items =
        complete_at("component App inherits Window { property <hit3d> hit: Scene3D.sphere-|; }");
    for expected in ["sphere-cast", "sphere-overlap"] {
        assert!(items.iter().any(|item| item["label"] == expected));
    }
    let source = "component App inherits Window { property <hit3d> hit: Scene3D.sphere-cast(";
    let signature = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        signature["signatures"][0]["label"],
        "Scene3D.sphere-cast(objects: [record], origin: point3d, direction: vector3d, max-distance: float, radius: float, layer-mask?: int) -> hit3d"
    );
}

#[test]
fn sphere_slide_completion_describes_displacement_and_returned_center() {
    let items = complete_at(
        "component App inherits Window { property <point3d> moved: Scene3D.sphere-s|; }",
    );
    assert!(items.iter().any(|item| item["label"] == "sphere-slide"));
    let source = "component App inherits Window { property <point3d> moved: Scene3D.sphere-slide(";
    let signature = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        signature["signatures"][0]["label"],
        "Scene3D.sphere-slide(objects: [record], origin: point3d, displacement: vector3d, radius: float, layer-mask?: int) -> point3d"
    );
}

#[test]
fn sphere_walk_completion_describes_step_snap_and_slope() {
    let items = complete_at(
        "component App inherits Window { property <point3d> moved: Scene3D.sphere-w|; }",
    );
    assert!(items.iter().any(|item| item["label"] == "sphere-walk"));
    let source = "component App inherits Window { property <point3d> moved: Scene3D.sphere-walk(";
    let signature = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        signature["signatures"][0]["label"],
        "Scene3D.sphere-walk(objects: [record], origin: point3d, displacement: vector3d, radius: float, step-height: float, snap-distance: float, max-slope: float, layer-mask?: int) -> point3d"
    );
}

#[test]
fn mesh_url_completes_automatic_lod_option() {
    let items = complete_at(
        r#"component App inherits Window { property <string> key: @mesh-url("model.glb", au|); }"#,
    );
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["textEdit"]["newText"], "auto-lod");
}

#[test]
fn mesh_url_completes_the_chart_option_after_lod() {
    let complete = |tail: &str| {
        complete_at(&format!(
            r#"component App inherits Window {{ property <string> key: @mesh-url("model.glb", {tail}); }}"#
        ))
    };
    let options = complete("|");
    assert!(options.iter().any(|item| item["label"] == "lightmap-uv"));
    let after_lod = complete("auto-lod, lig|");
    assert_eq!(after_lod.len(), 1);
    assert_eq!(after_lod[0]["textEdit"]["newText"], "lightmap-uv");
    let after_chart = complete("lightmap-uv, |");
    assert_eq!(after_chart.len(), 1);
    assert_eq!(after_chart[0]["textEdit"]["newText"], "auto-lod");
}

#[test]
fn scene_asset_urls_complete_and_link_local_resources() {
    let root = workspace_fixture("mesh-paths");
    let models = root.join("models");
    std::fs::create_dir_all(&models).unwrap();
    for file in [
        "room.glb",
        "roof.gltf",
        "room.bin",
        "room.hdr",
        "room.wgsl",
        "room.png",
        "room.svg",
    ] {
        std::fs::write(models.join(file), []).unwrap();
    }
    let uris = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let uri = uris.uri(&root.join("app.xr")).unwrap();
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let shader =
        r#"component App inherits Window { SceneView3D { scene-effect: @shader-url("models/ro"#;
    let shaders = completion::complete(shader, 0, shader.len(), Some(&workspace));
    assert_eq!(shaders.len(), 1);
    assert_eq!(shaders[0]["textEdit"]["newText"], "room.wgsl");
    let complete_shader = shader.to_owned() + r#"om.wgsl"); } }"#;
    let links = completion::document_links(&complete_shader, &workspace);
    assert_eq!(
        links[0]["target"],
        workspace.uri(&models.join("room.wgsl")).unwrap()
    );
    let source = r#"component App inherits Window { property <string> mesh: @mesh-url("models/ro"#;
    let items = completion::complete(source, 0, source.len(), Some(&workspace));
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str())
            .collect::<Vec<_>>(),
        [Some("roof.gltf"), Some("room.glb"), Some("room.png")]
    );
    assert_eq!(items[1]["textEdit"]["newText"], "room.glb");
    let emitter = r#"struct Emitter { emitter-mesh: string } component App inherits Window { property <Emitter> emitter: Emitter { emitter-mesh: @mesh-url("models/ro"#;
    let items = completion::complete(emitter, 0, emitter.len(), Some(&workspace));
    assert!(items.iter().any(|item| item["label"] == "room.glb"));
    let model = emitter.replace("emitter-mesh", "particle-model");
    let items = completion::complete(&model, 0, model.len(), Some(&workspace));
    assert!(items.iter().any(|item| item["label"] == "room.glb"));
    let texture =
        r#"component App inherits Window { property <string> map: @texture-url("models/ro"#;
    let textures = completion::complete(texture, 0, texture.len(), Some(&workspace));
    assert_eq!(textures.len(), 1);
    assert_eq!(textures[0]["textEdit"]["newText"], "room.png");
    let complete_texture = texture.to_owned() + r#"om.png"); }"#;
    let links = completion::document_links(&complete_texture, &workspace);
    assert_eq!(
        links[0]["target"],
        workspace.uri(&models.join("room.png")).unwrap()
    );
    for field in [
        "specular-map",
        "specular-color-map",
        "thickness-map",
        "anisotropy-map",
    ] {
        let source = format!(
            r#"struct Finish {{ {field}: string }} component App inherits Window {{
                property <Finish> finish: Finish {{ {field}: @texture-url("models/ro"#
        );
        let cursor = source.lines().last().unwrap().len();
        let items = completion::complete(&source, 1, cursor, Some(&workspace));
        assert!(
            items.iter().any(|item| item["label"] == "room.png"),
            "{field}: {items:?}"
        );
    }
    let source =
        r#"component App inherits Window { SceneView3D { environment: @environment-url("models/ro"#;
    let items = completion::complete(source, 0, source.len(), Some(&workspace));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["textEdit"]["newText"], "room.hdr");
    let complete = source.to_owned() + r#"om.hdr"); } }"#;
    let links = completion::document_links(&complete, &workspace);
    assert_eq!(
        links[0]["target"],
        workspace.uri(&models.join("room.hdr")).unwrap()
    );
    let source =
        r#"component App inherits Window { SceneView3D { scene-lut: @image-url("models/ro"#;
    let items = completion::complete(source, 0, source.len(), Some(&workspace));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["textEdit"]["newText"], "room.png");
    let complete = source.to_owned() + r#"om.png"); } }"#;
    let links = completion::document_links(&complete, &workspace);
    assert_eq!(
        links[0]["target"],
        workspace.uri(&models.join("room.png")).unwrap()
    );
    let members = complete_at("component App inherits Window { SceneView3D { scene-lut| } }");
    for expected in ["scene-lut", "scene-lut-size", "scene-lut-strength"] {
        assert!(
            members.iter().any(|item| item["label"] == expected),
            "{expected}"
        );
    }

    let source = r#"struct Emitter { sprite: image } component App inherits Window { property <Emitter> effect: Emitter { sprite: @image-url("models/ro"#;
    let items = completion::complete(source, 0, source.len(), Some(&workspace));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["textEdit"]["newText"], "room.png");
    let complete = source.to_owned() + r#"om.png") }; }"#;
    let links = completion::document_links(&complete, &workspace);
    assert_eq!(
        links[0]["target"],
        workspace.uri(&models.join("room.png")).unwrap()
    );
    let source = source.replace("sprite", "color-table");
    let items = completion::complete(&source, 0, source.len(), Some(&workspace));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["textEdit"]["newText"], "room.png");
}

#[test]
fn png_mesh_resource_links_and_displays_heightfield_dimensions() {
    let root = workspace_fixture("heightfield-hover");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let uri = workspace.uri(&root.join("app.xr")).unwrap();
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    header.extend(64u32.to_be_bytes());
    header.extend(32u32.to_be_bytes());
    header.extend([16, 0, 0, 0, 0]);
    let png = root.join("ridge.png");
    std::fs::write(&png, header).unwrap();
    let source = r#"component App inherits Window { title: @mesh-url("ridge.png"); }"#;
    let offset = source.find("ridge").unwrap();
    let hover = completion::query(
        "textDocument/hover",
        &uri,
        source,
        Some(analysis::Source::new(source).coordinates(offset)),
        Some(&workspace),
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("64 × 32 samples · 16-bit grayscale")
    );
    let links = completion::document_links(source, &workspace);
    assert_eq!(links[0]["target"], workspace.uri(&png).unwrap());
}

#[test]
fn names_glb_animation_clips_in_record_bindings() {
    let root = workspace_fixture("mesh-clip-values");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let uri = workspace.uri(&root.join("app.xr")).unwrap();
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let mut json =
        br#"{"animations":[{"name":"Slide"},{"name":"Step"},{},{"name":"Step"},{"name":"Morph","channels":[{"target":{"path":"weights"}}]}]}"#.to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bytes = b"glTF".to_vec();
    bytes.extend(2u32.to_le_bytes());
    bytes.extend((20u32 + json.len() as u32).to_le_bytes());
    bytes.extend((json.len() as u32).to_le_bytes());
    bytes.extend(b"JSON");
    bytes.extend(json);
    std::fs::write(root.join("rig.glb"), bytes).unwrap();
    let source = r#"component App inherits Window { property <Actor> mesh: Actor {
        animation: "S|", mesh: @mesh-url("rig.glb")
    }; }"#;
    let marker = source.find('|').unwrap();
    let source = source.replace('|', "");
    let (line, column) = analysis::Source::new(&source).coordinates(marker);
    let items = completion::complete(&source, line, column, Some(&workspace));
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["textEdit"]["newText"], "Slide");
    let mut source = source;
    source.replace_range(marker - 1..marker, "M");
    let items = completion::complete(&source, line, column, Some(&workspace));
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["textEdit"]["newText"], "Morph");
    source.replace_range(marker - 1..marker, "#");
    let (line, column) = analysis::Source::new(&source).coordinates(marker);
    let items = completion::complete(&source, line, column, Some(&workspace));
    assert_eq!(
        items
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect::<Vec<_>>(),
        ["#1", "#2", "#3"]
    );
    let incomplete = r#"component App inherits Window { property <Actor> mesh: Actor {
        mesh: @mesh-url("rig.glb"), animation-to: "Sl"#;
    let (line, column) = analysis::Source::new(incomplete).coordinates(incomplete.len());
    let items = completion::complete(incomplete, line, column, Some(&workspace));
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["textEdit"]["newText"], "Slide");
}

#[test]
fn camera_selector_completion_uses_the_rig_in_the_current_component() {
    let root = workspace_fixture("mesh-camera-values");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let uri = workspace.uri(&root.join("app.xr")).unwrap();
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    for (file, name) in [("first.gltf", "Hero"), ("second.gltf", "Other")] {
        std::fs::write(
            root.join(file),
            serde_json::json!({
                "nodes": [{"camera":0, "name":name}], "cameras": [{"type":"perspective"}]
            })
            .to_string(),
        )
        .unwrap();
    }
    let marked = r#"
        component Other inherits Window {
            property <[Actor]> scene: [Actor { id: "rig", mesh: @mesh-url("second.gltf") }];
        }
        component App inherits Window {
            property <[Actor]> scene: [Actor { id: "rig", mesh: @mesh-url("first.gltf") }];
            SceneView3D { model: root.scene; camera: "rig/|"; }
        }
    "#;
    let text = marked.replace('|', "");
    let (line, column) = analysis::Source::new(&text).coordinates(marked.find('|').unwrap());
    let items = completion::complete(&text, line, column, Some(&workspace));
    assert_eq!(
        items
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect::<Vec<_>>(),
        ["rig/#0", "rig/Hero"]
    );
    assert_eq!(items[1]["textEdit"]["newText"], "rig/Hero");
}

#[test]
fn glb_hover_lists_texture_samplers_and_material_metadata_without_binary_data() {
    let root = workspace_fixture("mesh-morph-hover");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let uri = workspace.uri(&root.join("app.xr")).unwrap();
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();

    let mut json = br#"{"extensionsUsed":["KHR_texture_transform"],"nodes":[{"camera":0,"name":"Hero"},{"camera":1},{"camera":1}],"cameras":[{"type":"perspective"},{"type":"orthographic","name":"Duplicate"}],"extensions":{"KHR_lights_punctual":{"lights":[{"type":"point","name":"Fill"},{"type":"spot"}]}},"materials":[
        {"extensions":{"KHR_materials_clearcoat":{},"KHR_materials_unknown":{}}},
        {"extensions":{"KHR_materials_clearcoat":{},"KHR_materials_emissive_strength":{},"KHR_materials_ior":{},"KHR_materials_specular":{},"KHR_materials_transmission":{},"KHR_materials_volume":{},"KHR_materials_dispersion":{}}}
    ],"meshes":[
        {"primitives":[{"targets":[{},{}]}],"extras":{"targetNames":["extend","retract"]}},
        {"primitives":[{"targets":[{},{}]}],"extras":{"targetNames":["blink","blink"]}},
        {"primitives":[{"targets":[{}]}],"extras":[123]}
    ],"textures":[{"sampler":0},{"sampler":1},{}],
    "samplers":[{"minFilter":9985},{"minFilter":9728}]}"#
        .to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let size: u32 = 24 * 1024 * 1024;
    let mesh = root.join("face.glb");
    let mut bytes = b"glTF".to_vec();
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(size.to_le_bytes());
    bytes.extend((json.len() as u32).to_le_bytes());
    bytes.extend(b"JSON");
    bytes.extend(json);
    std::fs::write(&mesh, bytes).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(mesh)
        .unwrap()
        .set_len(size.into())
        .unwrap();

    let marked = r#"component App inherits Window { title: @mesh-url("fac|e.glb"); }"#;
    let text = marked.replace('|', "");
    let cursor = analysis::Source::new(&text).coordinates(marked.find('|').unwrap());
    let hover = completion::query(
        "textDocument/hover",
        &uri,
        &text,
        Some(cursor),
        Some(&workspace),
    );
    assert_eq!(hover["contents"]["kind"], "plaintext");
    assert_eq!(
        hover["contents"]["value"],
        "glTF morph targets for morph-weights\n\nMesh #0\ntarget-0  extend\ntarget-1  retract\n\nMesh #1\ntarget-0\ntarget-1\n\nMesh #2\ntarget-0\n\nglTF material extensions\nKHR_materials_clearcoat\nKHR_materials_emissive_strength\nKHR_materials_ior\nKHR_materials_specular\nKHR_materials_transmission\nKHR_materials_volume\nKHR_materials_dispersion\n\nglTF texture coordinates\nKHR_texture_transform (offset, rotation, scale, UV set)\n\nglTF camera nodes (use row-id/selector)\n#0  perspective  Hero\n#1  orthographic\n#2  orthographic\n\nglTF punctual lights\nLight #0  point  Fill\nLight #1  spot\n\nglTF texture minification\nTexture #0  LINEAR_MIPMAP_NEAREST\nTexture #1  NEAREST\nTexture #2  LINEAR_MIPMAP_LINEAR (XRack default)"
    );
}

#[test]
fn standard_library_completion_and_signature_help_use_the_shared_catalog() {
    for &namespace in xrack_parser::builtins::NAMESPACES {
        let items = complete_at(&format!(
            "component App inherits Window {{ title: {namespace}.|"
        ));
        for function in xrack_parser::builtins::FUNCTIONS
            .iter()
            .filter(|function| function.namespace == namespace && function.result.is_some())
        {
            assert!(
                items.iter().any(|item| item["label"] == function.member
                    && item["detail"] == function.signature()),
                "missing {}.{}",
                function.namespace,
                function.member
            );
        }
    }

    let source =
        "component App inherits Window { title: Strings.format(\"{}\", Math.clamp(2px, 0px, ";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Math.clamp(value: T, min: T, max: T) -> T"
    );
    assert_eq!(hint["activeParameter"], 2);

    let source = "component App inherits Window { background: Colors.rgba(255, 0, 32, ";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Colors.rgba(red: int, green: int, blue: int, alpha: float) -> color"
    );
    assert_eq!(hint["activeParameter"], 3);

    let source = "component App inherits Window { width: I18n.parse-number(root.input, ";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "I18n.parse-number(text: string, fallback: float) -> float"
    );
    assert_eq!(hint["activeParameter"], 1);

    let source = "component App inherits Window { out property <Date> result: Dates.add-months(root.input, -1, ";
    let hint = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        hint["signatures"][0]["label"],
        "Dates.add-months(date: Date, months: int, fallback: Date) -> Date"
    );
    assert_eq!(hint["activeParameter"], 2);

    let weekday = request_at(
        "textDocument/hover",
        URI,
        "component App inherits Window { out property <int> weekday: Dates.week|day(Date { year: 2024, month: 2, day: 29 }); }",
        None,
    );
    assert!(
        weekday["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Monday (1) through Sunday (7)")
    );

    let invalid = "component App inherits Window { out property <bool> valid: Dates.is-valid(2); }";
    let diagnostics = analysis::analyze(URI, invalid, None).diagnostics;
    assert!(
        diagnostics[URI][0]["message"]
            .as_str()
            .unwrap()
            .contains("expected date, found int")
    );

    let locale = request_at(
        "textDocument/hover",
        URI,
        "component App inherits Window { title: I18n.lo|cale(); }",
        None,
    );
    assert!(
        locale["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("negotiated catalog locale")
    );

    let invalid = "component App inherits Window { title: I18n.locale(1); }";
    let diagnostic = &analysis::analyze(URI, invalid, None).diagnostics[URI][0];
    let start = invalid.find("I18n.locale").unwrap();
    assert_eq!(
        diagnostic["range"],
        analysis::Source::new(invalid).range(start, start + "I18n.locale".len())
    );
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("expects 0 arguments")
    );

    let hover = request_at(
        "textDocument/hover",
        URI,
        "component App inherits Window { title: Strings.character-co|unt(\"👩‍💻\"); }",
        None,
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Unicode grapheme")
    );

    let argument = complete_at(
        "component App inherits Window { button := Button { clicked => { Log.write((LogLe|",
    );
    assert!(argument.iter().any(|item| item["label"] == "LogLevel"));
    let shadowed =
        complete_at("component App inherits Window { Model := Rectangle {} title: Model.|");
    assert!(!shadowed.iter().any(|item| item["label"] == "count"));
}

#[test]
fn element_receivers_shadow_builtin_namespaces() {
    let source = r#"component Operations inherits Rectangle {
        pure callback round(value: int) -> int;
        round(value) => { value + 1 }
    }
    export component App inherits Window {
        Math := Operations {}
        title: Strings.format("{}", Math.round(2));
    }"#;
    assert!(analysis::analyze(URI, source, None).diagnostics.is_empty());
    assert_eq!(
        request_at(
            "textDocument/definition",
            URI,
            &source.replace("Math.round", "Math.ro|und"),
            None,
        ),
        location_at(
            URI,
            &source.replace("callback round", "callback |round"),
            "round"
        ),
    );
    let items = complete_at(&source.replace("Math.round(2)", "Math.|"));
    assert!(items.iter().any(|item| item["label"] == "round"));
    assert!(!items.iter().any(|item| item["label"] == "pi"));
    let offset = source.find("Math.round(2)").unwrap() + "Math.round(".len();
    let hint = completion::signature_help(
        source,
        analysis::Source::new(source).coordinates(offset),
        None,
    );
    assert_eq!(hint["signatures"][0]["label"], "round(value: int) -> int");
    let invalid = source.replace("Math.round(2)", "Math.pi");
    assert!(
        !analysis::analyze(URI, &invalid, None)
            .diagnostics
            .is_empty()
    );
}

#[test]
fn builtin_signatures_work_in_incomplete_buffers_and_diagnostics_cover_function_names() {
    for (call, parameter, signature) in [
        (
            "Log.write(LogLevel.debug, ",
            1,
            "Log.write(level: LogLevel, message: string)",
        ),
        (
            "Log.debug(Strings.format(\"{}\", ",
            1,
            "Strings.format(template: string literal, values...) -> string",
        ),
        ("Log.info(", 0, "Log.info(message: string)"),
        ("Math.round(", 0, "Math.round(value: int | float) -> int"),
        (
            "@tr(\"welcome\", ",
            1,
            "@tr(id: string literal, named arguments...) -> string",
        ),
        (
            "@linear-gradient(90deg, ",
            1,
            "@linear-gradient(angle: degrees, color stops...) -> brush",
        ),
        (
            "@image-url(",
            0,
            "@image-url(path: string literal, nine-slice(...) (optional)) -> image",
        ),
        (
            "@mesh-url(",
            0,
            "@mesh-url(path: string literal, auto-lod (optional), lightmap-uv (optional)) -> string",
        ),
        (
            "@environment-url(",
            0,
            "@environment-url(path: string literal) -> string",
        ),
        (
            "@shader-url(",
            0,
            "@shader-url(path: string literal) -> string",
        ),
    ] {
        let text =
            format!("component Demo inherits Window {{ button := Button {{ clicked => {{ {call}");
        let source = analysis::Source::new(&text);
        let hint = completion::signature_help(&text, source.coordinates(text.len()), None);
        assert_eq!(hint["signatures"][0]["label"], signature);
        assert_eq!(hint["activeParameter"], parameter);
        assert_eq!(hint["signatures"][0]["documentation"]["kind"], "markdown");
        if call == "@shader-url(" {
            let help = hint["signatures"][0]["documentation"]["value"].to_string();
            assert!(
                help.contains("scene_surface") && help.contains("scene_buffer_world_position"),
                "{help}"
            );
        }
    }

    for (call, expected) in [
        (
            r#"Log.write("missing level");"#,
            "expects a level and a message",
        ),
        (r#"Log.unknown("message");"#, "unknown built-in function"),
    ] {
        let text = format!(
            "component Demo inherits Window {{ button := Button {{ clicked => {{ {call} }} }} }}"
        );
        let diagnostic = &analysis::analyze(URI, &text, None).diagnostics[URI][0];
        let start = text.find("Log.").unwrap();
        let end = start + call.find('(').unwrap();
        assert_eq!(diagnostic["code"], "semantic");
        assert_eq!(
            diagnostic["range"],
            analysis::Source::new(&text).range(start, end)
        );
        assert!(diagnostic["message"].as_str().unwrap().contains(expected));
    }
}

#[test]
fn definition_respects_scope_and_keeps_working_before_the_file_parses() {
    const SOURCE: &str = concat!(
        "component Demo inherits Window {\r\n",
        "  property <int> count: 0;\r\n",
        "  callback change(count: int);\r\n",
        "  change(count) => { root.count += count; }\r\n",
        "  for item[index] in root.count : row := Text { text: \"中🙂\" + index + item; }\r\n",
        "  Text { text: row.text + root.count + ",
    );
    for (reference, declaration, name) in [
        ("root.|count +", "<int> |count: 0", "count"),
        ("+= co|unt;", "change(|count) =>", "count"),
        ("+ |index +", "item[|index]", "index"),
        ("+ it|em;", "for |item", "item"),
        ("ro|w.text", ": |row :=", "row"),
        ("  ch|ange(count) =>", "callback |change(", "change"),
    ] {
        let clean = reference.replace('|', "");
        let target = SOURCE.replace(&declaration.replace('|', ""), declaration);
        assert_eq!(
            request_at(
                "textDocument/definition",
                URI,
                &SOURCE.replacen(&clean, reference, 1),
                None
            ),
            location_at(URI, &target, name),
            "{reference}"
        );
    }
    let hover = request_at(
        "textDocument/hover",
        URI,
        &SOURCE.replace("+ index +", "+ in|dex +"),
        None,
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("index: int")
    );
    let highlights = request_at(
        "textDocument/documentHighlight",
        URI,
        &SOURCE.replace("+= count;", "+= co|unt;"),
        None,
    );
    assert_eq!(highlights.as_array().unwrap().len(), 2);
}

#[test]
fn definition_follows_import_aliases_inheritance_and_nested_record_fields() {
    let uri = fixture_uri("entry.xr");
    let card_uri = fixture_uri("Card%20%E4%B8%AD.xr");
    let base_uri = fixture_uri("base.xr");
    let card = concat!(
        "import { Parent } from \"base.xr\";\n",
        "export struct CardData { address: Address }\n",
        "struct Address { city: string }\n",
        "export component Card inherits Parent { in property <CardData> data; }",
    );
    let base = "export component Parent inherits Rectangle { in property <int> points; property <int> hidden; callback reset; }";
    let workspace = completion::Workspace::new(
        &uri,
        [(card_uri.as_str(), card), (base_uri.as_str(), base)].into_iter(),
    )
    .unwrap();
    let prefix = "import { Card as Tile, CardData as Data } from \"Card 中.xr\";\n";
    for (line, target, name) in [
        (
            "component Demo inherits Ti|le {",
            location_at(
                &card_uri,
                &card.replace("component Card", "component |Card"),
                "Card",
            ),
            "Tile",
        ),
        (
            "component Demo inherits Window { item := Tile {} Text { text: item.po|ints;",
            location_at(&base_uri, &base.replace("> points", "> |points"), "points"),
            "points",
        ),
        (
            "component Demo inherits Window { property <[Da|ta]> items;",
            location_at(
                &card_uri,
                &card.replace("struct CardData", "struct |CardData"),
                "CardData",
            ),
            "Data",
        ),
        (
            "component Demo inherits Window { item := Tile {} Text { text: item.data.address.ci|ty;",
            location_at(&card_uri, &card.replace("{ city:", "{ |city:"), "city"),
            "city",
        ),
        (
            "component Demo inherits Window { item := Tile {} Button { clicked => { item.re|set();",
            location_at(
                &base_uri,
                &base.replace("callback reset", "callback |reset"),
                "reset",
            ),
            "reset",
        ),
    ] {
        assert_eq!(
            request_at(
                "textDocument/definition",
                &uri,
                &format!("{prefix}{line}"),
                Some(&workspace)
            ),
            target,
            "{name}"
        );
    }
    assert_eq!(
        request_at(
            "textDocument/definition",
            &uri,
            &format!(
                "{prefix}component Demo inherits Window {{ item := Tile {{}} Text {{ text: item.hid|den;"
            ),
            Some(&workspace),
        ),
        Value::Null
    );
    assert_eq!(
        request_at(
            "textDocument/definition",
            &uri,
            &prefix.replace("as Tile", "as Ti|le"),
            Some(&workspace)
        ),
        location_at(
            &card_uri,
            &card.replace("component Card", "component |Card"),
            "Card"
        ),
    );
}

#[test]
fn semantic_tokens_report_context_and_utf16_deltas() {
    let text = concat!(
        "struct address { city: string }\n",
        "component Demo inherits Window {\r\n",
        "  property <[address]> people;\n",
        "  pure callback format(value: address) -> string;\n",
        "  format(value) => { \"🙂\" + value.city }\n",
        "  Text { wrap: TextWrap.word-wrap; text: \"people\"; } // Window\n",
        "  Button { clicked => { Log.write(LogLevel.info, Strings.format(\"{}\", 1)); } }\n",
    );
    let legend = completion::legend();
    let response = completion::query("textDocument/semanticTokens/full", URI, text, None, None);
    let data = response["data"].as_array().unwrap();
    let mut previous = (0, 0);
    let source = analysis::Source::new(text);
    let tokens: Vec<_> = data
        .chunks_exact(5)
        .map(|token| {
            let number = |i: usize| token[i].as_u64().unwrap() as usize;
            previous.0 += number(0);
            previous.1 = if number(0) == 0 {
                previous.1 + number(1)
            } else {
                number(1)
            };
            let start = source.offset_at(previous.0, previous.1).unwrap();
            let end = source
                .offset_at(previous.0, previous.1 + number(2))
                .unwrap();
            (
                &text[start..end],
                legend["tokenTypes"][number(3)].as_str().unwrap(),
                number(4),
            )
        })
        .collect();
    for expected in [
        ("address", "struct", 1),
        ("address", "struct", 0),
        ("string", "type", 4),
        ("Window", "class", 4),
        ("people", "property", 1),
        ("format", "function", 1),
        ("value", "parameter", 1),
        ("value", "parameter", 0),
        ("city", "property", 0),
        ("TextWrap", "enum", 4),
        ("word-wrap", "enumMember", 4),
        ("write", "function", 4),
        ("format", "function", 4),
        ("LogLevel", "enum", 4),
        ("info", "enumMember", 4),
    ] {
        assert!(
            tokens.contains(&expected),
            "missing {expected:?}: {tokens:?}"
        );
    }
    assert_eq!(tokens.iter().filter(|token| token.0 == "people").count(), 1);
    assert_eq!(tokens.iter().filter(|token| token.0 == "Window").count(), 1);
}

pub(crate) fn workspace_fixture(name: &str) -> std::path::PathBuf {
    let root = std::env::var_os("TEST_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("xrack-lsp-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

fn serve_bytes(input: &[u8], output: &mut impl io::Write) -> io::Result<ExitCode> {
    let (events, receiver) = mpsc::channel();
    server_events::read_input(input, events.clone());
    serve(receiver, events, output)
}

fn exchange(messages: Vec<Value>) -> Vec<Value> {
    let mut input = Vec::new();
    for message in messages {
        protocol::write_message(&mut input, &message).unwrap();
    }
    let mut output = Vec::new();
    serve_bytes(&input, &mut output).unwrap();
    let mut output = output.as_slice();
    std::iter::from_fn(|| protocol::read_message(&mut output).unwrap()).collect()
}

fn request_document_symbols(id: i32, uri: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":uri}}})
}

#[test]
fn diagnostics_follow_transitive_unsaved_imports_and_fall_back_to_disk() {
    let root = workspace_fixture("diagnostics");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let app_uri = workspace.uri(&root.join("app.xr")).unwrap();
    let bridge_uri = workspace.uri(&root.join("bridge.xr")).unwrap();
    let leaf = root.join("leaf.xr");
    let leaf_uri = workspace.uri(&leaf).unwrap();
    let app = "import { Bridge } from \"bridge.xr\";\ncomponent App inherits Window { Bridge { value: 1; } }";
    let bridge = "import { Leaf } from \"leaf.xr\";\nexport component Bridge inherits Leaf {}";
    let good = "export component Leaf inherits Rectangle { in property <int> value; }";
    let different_type = "export component Leaf inherits Rectangle { in property <string> value; }";
    let semantic_error = "export component Leaf inherits Rectangle {\n  Text { text: \"中🙂\" + missing; }\n  in property <int> value;\n}";
    let syntax_error = "export component Leaf inherits Rectangle {";
    std::fs::write(root.join("bridge.xr"), bridge).unwrap();
    std::fs::write(&leaf, good).unwrap();
    let open = |uri: &str, text: &str| json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument":{"uri":uri,"version":1,"text":text}}});
    let change = |version, text| json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{"textDocument":{"uri":leaf_uri,"version":version}, "contentChanges":[{"text":text}]}});
    let diagnostics = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        open(&app_uri, app),
        open(&leaf_uri, different_type),
        change(2, semantic_error),
        request_document_symbols(10, &leaf_uri),
        change(3, syntax_error),
        json!({"jsonrpc":"2.0", "method":"textDocument/didClose", "params":{"textDocument":{"uri":leaf_uri}}}),
        open(&bridge_uri, bridge),
    ]);
    let reports = |uri: &str| {
        diagnostics
            .iter()
            .filter(|message| {
                message["method"] == "textDocument/publishDiagnostics"
                    && message["params"]["uri"] == uri
            })
            .map(|message| &message["params"])
            .collect::<Vec<_>>()
    };
    let app_reports = reports(&app_uri);
    assert_eq!(app_reports.len(), 5, "{app_reports:?}");
    assert_eq!(app_reports[0]["diagnostics"], json!([]));
    let mismatch = &app_reports[1]["diagnostics"][0];
    assert_eq!(mismatch["range"]["start"]["line"], 1);
    assert!(
        mismatch["message"]
            .as_str()
            .unwrap()
            .contains("type mismatch")
    );
    for report in &app_reports[2..4] {
        let error = &report["diagnostics"][0];
        assert_eq!(report["version"], 1);
        assert_eq!(error["range"]["start"]["line"], 0);
        assert_eq!(error["relatedInformation"][0]["location"]["uri"], leaf_uri);
    }
    assert_eq!(app_reports[4]["diagnostics"], json!([]));
    let leaf_reports = reports(&leaf_uri);
    let error = &leaf_reports[1]["diagnostics"][0];
    assert_eq!(error["code"], "semantic");
    assert_eq!(error["range"]["start"]["line"], 1);
    assert_eq!(
        error["range"]["start"]["character"],
        "  Text { text: \"中🙂\" + ".encode_utf16().count()
    );
    assert_eq!(leaf_reports[2]["diagnostics"][0]["code"], "syntax");
    assert_eq!(leaf_reports[3]["diagnostics"], json!([]));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_new_unsaved_import_satisfies_a_previously_missing_dependency() {
    let root = workspace_fixture("new-import");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let app = workspace.uri(&root.join("app.xr")).unwrap();
    let dependency = workspace.uri(&root.join("new.xr")).unwrap();
    let open = |uri: &str, text: &str| json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument":{"uri":uri,"version":1,"text":text}}});
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        open(
            &app,
            "import { NewItem } from \"new.xr\"; component App inherits Window { NewItem {} }",
        ),
        open(
            &dependency,
            "export component NewItem inherits Rectangle {}",
        ),
    ]);
    let reports: Vec<_> = messages
        .iter()
        .filter(|message| message["params"]["uri"] == app)
        .collect();
    assert_eq!(reports.len(), 2);
    assert!(
        reports[0]["params"]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("could not open")
    );
    assert_eq!(reports[1]["params"]["diagnostics"], json!([]));
    assert!(!root.join("new.xr").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn watched_import_creation_and_deletion_refresh_open_dependents() {
    let root = workspace_fixture("watched-import");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let app = workspace.uri(&root.join("app.xr")).unwrap();
    let dependency = root.join("item.xr");
    let mut documents = BTreeMap::from([(
        app.clone(),
        Document {
            version: 1,
            text: "import { Item } from \"item.xr\"; component App inherits Window { Item {} }"
                .into(),
            analysis: analysis::Analysis::default(),
        },
    )]);
    let mut published = BTreeMap::new();
    let mut output = Vec::new();
    refresh_analysis(
        &mut output,
        &mut documents,
        &mut published,
        Some(&app),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(
        published[&app]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("could not open")
    );
    std::fs::write(&dependency, "export component Item inherits Rectangle {}").unwrap();
    let changed = BTreeSet::from([normalize_source_path(&dependency)]);
    refresh_analysis(
        &mut output,
        &mut documents,
        &mut published,
        None,
        changed.clone(),
    )
    .unwrap();
    assert_eq!(published[&app]["diagnostics"], json!([]));
    std::fs::remove_file(dependency).unwrap();
    refresh_analysis(&mut output, &mut documents, &mut published, None, changed).unwrap();
    assert!(
        published[&app]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("could not open")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn references_and_rename_use_declaration_identity_across_workspace_files() {
    let root = workspace_fixture("rename");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let root_uri = workspace.uri(&root).unwrap();
    let widget = workspace.uri(&root.join("widget.xr")).unwrap();
    let app = workspace.uri(&root.join("app.xr")).unwrap();
    let unopened = workspace.uri(&root.join("unopened.xr")).unwrap();
    let declaration = "export component Card inherits Rectangle { in property <int> count; }";
    let client = "import { Card as Tile } from \"widget.xr\";\ncomponent App inherits Window {\n tile := Tile { count: 3; }\n Text { text: \"🙂\" + tile.count; }\n}";
    std::fs::write(
        root.join("widget.xr"),
        "export component Saved inherits Rectangle {}",
    )
    .unwrap();
    std::fs::write(root.join("unopened.xr"), "import { Card } from \"widget.xr\"; component Another inherits Window { Card { count: 9; } }").unwrap();
    let request = |id, method: &str, uri: &str, text: &str, marker: &str, name: &str| {
        let offset = text.find(marker).unwrap() + marker.len() - 1;
        let (line, character) = analysis::Source::new(text).coordinates(offset);
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"textDocument":{"uri":uri},"position":{"line":line,"character":character},"newName":name,"context":{"includeDeclaration":false}}})
    };
    let open = |uri: &str, text: &str| json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument":{"uri":uri,"version":1,"text":text}}});
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":root_uri}}),
        open(&widget, declaration),
        open(&app, client),
        request(2, "textDocument/references", &app, client, "tile.count", ""),
        request(
            3,
            "textDocument/rename",
            &app,
            client,
            "tile.count",
            "quantity",
        ),
        request(
            4,
            "textDocument/rename",
            &widget,
            declaration,
            "Card",
            "PanelCard",
        ),
        request(
            5,
            "textDocument/rename",
            &app,
            client,
            "as Tile",
            "PanelTile",
        ),
        request(
            6,
            "textDocument/prepareRename",
            &app,
            client,
            "tile.count",
            "",
        ),
        request(7, "textDocument/rename", &app, client, "tile.count", "tile"),
    ]);
    let response = |id| messages.iter().find(|message| message["id"] == id).unwrap();
    assert_eq!(response(2)["result"].as_array().unwrap().len(), 3);
    let edits = |id, uri: &str| {
        response(id)["result"]["changes"][uri]
            .as_array()
            .map_or(0, Vec::len)
    };
    assert_eq!(
        [edits(3, &app), edits(3, &widget), edits(3, &unopened)],
        [2, 1, 1]
    );
    assert_eq!(
        [edits(4, &app), edits(4, &widget), edits(4, &unopened)],
        [1, 1, 2]
    );
    assert_eq!(
        [edits(5, &app), edits(5, &widget), edits(5, &unopened)],
        [2, 0, 0]
    );
    assert_eq!(response(6)["result"]["placeholder"], "count");
    // The instance ID and a property on that instance occupy different scopes.
    assert_eq!(
        [edits(7, &app), edits(7, &widget), edits(7, &unopened)],
        [2, 1, 1]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn reference_requests_observe_preceding_buffer_updates_over_stdio() {
    let uri = "untitled:counter.xr";
    let first =
        "component App inherits Window { property <int> count; Text { text: \"\" + root.count; } }";
    let second = first.replace("count", "total");
    let request = |id, text: &str, spelling: &str| {
        let offset = text.rfind(spelling).unwrap();
        let (line, character) = analysis::Source::new(text).coordinates(offset);
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/references","params":{
            "textDocument":{"uri":uri}, "position":{"line":line,"character":character},
            "context":{"includeDeclaration":true}
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"version":1,"text":first}}}),
        request(2, first, "count"),
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":format!("\n{second}")} ]}}),
        request(3, &format!("\n{second}"), "total"),
    ]);
    for (id, line) in [(2, 0), (3, 1)] {
        let response = messages.iter().find(|message| message["id"] == id).unwrap();
        let references = response["result"].as_array().unwrap();
        assert_eq!(references.len(), 2);
        assert!(
            references.iter().all(
                |location| location["uri"] == uri && location["range"]["start"]["line"] == line
            )
        );
    }
}

#[test]
fn definition_and_rename_use_inherited_private_members_and_fall_back_during_edits() {
    let root = workspace_fixture("semantic-navigation");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let base_path = root.join("base.xr");
    let base_uri = workspace.uri(&base_path).unwrap();
    let app_uri = workspace.uri(&root.join("app.xr")).unwrap();
    let base = "export component Base inherits Window { property <int> hidden-count: 1; }";
    std::fs::write(&base_path, base).unwrap();
    let app = "import { Base } from \"base.xr\"; component App inherits Base { Text { text: \"🙂\" + root.hidden-count; } }";
    let incomplete = app.replace("root.hidden-count", "root");
    let open = json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":app_uri,"version":1,"text":app}}});
    let request = |id, method: &str, text: &str, marker: &str| {
        let (line, character) =
            analysis::Source::new(text).coordinates(text.rfind(marker).unwrap());
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"textDocument":{"uri":app_uri},
            "position":{"line":line,"character":character},"newName":"private-total"}})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":workspace.uri(&root)}}),
        open,
        request(2, "textDocument/definition", app, "hidden-count"),
        request(3, "textDocument/rename", app, "hidden-count"),
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
            "textDocument":{"uri":app_uri,"version":2},"contentChanges":[{"text":incomplete.trim_end_matches('}')}]
        }}),
        request(4, "textDocument/definition", &incomplete, "Base"),
    ]);
    let response = |id| messages.iter().find(|message| message["id"] == id).unwrap();
    assert_eq!(
        response(2)["result"],
        location_at(
            &base_uri,
            &base.replace("hidden-count", "|hidden-count"),
            "hidden-count"
        )
    );
    let edits = &response(3)["result"]["changes"];
    assert_eq!(edits[&base_uri].as_array().unwrap().len(), 1);
    assert_eq!(edits[&app_uri].as_array().unwrap().len(), 1);
    assert_eq!(response(4)["result"]["uri"], base_uri);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn workspace_search_and_auto_import_use_indexed_exports_and_unsaved_names() {
    let root = workspace_fixture("auto-import");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let widgets = root.join("widgets.xr");
    let widget_uri = workspace.uri(&widgets).unwrap();
    let app_uri = workspace.uri(&root.join("app.xr")).unwrap();
    let typed_uri = workspace.uri(&root.join("nested/typed.xr")).unwrap();
    std::fs::write(&widgets, "export component OldCard inherits Rectangle {}").unwrap();
    std::fs::write(
        root.join("cycle.xr"),
        "import { App } from \"app.xr\"; export component CardCycle inherits App {}",
    )
    .unwrap();
    let app = "// keep this comment\r\nexport component App inherits Window {\r\n  Car\r\n}";
    let typed = "component Typed inherits Window { property <Card> value; }";
    let typed_position = analysis::Source::new(typed).coordinates(typed.find("Card").unwrap() + 4);
    let (line, character) = analysis::Source::new(app).coordinates(app.find("Car").unwrap() + 3);
    let open = |uri: &str, text: &str| json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"version":1,"text":text}}});
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":workspace.uri(&root)}}),
        open(
            &widget_uri,
            "export component CardView inherits Rectangle {} component CardPrivate inherits Rectangle {} export struct CardData { count: int }",
        ),
        open(&app_uri, app),
        open(&typed_uri, typed),
        json!({"jsonrpc":"2.0","id":2,"method":"workspace/symbol","params":{"query":"cardv"}}),
        json!({"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{"textDocument":{"uri":app_uri},"position":{"line":line,"character":character}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"textDocument/completion","params":{"textDocument":{"uri":typed_uri},"position":{"line":typed_position.0,"character":typed_position.1}}}),
    ]);
    let response = |id| messages.iter().find(|message| message["id"] == id).unwrap();
    let symbols = response(2)["result"].as_array().unwrap();
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["name"], "CardView");
    assert_eq!(symbols[0]["location"]["uri"], widget_uri);
    let items = response(3)["result"].as_array().unwrap();
    let auto_imports: Vec<_> = items
        .iter()
        .filter(|item| item.get("additionalTextEdits").is_some())
        .collect();
    assert_eq!(auto_imports.len(), 1, "{items:?}");
    let candidate = auto_imports[0];
    assert_eq!(candidate["label"], "CardView");
    let edit = &candidate["additionalTextEdits"][0];
    assert_eq!(edit["range"]["start"], json!({"line":1,"character":0}));
    assert_eq!(
        edit["newText"],
        "import { CardView } from \"widgets.xr\";\r\n"
    );
    let records: Vec<_> = response(4)["result"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item.get("additionalTextEdits").is_some())
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["label"], "CardData");
    assert_eq!(
        records[0]["additionalTextEdits"][0]["newText"],
        "import { CardData } from \"../widgets.xr\";\n"
    );
    let edited = app
        .replace(
            "export component",
            &format!("{}export component", edit["newText"].as_str().unwrap()),
        )
        .replace("Car\r\n", "CardView {}\r\n");
    let entry = xrack_parser::parse(&edited).unwrap();
    let buffers = BTreeMap::from([(widgets, "export component CardView inherits Rectangle {}")]);
    let document = xrack_ir::modules::resolve_document(
        entry,
        &root.join("app.xr"),
        None,
        &buffers,
        &mut BTreeMap::new(),
        None,
    )
    .unwrap();
    assert!(xrack_ir::semantic::resolve_symbols(&document).is_ok());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rename_allows_equal_names_in_independent_component_and_handler_scopes() {
    let source = r#"component One inherits Window {
        property <int> count: 1;
        callback first(int) -> int;
        first(value) => { value + root.count }
        callback second(int) -> int;
        second(total) => { total }
    }
    component Two inherits Window {
        property <int> total: 2;
        Text { text: "" + root.total; }
    }"#;
    let request = |id, marker: &str, new_name: &str| {
        let offset = source.find(marker).unwrap();
        let (line, character) = analysis::Source::new(source).coordinates(offset);
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/rename","params":{
            "textDocument":{"uri":URI},"position":{"line":line,"character":character},"newName":new_name
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":URI,"version":1,"text":source}
        }}),
        request(2, "count: 1", "total"),
        request(3, "value +", "total"),
    ]);
    let edits = |id| {
        let response = messages.iter().find(|message| message["id"] == id).unwrap();
        response["result"]["changes"][URI]
            .as_array()
            .unwrap_or_else(|| panic!("{response}"))
    };
    assert_eq!(edits(2).len(), 2);
    assert_eq!(edits(3).len(), 2);
    assert!(
        edits(2)
            .iter()
            .chain(edits(3))
            .all(|edit| edit["range"]["start"]["line"].as_u64().unwrap() < 7)
    );
}

#[test]
fn rename_rejects_duplicate_declarations_and_capture_in_both_directions() {
    let source = r#"component App inherits Window {
        property <int> count: 1;
        property <int> total: 2;
        callback calculate(int) -> int;
        calculate(shadow) => { count + shadow + total }
        Rectangle {
            property <int> local: 3;
            Text { text: "" + total; }
        }
    }"#;
    let request = |id, marker: &str, new_name: &str| {
        let offset = source.find(marker).unwrap();
        let (line, character) = analysis::Source::new(source).coordinates(offset);
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/rename","params":{
            "textDocument":{"uri":URI},"position":{"line":line,"character":character},"newName":new_name
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":URI,"version":1,"text":source}
        }}),
        request(2, "count: 1", "total"),
        request(3, "count: 1", "shadow"),
        request(4, "shadow +", "total"),
        request(5, "local: 3", "total"),
        request(6, "local: 3", "count"),
    ]);
    let response = |id| messages.iter().find(|message| message["id"] == id).unwrap();
    for id in 2..=5 {
        assert_eq!(response(id)["error"]["code"], -32602, "{}", response(id));
    }
    assert_eq!(
        response(6)["result"]["changes"][URI]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn rename_checks_scopes_in_incomplete_buffers() {
    let source = r#"component App inherits Window {
        property <int> count;
        property <int> total;
        Text { text: "" + root.count;
    "#;
    let (line, character) =
        analysis::Source::new(source).coordinates(source.find("count").unwrap());
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":URI,"version":1,"text":source}
        }}),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/rename","params":{
            "textDocument":{"uri":URI},"position":{"line":line,"character":character},"newName":"total"
        }}),
        json!({"jsonrpc":"2.0","id":3,"method":"textDocument/rename","params":{
            "textDocument":{"uri":URI},"position":{"line":line,"character":character},"newName":"amount"
        }}),
    ]);
    let response = |id| messages.iter().find(|message| message["id"] == id).unwrap();
    assert_eq!(response(2)["error"]["code"], -32602);
    assert_eq!(
        response(3)["result"]["changes"][URI]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn rename_rejects_conflicts_in_unedited_transitive_importers() {
    let root = workspace_fixture("rename-importer-conflict");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let base = root.join("base.xr");
    let base_uri = workspace.uri(&base).unwrap();
    let source = "export component Base inherits Window { property <int> count: 1; }";
    std::fs::write(&base, source).unwrap();
    std::fs::write(
        root.join("bridge.xr"),
        "import { Base } from \"base.xr\"; export component Bridge inherits Base {}",
    )
    .unwrap();
    std::fs::write(
        root.join("app.xr"),
        "import { Bridge } from \"bridge.xr\"; component App inherits Bridge { property <int> total: 2; }",
    ).unwrap();
    let (line, character) =
        analysis::Source::new(source).coordinates(source.find("count").unwrap());
    let rename = |id, name| {
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/rename","params":{
            "textDocument":{"uri":base_uri},"position":{"line":line,"character":character},"newName":name
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":workspace.uri(&root)}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":base_uri,"version":1,"text":source}
        }}),
        rename(2, "total"),
        rename(3, "amount"),
    ]);
    let response = |id| messages.iter().find(|message| message["id"] == id).unwrap();
    assert_eq!(response(2)["error"]["code"], -32602);
    let changes = response(3)["result"]["changes"].as_object().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[&base_uri].as_array().unwrap().len(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn signature_help_tracks_nested_calls_and_imported_unsaved_callbacks() {
    let root = workspace_fixture("signature-help");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let library = workspace.uri(&root.join("math.xr")).unwrap();
    let implementation = workspace.uri(&root.join("implementation.xr")).unwrap();
    let app = workspace.uri(&root.join("app.xr")).unwrap();
    let marked = r#"import { Math as Helper } from "math.xr";
component App inherits Window {
    math := Helper {}
    pure callback convert(input: int, scale: int) -> int;
    pure callback local <=> root.convert;
    property <int> first: math.sum(root.local(1, /*inner*/2), /*outer*/3);
    property <int> second: math.sum(4, /*incomplete*/
}"#;
    let hint = |id, marker: &str| {
        let offset = marked.find(marker).unwrap() + marker.len();
        let (line, character) = analysis::Source::new(marked).coordinates(offset);
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/signatureHelp","params":{
            "textDocument":{"uri":app},"position":{"line":line,"character":character}
        }})
    };
    let open = |uri: &str, text: &str| {
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":uri,"version":1,"text":text}
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        open(
            &implementation,
            "export component Implementation inherits Rectangle { pure callback add(left: int, right: int) -> int; }",
        ),
        open(
            &library,
            "import { Implementation } from \"implementation.xr\"; export component Math inherits Rectangle { service := Implementation {} pure callback sum <=> service.add; }",
        ),
        open(&app, marked),
        hint(2, "/*inner*/"),
        hint(3, "/*outer*/"),
        hint(4, "/*incomplete*/"),
        hint(5, "convert("),
    ]);
    let response = |id| &messages.iter().find(|message| message["id"] == id).unwrap()["result"];
    assert_eq!(
        response(1)["capabilities"]["signatureHelpProvider"]["triggerCharacters"],
        json!(["(", ","])
    );
    assert_eq!(
        response(2)["signatures"][0]["label"],
        "local(input: int, scale: int) -> int"
    );
    for id in 3..=4 {
        assert_eq!(
            response(id)["signatures"][0]["label"],
            "sum(left: int, right: int) -> int"
        );
        assert_eq!(
            response(id)["signatures"][0]["parameters"][1]["label"],
            "right: int"
        );
    }
    for id in 2..=4 {
        assert_eq!(response(id)["activeParameter"], 1);
    }
    assert!(response(5).is_null());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn import_paths_expose_document_links_and_definition_targets() {
    let root = workspace_fixture("import-links");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let library = workspace.uri(&root.join("ui 部件.xr")).unwrap();
    let font_path = root.join("body.ttf");
    std::fs::write(&font_path, []).unwrap();
    let font = workspace.uri(&font_path).unwrap();
    let app = workspace.uri(&root.join("app.xr")).unwrap();
    let source = concat!(
        "/*🙂*/ import { Card } from \"ui 部件.xr\";\r\n",
        "import \"body.ttf\";\r\n",
        "import { Missing } from \"missing.xr\";\r\n",
        "component App inherits Window { title: \"ui 部件.xr\"; }",
    );
    let start = source.find("ui 部件.xr").unwrap();
    let coords = analysis::Source::new(source);
    let (line, character) = coords.coordinates(start + 1);
    let open = |uri: &str, text: &str| {
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":uri,"version":1,"text":text}
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        open(&library, "export component Card inherits Rectangle {}"),
        open(&app, source),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/documentLink","params":{"textDocument":{"uri":app}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"textDocument/definition","params":{
            "textDocument":{"uri":app},"position":{"line":line,"character":character}
        }}),
    ]);
    let response = |id| &messages.iter().find(|message| message["id"] == id).unwrap()["result"];
    let links = response(2).as_array().unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0]["target"], library);
    assert_eq!(links[1]["target"], font);
    assert_eq!(
        links[0]["range"],
        coords.range(start, start + "ui 部件.xr".len())
    );
    assert_eq!(response(3)["uri"], library);
    assert_eq!(
        response(3)["range"]["start"],
        json!({"line":0,"character":0})
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_component_diagnostic_offers_a_versioned_quick_fix() {
    let root = workspace_fixture("import-quick-fix");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let app = workspace.uri(&root.join("app.xr")).unwrap();
    let widget = root.join("widgets.xr");
    let widget_uri = workspace.uri(&widget).unwrap();
    std::fs::write(&widget, "export component SavedName inherits Rectangle {}").unwrap();
    std::fs::write(
        root.join("private.xr"),
        "component Card inherits Rectangle {}",
    )
    .unwrap();
    std::fs::write(
        root.join("cycle.xr"),
        "import { App } from \"app.xr\"; export component Card inherits App {}",
    )
    .unwrap();
    let source = "// Comment\r\nexport component App inherits Window { card := Card {} }";
    let open = |uri: &str, text: &str| {
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":uri,"version":1,"text":text}
        }})
    };
    let initial = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":workspace.uri(&root)}});
    let library_text = "export component Card inherits Rectangle {}";
    let notifications = vec![initial, open(&widget_uri, library_text), open(&app, source)];
    let messages = exchange(notifications.clone());
    let diagnostic = messages
        .iter()
        .find(|message| message["params"]["uri"] == app)
        .unwrap()["params"]["diagnostics"][0]
        .clone();
    let start = source.rfind("Card").unwrap();
    assert_eq!(
        diagnostic["range"],
        analysis::Source::new(source).range(start, start + 4)
    );
    assert_eq!(diagnostic["data"]["missingComponent"], "Card");
    let action = |id, kind: &str| {
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/codeAction","params":{
            "textDocument":{"uri":app},"range":diagnostic["range"],"context":{"diagnostics":[diagnostic],"only":[kind]}
        }})
    };
    let mut requests = notifications;
    requests.extend([
        action(2, "quickfix"),
        action(3, "refactor"),
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
            "textDocument":{"uri":app,"version":2},"contentChanges":[{"text":source.replace("Card", "Rectangle")}]
        }}),
        action(4, "quickfix"),
    ]);
    let messages = exchange(requests);
    let response = |id| &messages.iter().find(|message| message["id"] == id).unwrap()["result"];
    let fixes = response(2).as_array().unwrap();
    assert_eq!(fixes.len(), 1, "{fixes:?}");
    assert_eq!(fixes[0]["title"], "Import Card from widgets.xr");
    let edit = &fixes[0]["edit"]["documentChanges"][0];
    assert_eq!(edit["textDocument"], json!({"uri":app,"version":1}));
    assert_eq!(
        edit["edits"][0]["newText"],
        "import { Card } from \"widgets.xr\";\r\n"
    );
    assert_eq!(response(3), &json!([]));
    assert_eq!(response(4), &json!([]));
    let updated = source.replace(
        "export component",
        &format!(
            "{}export component",
            edit["edits"][0]["newText"].as_str().unwrap()
        ),
    );
    let updated_workspace =
        completion::Workspace::new(&app, [(&*widget_uri, library_text)].into_iter()).unwrap();
    assert!(
        analysis::analyze(&app, &updated, Some(&updated_workspace))
            .diagnostics
            .is_empty()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn document_formatting_preserves_comments_strings_and_utf16_ranges() {
    let source = concat!(
        "\u{feff}// file 🙂\r\n",
        "import{Card as Tile}from\"widgets.xr\"; // keep\r\n",
        "/* block\r\n  keep 🙂 */\r\n",
        "component Demo inherits Window{property < int > count:2;\r\n",
        "Tile{width:5 px;text:\"中🙂 // /* literal */\";}\r\n",
        "if count<3:Button{clicked()=>{root.count+=-1;}}\r\n",
        "} // end 🙂",
    );
    let expected = concat!(
        "\u{feff}// file 🙂\r\n",
        "import { Card as Tile } from \"widgets.xr\"; // keep\r\n",
        "/* block\r\n  keep 🙂 */\r\n",
        "component Demo inherits Window {\r\n",
        "  property <int> count: 2;\r\n",
        "  Tile {\r\n",
        "    width: 5px;\r\n",
        "    text: \"中🙂 // /* literal */\";\r\n",
        "  }\r\n",
        "  if count < 3: Button {\r\n",
        "    clicked() => {\r\n",
        "      root.count += -1;\r\n",
        "    }\r\n",
        "  }\r\n",
        "} // end 🙂\r\n",
    );
    let open = |uri: &str, text: &str| {
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":uri,"version":1,"text":text}
        }})
    };
    let request = |id, uri: &str| {
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/formatting","params":{
            "textDocument":{"uri":uri},"options":{"tabSize":2,"insertSpaces":true}
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        open(URI, source),
        request(2, URI),
        open("untitled:formatted.xr", expected),
        request(3, "untitled:formatted.xr"),
        open(
            "untitled:incomplete.xr",
            "component Demo inherits Window { Text { text: \"missing",
        ),
        request(4, "untitled:incomplete.xr"),
    ]);
    let response = |id| &messages.iter().find(|message| message["id"] == id).unwrap()["result"];
    assert_eq!(
        response(1)["capabilities"]["documentFormattingProvider"],
        true
    );
    let edits = response(2).as_array().unwrap();
    assert_eq!(edits.len(), 1, "{response:?}", response = response(2));
    assert_eq!(edits[0]["newText"], expected);
    assert_eq!(
        edits[0]["range"]["end"],
        json!({"line":7,"character":"} // end 🙂".encode_utf16().count()})
    );
    assert_eq!(response(3), &json!([]));
    assert_eq!(response(4), &json!([]));
}

#[test]
fn formatting_keeps_xrack_grammar_and_honors_indentation_options() {
    let source = r#"struct Entry{label:string, count:int}
component Demo inherits Window{
property<[Entry]> rows:[Entry{label:"a",count:1}];
property<float> ratio:root.count%2+1.5e-2;
property<brush> ink:@linear-gradient(-90 deg,#111 0%,#fff);
button:=Button{clicked=>{Log.write(LogLevel.info,Strings.format("{{{}}}",1));}}
for row[i] in root.rows:Text{text: i>0?row.label:"🙂";}
states[active when true:{width:20 px;in{animate width{duration:2 ms;}}}]
}"#;
    let options = json!({"tabSize":8,"insertSpaces":false,"insertFinalNewline":false});
    let edits = formatting::edits(source, &options);
    let formatted = edits[0]["newText"]
        .as_str()
        .unwrap_or_else(|| panic!("{edits}"));
    assert!(
        formatted.contains("\n\tlabel: string,\n\tcount: int\n}"),
        "{formatted}"
    );
    assert!(
        formatted.contains("\n\tproperty <[Entry]> rows: [Entry { label: \"a\", count: 1 }];"),
        "{formatted}"
    );
    assert!(formatted.contains("root.count % 2 + 1.5e-2"), "{formatted}");
    assert!(
        formatted.contains("@linear-gradient(-90deg, #111 0%, #fff)"),
        "{formatted}"
    );
    assert!(
        formatted
            .contains("for row[i] in root.rows: Text {\n\t\ttext: i > 0 ? row.label : \"🙂\";"),
        "{formatted}"
    );
    assert!(
        formatted.contains("\n\tstates [\n\t\tactive when true: {"),
        "{formatted}"
    );
    assert!(
        formatted.contains(r#"Log.write(LogLevel.info, Strings.format("{{{}}}", 1));"#),
        "{formatted}"
    );
    assert!(!formatted.ends_with('\n'));
    assert_eq!(formatting::edits(formatted, &options), json!([]));
}

#[test]
fn record_update_base_supports_diagnostics_navigation_and_completion() {
    let source = r#"struct User { name: string, age: int }
component Demo inherits Window {
    property <User> original: User { name: "old", age: 27 };
    property <User> changed: User {name:"new",.. root.original};
}"#;
    let diagnostics = analysis::analyze(URI, source, None).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let edits = formatting::edits(source, &json!({}));
    let formatted = edits[0]["newText"].as_str().unwrap();
    assert!(
        formatted.contains("User { name: \"new\", ..root.original }"),
        "{formatted}"
    );
    assert_eq!(
        request_at(
            "textDocument/definition",
            URI,
            &source.replace("root.original", "root.ori|ginal"),
            None,
        ),
        location_at(
            URI,
            &source.replace("> original", "> |original"),
            "original"
        ),
    );

    for (marker, expected) in [(".. ro|", "root"), ("..root.ori|", "original")] {
        let items = complete_at(&source.replace(".. root.original};", marker));
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
    let wrong_type = source.replace("root.original", "42");
    let diagnostics = analysis::analyze(URI, &wrong_type, None).diagnostics;
    assert!(
        diagnostics
            .values()
            .flatten()
            .any(|diagnostic| diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.contains("type mismatch"))),
        "{diagnostics:?}"
    );
}

#[test]
fn callback_control_flow_supports_loop_scope_and_incomplete_branches() {
    let source = r#"struct LoopEntry { label: string }
component Demo inherits Window {
    property <[LoopEntry]> rows: [LoopEntry { label: "a" }];
    property <string> output;
    callback run(bool);
    run(go)=>{if go{for row[index] in root.rows{root.output = row.label;}}
    else{root.output = "";}}
}"#;
    let diagnostics = analysis::analyze(URI, source, None).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let edits = formatting::edits(source, &json!({}));
    let formatted = edits[0]["newText"].as_str().unwrap();
    assert!(
        formatted.contains("for row[index] in root.rows {"),
        "{formatted}"
    );
    assert!(formatted.contains("} else {"), "{formatted}");
    assert_eq!(
        request_at(
            "textDocument/definition",
            URI,
            &source.replace("row.label", "ro|w.label"),
            None
        ),
        location_at(URI, &source.replace("for row", "for |row"), "row"),
    );
    let items = complete_at(&source.replace("row.label;", "row.la|"));
    assert!(
        items.iter().any(|item| item["label"] == "label"),
        "{items:?}"
    );
    let items = complete_at(&source.replace("else{root.output = \"\";}", "el|"));
    assert!(
        items.iter().any(|item| item["label"] == "else"),
        "{items:?}"
    );
    let items = complete_at(&source.replace("root.output = row.label;", "i|"));
    for expected in ["if", "index"] {
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
}

#[test]
fn index_progress_waits_for_client_ack_and_reports_completion() {
    let root = workspace_fixture("index-progress");
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    std::fs::write(
        root.join("card.xr"),
        "export component Card inherits Rectangle {}",
    )
    .unwrap();
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "rootUri":workspace.uri(&root),"capabilities":{"window":{"workDoneProgress":true}}
    }});
    let initialized = json!({"jsonrpc":"2.0","method":"initialized","params":{}});
    let search =
        json!({"jsonrpc":"2.0","id":2,"method":"workspace/symbol","params":{"query":"Card"}});
    let messages = exchange(vec![
        initialize.clone(),
        initialized.clone(),
        json!({"jsonrpc":"2.0","id":"xrack-index-0","result":null}),
        search.clone(),
    ]);
    let create = messages
        .iter()
        .position(|message| message["method"] == "window/workDoneProgress/create")
        .unwrap();
    assert_eq!(messages[create]["params"]["token"], "xrack-index-0");
    let reports: Vec<_> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message["method"] == "$/progress")
        .collect();
    assert!(
        reports.iter().all(|(position, message)| *position > create
            && message["params"]["token"] == "xrack-index-0")
    );
    assert_eq!(
        reports.first().unwrap().1["params"]["value"]["kind"],
        "begin"
    );
    assert!(
        reports
            .iter()
            .any(|(_, message)| message["params"]["value"]["percentage"] == 100)
    );
    assert_eq!(reports.last().unwrap().1["params"]["value"]["kind"], "end");
    assert_eq!(
        messages.iter().find(|message| message["id"] == 2).unwrap()["result"][0]["name"],
        "Card"
    );

    let rejected = exchange(vec![
        initialize,
        initialized,
        json!({"jsonrpc":"2.0","id":"xrack-index-0","error":{"code":-32601,"message":"unsupported"}}),
        search,
    ]);
    assert!(
        !rejected
            .iter()
            .any(|message| message["method"] == "$/progress")
    );
    assert_eq!(
        rejected.iter().find(|message| message["id"] == 2).unwrap()["result"][0]["name"],
        "Card"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelling_an_index_request_returns_one_error_and_keeps_the_session_usable() {
    let uri = "untitled:cancel.xr";
    let text = "component Demo inherits Window { property <int> count; Text { text: \"\" + root.count; } }";
    let (line, character) = analysis::Source::new(text).coordinates(text.rfind("count").unwrap());
    let reference = |id| {
        json!({"jsonrpc":"2.0","id":id,"method":"textDocument/references","params":{
            "textDocument":{"uri":uri},"position":{"line":line,"character":character},"context":{"includeDeclaration":true}
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":uri,"version":1,"text":text}
        }}),
        reference("cancel-me"),
        json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":"cancel-me"}}),
        reference("keep-me"),
    ]);
    let cancelled: Vec<_> = messages
        .iter()
        .filter(|message| message["id"] == "cancel-me")
        .collect();
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0]["error"]["code"], -32800);
    let completed = messages
        .iter()
        .find(|message| message["id"] == "keep-me")
        .unwrap();
    assert_eq!(completed["result"].as_array().unwrap().len(), 2);
}

#[test]
fn consecutive_edits_publish_the_latest_version_before_an_index_query() {
    let uri = "untitled:edit-burst.xr";
    let first = "component Demo inherits Window { property <int> count; Text { text: \"\" + root.count; } }";
    let final_text = format!("\n{}", first.replace("count", "total"));
    let (line, character) =
        analysis::Source::new(&final_text).coordinates(final_text.rfind("total").unwrap());
    let change = |version, text: &str| {
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
            "textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]
        }})
    };
    let messages = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":uri,"version":1,"text":first}
        }}),
        change(2, "component Demo inherits Window {"),
        change(3, &final_text),
        change(2, first),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/references","params":{
            "textDocument":{"uri":uri},"position":{"line":line,"character":character},"context":{"includeDeclaration":true}
        }}),
    ]);
    let versions: Vec<_> = messages
        .iter()
        .filter(|message| message["method"] == "textDocument/publishDiagnostics")
        .map(|message| message["params"]["version"].as_i64().unwrap())
        .collect();
    assert_eq!(versions, [1, 3]);
    let references = &messages.iter().find(|message| message["id"] == 2).unwrap()["result"];
    assert_eq!(references.as_array().unwrap().len(), 2);
    assert!(
        references
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["range"]["start"]["line"] == 1)
    );
}

#[test]
fn virtual_item_delegate_completes_its_action_binding() {
    let items = complete_at(
        "component App inherits Window { run := Action {} ListView { for label in [\"A\"]: ItemDelegate { | } } }",
    );
    assert!(items.iter().any(|item| item["label"] == "action"));
}

#[test]
fn media_player_completion_and_seek_signature() {
    let receiver = "component App inherits Window { movie := MediaPlayer {} Button { clicked => { movie.| } } }";
    let items = complete_at(receiver);
    for expected in [
        "seek",
        "pause",
        "play",
        "stop",
        "duration",
        "position",
        "ready",
        "status",
        "seekable",
        "live",
        "seek-start",
        "seek-end",
        "at-live-edge",
        "go-live",
    ] {
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{expected}: {items:?}"
        );
    }
    let binding = complete_at("component App inherits Window { MediaPlayer { | } }");
    assert!(binding.iter().any(|item| item["label"] == "volume"));
    assert!(
        binding
            .iter()
            .any(|item| item["label"] == "network-bandwidth")
    );
    assert!(!binding.iter().any(|item| item["label"] == "duration"));
    assert!(!binding.iter().any(|item| item["label"] == "seek-start"));
    let line =
        "component App inherits Window { movie := MediaPlayer {} Button { clicked => { movie.seek(";
    let signature = completion::signature_help(line, (0, line.len()), None);
    assert_eq!(signature["signatures"][0]["label"], "seek(position: float)");
    let line = line.replace("movie.seek(", "movie.go-live(");
    let signature = completion::signature_help(&line, (0, line.len()), None);
    assert_eq!(signature["signatures"][0]["label"], "go-live()");
}

#[test]
fn sound_effect_completes_its_bindings_and_playback_signatures() {
    let root = complete_at("component App inherits Window { chime := Sou| }");
    assert!(root.iter().any(|item| item["label"] == "SoundEffect"));
    for invalid in [
        "component App inherits Sou|",
        "component App inherits Window { Sou| }",
        "component App inherits Window { VerticalLayout { chime := Sou| } }",
        "component App inherits Window { if true: chime := Sou| }",
    ] {
        assert!(
            !complete_at(invalid)
                .iter()
                .any(|item| item["label"] == "SoundEffect")
        );
    }
    let bindings = complete_at("component App inherits Window { chime := SoundEffect { | } }");
    for expected in ["source", "volume", "muted", "loops"] {
        assert!(
            bindings.iter().any(|item| item["label"] == expected),
            "{expected}: {bindings:?}"
        );
    }
    for unavailable in [
        "ready",
        "status",
        "loops-remaining",
        "width",
        "autoplay",
        "Button",
    ] {
        assert!(!bindings.iter().any(|item| item["label"] == unavailable));
    }
    let items = complete_at(
        "component App inherits Window { chime := SoundEffect {} button := Button { clicked => { chime.| } } }",
    );
    for expected in [
        "play",
        "stop",
        "ready",
        "playing",
        "status",
        "error",
        "loops-remaining",
    ] {
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{expected}: {items:?}"
        );
    }
    assert!(!items.iter().any(|item| item["label"] == "seek"));
    let line = "component App inherits Window { chime := SoundEffect {} button := Button { clicked => { chime.play(";
    let signature = completion::signature_help(line, (0, line.len()), None);
    assert_eq!(signature["signatures"][0]["label"], "play()");
    let line = line.replace("chime.play(", "chime.stop(");
    let signature = completion::signature_help(&line, (0, line.len()), None);
    assert_eq!(signature["signatures"][0]["label"], "stop()");
}

#[test]
fn combo_box_completes_submission_rules_and_read_only_status() {
    let items = complete_at("component App inherits Window { ComboBox { | } }");
    for property in [
        "input-type",
        "minimum-length",
        "maximum-length",
        "submit-when",
        "minimum-value",
        "maximum-value",
        "decimals",
        "decimal-separator",
    ] {
        assert!(
            items.iter().any(|item| item["label"] == property),
            "{property}"
        );
    }
    assert!(!items.iter().any(|item| item["label"] == "acceptable-input"));
    let status = complete_at(
        "component App inherits Window { amount := ComboBox {} Button { enabled: amount.acceptable-|",
    );
    assert!(
        status
            .iter()
            .any(|item| item["label"] == "acceptable-input"),
        "{status:?}"
    );
}

#[test]
fn combo_box_completes_effective_selection_and_editor_state() {
    let items = complete_at(
        "component App inherits Window { choice := ComboBox {} Text { text: choice.| } }",
    );
    for (name, detail) in [
        ("count", "effective int"),
        ("current-index", "effective int"),
        ("current-text", "effective string"),
        ("current-value", "effective value · model value-role"),
        ("edit-text", "effective string"),
    ] {
        assert!(
            items
                .iter()
                .any(|item| item["label"] == name && item["detail"] == detail),
            "{name}: {items:?}"
        );
    }
}

#[test]
fn editing_finished_completion_offers_the_typed_handler() {
    for kind in ["TextInput", "LineEdit", "TextEdit", "TextArea", "ComboBox"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ editing-f| }} }}"
        ));
        let handler = items
            .iter()
            .find(|item| item["label"] == "editing-finished")
            .unwrap();
        assert_eq!(handler["detail"], "event handler (string, bool)");
    }
    let items = complete_at("component App inherits Window { Rectangle { editing-f| } }");
    assert!(!items.iter().any(|item| item["label"] == "editing-finished"));
}

#[test]
fn expander_offers_the_boolean_property_and_expansion_handler() {
    let items = complete_at("component App inherits Window { detail := Expander { expanded| } }");
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "expanded" && item["detail"] == "bool")
    );
    assert!(items.iter().any(|item| item["label"] == "expanded-changed"));
}

#[test]
fn time_field_helpers_and_popup_bounds_are_available_in_lsp() {
    for (prefix, name) in [
        ("par", "parse-iso"),
        ("is-", "is-valid-iso"),
        ("for", "format-iso"),
        ("in-", "in-range"),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ out property <bool> allowed: Times.{prefix}| }}"
        ));
        assert!(items.iter().any(|item| item["label"] == name), "{items:?}");
    }
    let items = complete_at("component App inherits Window { TimePickerPopup { maximum-t| } }");
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "maximum-time" && item["detail"] == "time")
    );
}

#[test]
fn date_field_builtins_complete_under_dates() {
    for (prefix, name) in [("par", "parse-iso"), ("is-s", "is-selectable")] {
        let items = complete_at(&format!(
            "component App inherits Window {{ out property <bool> allowed: Dates.{prefix}| }}"
        ));
        assert!(items.iter().any(|item| item["label"] == name), "{items:?}");
    }
}

#[test]
fn date_range_properties_and_day_cell_inputs_complete_with_types() {
    let items = complete_at("component App inherits Window { MonthGrid { highlight-| } }");
    for name in ["highlight-start", "highlight-end"] {
        assert!(
            items
                .iter()
                .any(|item| item["label"] == name && item["detail"] == "date"),
            "{items:?}"
        );
    }
    let items =
        complete_at("component App inherits Window { MonthGrid { DayCell { visible: cell-| } } }");
    for name in ["cell-in-range", "cell-range-start", "cell-range-end"] {
        assert!(items.iter().any(|item| item["label"] == name), "{items:?}");
    }
}

#[test]
fn calendar_browsing_offers_typed_month_functions_and_grid_mode() {
    let items =
        complete_at("component App inherits Window { out property <[Date]> months: Dates.mon| }");
    for name in ["months", "month-index"] {
        assert!(items.iter().any(|item| item["label"] == name), "{items:?}");
    }
    let items = complete_at("component App inherits Window { MonthGrid { fixed-| } }");
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "fixed-month" && item["detail"] == "bool")
    );
}

#[test]
fn only_date_selection_controls_offer_disabled_dates_and_weekdays() {
    for (kind, expected) in [
        ("Calendar", true),
        ("MonthGrid", true),
        ("DatePickerPopup", true),
        ("DayOfWeekRow", false),
        ("WeekNumberColumn", false),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ disabled-| }} }}"
        ));
        for (name, detail) in [
            ("disabled-weekdays", "intlist"),
            ("disabled-dates", "datelist"),
        ] {
            let item = items.iter().find(|item| item["label"] == name);
            assert_eq!(item.is_some(), expected, "{kind}: {items:?}");
            if let Some(item) = item {
                assert_eq!(item["detail"], detail);
            }
        }
    }
}

#[test]
fn editor_insets_and_placeholder_colors_complete_with_correct_types() {
    for kind in [
        "TextInput",
        "LineEdit",
        "TextArea",
        "SearchField",
        "ComboBox",
    ] {
        let padded = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ padding-| }} }}"
        ));
        for side in ["left", "right", "top", "bottom"] {
            let property = format!("padding-{side}");
            assert!(
                padded
                    .iter()
                    .any(|item| item["label"] == property && item["detail"] == "length"),
                "{kind}: {padded:?}"
            );
        }
        let ink = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ placeholder-text-c| }} }}"
        ));
        assert!(
            ink.iter()
                .any(|item| item["label"] == "placeholder-text-color" && item["detail"] == "color"),
            "{kind}: {ink:?}"
        );
    }
    let invalid =
        complete_at("component App inherits Window { Rectangle { placeholder-text-c| } }");
    assert!(
        !invalid
            .iter()
            .any(|item| item["label"] == "placeholder-text-color")
    );
}

#[test]
fn password_presentation_is_typed_and_limited_to_single_line_editors() {
    for (kind, available) in [
        ("TextInput", true),
        ("LineEdit", true),
        ("ComboBox", true),
        ("TextArea", false),
        ("SearchField", false),
        ("Rectangle", false),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ password-| }} }}"
        ));
        for (property, ty) in [
            ("password-character", "string"),
            ("password-mask-delay", "int"),
        ] {
            assert_eq!(
                items
                    .iter()
                    .any(|item| item["label"] == property && item["detail"] == ty),
                available,
                "{kind} {property}: {items:?}"
            );
        }
    }
}

#[test]
fn single_line_text_editors_complete_the_string_input_mask() {
    for (kind, available) in [
        ("TextInput", true),
        ("LineEdit", true),
        ("TextArea", false),
        ("ComboBox", true),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ input-m| }} }}"
        ));
        assert_eq!(
            items
                .iter()
                .any(|item| item["label"] == "input-mask" && item["detail"] == "string"),
            available,
            "{items:?}",
        );
    }
}

#[test]
fn keyboard_assistance_completes_and_checks_editor_properties() {
    for source in [
        "component App inherits Window { TextArea { keyboard-capitalization: |",
        "component App inherits Window { LineEdit { keyboard-capitalization: KeyboardCapitalization.|",
    ] {
        let values = complete_at(source);
        for mode in ["auto", "none", "words", "sentences", "characters"] {
            assert!(
                values.iter().any(|item| item["label"] == mode),
                "{values:?}"
            );
        }
    }
    let props = complete_at("component App inherits Window { SearchField { |");
    for name in ["auto-correct", "spell-check"] {
        assert!(
            props
                .iter()
                .any(|item| item["label"] == name && item["detail"] == "bool"),
            "{props:?}"
        );
    }
    let source = "component App inherits Window { TextArea { keyboard-capitalization: words; auto-correct: false; spell-check: true; } }";
    let report = analysis::analyze(URI, source, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = source.replace("TextArea", "Rectangle");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(report[URI].iter().any(|item| {
        item["message"]
            .as_str()
            .is_some_and(|message| message.contains("keyboard-capitalization"))
    }));
}

#[test]
fn keyboard_navigation_completes_only_on_text_editors() {
    for (kind, available) in [
        ("TextArea", true),
        ("SearchField", true),
        ("Rectangle", false),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ keyboard-n|"
        ));
        assert_eq!(
            items
                .iter()
                .any(|item| item["label"] == "keyboard-navigation" && item["detail"] == "bool"),
            available,
            "{items:?}"
        );
    }

    let source = "component App inherits Window { TextArea { keyboard-navigation: true; } }";
    let report = analysis::analyze(URI, source, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = source.replace("true", "3");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(!report[URI].is_empty());
}

#[test]
fn return_key_hints_complete_with_or_without_enum_qualification() {
    for source in [
        "component App inherits Window { LineEdit { return-key: |",
        "component App inherits Window { TextInput { return-key: ReturnKey.|",
    ] {
        let values = complete_at(source);
        for mode in ["auto", "next", "done"] {
            assert!(
                values.iter().any(|item| item["label"] == mode),
                "{values:?}"
            );
        }
    }
    let valid = "component App inherits Window { LineEdit { return-key: ReturnKey.next; } }";
    let report = analysis::analyze(URI, valid, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = valid.replace("LineEdit", "TextArea");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(report[URI].iter().any(|item| {
        item["message"]
            .as_str()
            .is_some_and(|message| message.contains("return-key"))
    }));
}

#[test]
fn accessible_heading_level_completes_only_on_text_and_label() {
    for (kind, expected) in [("Text", true), ("Label", true), ("Button", false)] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ accessible-h|"
        ));
        assert_eq!(
            items.iter().any(|item| {
                item["label"] == "accessible-heading-level" && item["detail"] == "int"
            }),
            expected
        );
    }
    let source = "component App inherits Window { Label { accessible-heading-level: 2; } }";
    let report = analysis::analyze(URI, source, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = source.replace("2", "true");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(!report[URI].is_empty());
}

#[test]
fn accessible_live_completion_is_scoped_to_plain_text() {
    for (kind, expected) in [("Text", true), ("Label", true), ("Button", false)] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ accessible-l|"
        ));
        assert_eq!(
            items.iter().any(|item| item["label"] == "accessible-live"),
            expected
        );
    }
    for value in ["|", "AccessibleLive.|"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ Label {{ accessible-live: {value}"
        ));
        for priority in ["off", "polite", "assertive"] {
            assert!(items.iter().any(|item| item["label"] == priority));
        }
    }
    let source = "component App inherits Window { Label { accessible-live: polite; } }";
    let report = analysis::analyze(URI, source, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = source.replace("polite", "urgent");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(!report[URI].is_empty());
}

#[test]
fn drag_auto_scroll_completes_only_on_list_and_grid_views() {
    for (kind, expected) in [("ListView", true), ("GridView", true), ("DragArea", false)] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ drag-auto-|"
        ));
        assert_eq!(
            items
                .iter()
                .any(|item| item["label"] == "drag-auto-scroll" && item["detail"] == "bool"),
            expected
        );
    }
}

#[test]
fn touch_drag_delay_completes_only_on_drag_sources() {
    for (kind, expected) in [("DragArea", true), ("DropArea", false), ("ListView", false)] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ touch-drag-|"
        ));
        assert_eq!(
            items
                .iter()
                .any(|item| { item["label"] == "touch-drag-delay" && item["detail"] == "int" }),
            expected
        );
    }
    let source = "component App inherits Window { DragArea { touch-drag-delay: 420; } }";
    let report = analysis::analyze(URI, source, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = source.replace("420", "true");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(!report[URI].is_empty());
}

#[test]
fn keyboard_dismissal_on_drag_completes_only_on_viewports() {
    for (kind, expected) in [
        ("ScrollView", true),
        ("Flickable", true),
        ("ListView", true),
        ("LineEdit", false),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ dismiss-key|"
        ));
        assert_eq!(
            items.iter().any(|item| {
                item["label"] == "dismiss-keyboard-on-drag" && item["detail"] == "bool"
            }),
            expected
        );
    }
    let source = "component App inherits Window { ScrollView { dismiss-keyboard-on-drag: true; } }";
    let report = analysis::analyze(URI, source, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = source.replace("true", "\"true\"");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(!report[URI].is_empty());
}

#[test]
fn pull_to_refresh_completion_and_diagnostics_follow_scrollable_elements() {
    for kind in ["ScrollView", "ListView", "Flickable"] {
        let props = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ pull-to-r|"
        ));
        assert!(
            props
                .iter()
                .any(|item| item["label"] == "pull-to-refresh" && item["detail"] == "bool")
        );
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ refre|"
        ));
        assert!(
            items
                .iter()
                .any(|item| item["label"] == "refresh-requested")
        );
        assert!(
            items
                .iter()
                .any(|item| item["label"] == "refreshing" && item["detail"] == "bool")
        );
    }
    let items = complete_at("component App inherits Window { Button { refre|");
    assert!(
        !items
            .iter()
            .any(|item| item["label"] == "refresh-requested")
    );
    let valid = r#"component App inherits Window {
        list := ListView {
            pull-to-refresh: true;
            refresh-requested() => {}
            for row in ["item"] : Label { text: row; }
        }
    }"#;
    let report = analysis::analyze(URI, valid, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
}

#[test]
fn cache_rendering_hint_completes_and_checks_visual_properties() {
    let props = complete_at("component App inherits Window { Rectangle { cache-r|");
    assert!(
        props
            .iter()
            .any(|item| item["label"] == "cache-rendering-hint" && item["detail"] == "bool")
    );
    let valid = r#"
        component Panel inherits Rectangle {}
        component App inherits Window {
            Panel { cache-rendering-hint: true; }
        }
    "#;
    let report = analysis::analyze(URI, valid, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    for invalid in [
        "component App inherits Window { Rectangle { cache-rendering-hint: 5; } }",
        "component App inherits Window { Timer { cache-rendering-hint: true; } }",
    ] {
        let report = analysis::analyze(URI, invalid, None).diagnostics;
        assert!(report.get(URI).is_some_and(|items| !items.is_empty()));
    }
}

#[test]
fn repeated_styled_text_links_complete_and_validate_as_strings() {
    let completions = complete_at(
        "component App inherits Window { for title in [\"path\"] : StyledText { link-act|",
    );
    assert!(completions.iter().any(|item| {
        item["label"] == "link-activated" && item["detail"] == "event handler (string)"
    }));
    let valid = r#"component App inherits Window {
        out property <string> target;
        for title in ["file"] : StyledText {
            text: @markdown("[file](src/main.rs)");
            link-activated(path) => { root.target = title + path; }
        }
    }"#;
    let diagnostics = analysis::analyze(URI, valid, None).diagnostics;
    assert!(
        diagnostics.get(URI).is_none_or(Vec::is_empty),
        "{diagnostics:?}"
    );
    let invalid = valid.replace("<string>", "<int>");
    let diagnostics = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(!diagnostics[URI].is_empty());
}

#[test]
fn file_drop_failure_handler_completes_with_a_string_argument() {
    let items = complete_at("component App inherits Window { DropArea { files-f|");
    assert!(items.iter().any(|item| {
        item["label"] == "files-failed" && item["detail"] == "event handler (string)"
    }));
    let items = complete_at("component App inherits Window { Button { files-f|");
    assert!(!items.iter().any(|item| item["label"] == "files-failed"));
    let valid = r#"component App inherits Window {
        out property <string> message;
        inbox := DropArea {
            accept-files: true;
            files-failed(reason) => { root.message = reason; }
        }
    }"#;
    let report = analysis::analyze(URI, valid, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = valid.replace("<string>", "<[string]>");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(!report[URI].is_empty());
}

#[test]
fn input_purpose_completion_and_diagnostics_follow_single_line_editors() {
    let property = complete_at("component App inherits Window { LineEdit { input-pu|");
    assert!(
        property
            .iter()
            .any(|item| item["label"] == "input-purpose" && item["detail"] == "inputpurpose")
    );
    for source in [
        "component App inherits Window { LineEdit { input-purpose: |",
        "component App inherits Window { ComboBox { editable: true; input-purpose: InputPurpose.|",
    ] {
        let values = complete_at(source);
        for purpose in [
            "auto",
            "username",
            "current-password",
            "new-password",
            "one-time-code",
        ] {
            assert!(
                values.iter().any(|item| item["label"] == purpose),
                "{source}: {values:?}"
            );
        }
    }
    let valid = "component App inherits Window { LineEdit { input-type: email; input-purpose: username; } }";
    let report = analysis::analyze(URI, valid, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
    let invalid = valid
        .replace("LineEdit", "TextArea")
        .replace("input-type: email;", "");
    let report = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(report[URI].iter().any(|item| {
        item["message"]
            .as_str()
            .is_some_and(|message| message.contains("input-purpose"))
    }));
}

#[test]
fn contact_input_modes_complete_in_both_enum_forms() {
    for source in [
        "component App inherits Window { LineEdit { input-type: |",
        "component App inherits Window { TextInput { input-type: InputType.|",
        "component App inherits Window { ComboBox { editable: true; input-type: |",
    ] {
        let values = complete_at(source);
        for mode in ["email", "url", "tel"] {
            assert!(
                values.iter().any(|item| item["label"] == mode),
                "{source}: {values:?}"
            );
        }
    }
    let source = "component App inherits Window { LineEdit { input-type: email; } TextInput { input-type: InputType.url; } LineEdit { input-type: tel; } }";
    let report = analysis::analyze(URI, source, None).diagnostics;
    assert!(report.get(URI).is_none_or(Vec::is_empty), "{report:?}");
}

#[test]
fn input_validity_is_completed_as_a_read_only_enum() {
    let property = complete_at(
        "component App inherits Window { editor := LineEdit {} out property <InputValidity> status: editor.input-v| }",
    );
    assert!(property.iter().any(
        |item| item["label"] == "input-validity" && item["detail"] == "read-only InputValidity"
    ));
    let values = complete_at(
        "component App inherits Window { in property <InputValidity> status: InputValidity.| }",
    );
    for expected in ["invalid", "intermediate", "acceptable"] {
        assert!(
            values.iter().any(|item| item["label"] == expected),
            "{values:?}"
        );
    }
    let binding = complete_at("component App inherits Window { TextArea { input-v| } }");
    assert!(!binding.iter().any(|item| item["label"] == "input-validity"));
}

#[test]
fn pattern_validation_completes_the_binding_and_reports_bad_patterns() {
    for kind in ["TextInput", "LineEdit", "TextArea", "TextEdit", "ComboBox"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ input-pa| }} }}"
        ));
        assert!(
            items
                .iter()
                .any(|item| item["label"] == "input-pattern" && item["detail"] == "string"),
            "{kind}: {items:?}"
        );
    }
    let source = r#"component App inherits Window { LineEdit { input-pattern: "["; } }"#;
    let report = analysis::analyze(URI, source, None).diagnostics;
    assert!(report[URI].iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .is_some_and(|message| message.contains("invalid input-pattern"))
    }));
}

#[test]
fn text_editing_commands_complete_with_typed_arguments() {
    let prefix = "component App inherits Window { editor := LineEdit {} button := Button { clicked => { editor.";
    let items = complete_at(&format!("{prefix}replace-t|"));
    assert!(items.iter().any(|item| item["label"] == "replace-text"));
    let line = format!("{prefix}replace-text(0, 3, ");
    let signature = completion::signature_help(&line, (0, line.len()), None);
    assert_eq!(
        signature["signatures"][0]["label"],
        "replace-text(start: int, end: int, text: string)"
    );
    let rich = complete_at(&format!("{prefix}replace-t|").replace("LineEdit", "StyledText"));
    assert!(!rich.iter().any(|item| item["label"] == "replace-text"));
}

#[test]
fn selection_colors_complete_as_colors_on_text_selection_controls() {
    for (kind, supported) in [
        ("LineEdit", true),
        ("TextArea", true),
        ("SearchField", true),
        ("ComboBox", true),
        ("StyledText", true),
        ("Label", false),
    ] {
        for property in ["selection-color", "selected-text-color"] {
            let items = complete_at(&format!(
                "component App inherits Window {{ {kind} {{ {property}| }} }}"
            ));
            assert_eq!(
                items
                    .iter()
                    .any(|item| item["label"] == property && item["detail"] == "color"),
                supported,
                "{kind}.{property}: {items:?}",
            );
        }
    }
}

#[test]
fn styled_text_completes_read_only_selection_operations() {
    let props = complete_at("component App inherits Window { StyledText { selecta| } }");
    assert!(
        props
            .iter()
            .any(|item| item["label"] == "selectable" && item["detail"] == "bool")
    );
    let methods = complete_at(
        "component App inherits Window { doc := StyledText {} button := Button { clicked => { doc.|",
    );
    for expected in ["select-word", "copy", "selected-text"] {
        assert!(
            methods.iter().any(|item| item["label"] == expected),
            "{expected}: {methods:?}"
        );
    }
    assert!(
        !methods
            .iter()
            .any(|item| item["label"] == "cut" || item["label"] == "paste")
    );
}

#[test]
fn persistent_selection_completes_on_text_editors() {
    for (kind, supported) in [
        ("TextArea", true),
        ("LineEdit", true),
        ("ComboBox", true),
        ("Label", false),
    ] {
        let choices = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ persistent-sel| }} }}"
        ));
        assert_eq!(
            choices
                .iter()
                .any(|item| item["label"] == "persistent-selection" && item["detail"] == "bool"),
            supported
        );
    }
}

#[test]
fn editors_offer_overwrite_and_select_word_with_signature() {
    let properties = complete_at("component Demo inherits Window { TextArea { overwr| } }");
    assert!(
        properties
            .iter()
            .any(|item| item["label"] == "overwrite-mode" && item["detail"] == "bool")
    );
    let other = complete_at("component Demo inherits Window { Button { overwr| } }");
    assert!(!other.iter().any(|item| item["label"] == "overwrite-mode"));
    let line = "component Demo inherits Window { editor := TextEdit {} Button { clicked => { editor.select-w";
    let items = complete_at(&format!("{line}|"));
    assert!(items.iter().any(|item| item["label"] == "select-word"));
    let call = format!("{line}ord(");
    let help = completion::signature_help(&call, (0, call.len()), None);
    assert_eq!(help["signatures"][0]["label"], "select-word()");
}

#[test]
fn text_editors_complete_mouse_selection_modes() {
    for kind in [
        "TextInput",
        "LineEdit",
        "TextEdit",
        "TextArea",
        "SearchField",
        "ComboBox",
        "Button",
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ mouse-sel| }} }}"
        ));
        assert_eq!(
            items
                .iter()
                .any(|item| item["label"] == "mouse-selection-mode"),
            kind != "Button",
            "{kind}: {items:?}"
        );
    }
    for qualifier in ["", "MouseSelectionMode."] {
        let variants = complete_at(&format!(
            "component App inherits Window {{ LineEdit {{ mouse-selection-mode: {qualifier}| }} }}"
        ));
        for variant in ["characters", "words", "caret"] {
            assert!(
                variants.iter().any(|item| item["label"] == variant),
                "{variants:?}"
            );
        }
    }
}

#[test]
fn text_submission_completes_rules_and_read_only_status() {
    let items = complete_at("component App inherits Window { entry := LineEdit { | } }");
    for property in ["minimum-length", "submit-when"] {
        assert!(
            items.iter().any(|item| item["label"] == property),
            "{items:?}"
        );
    }
    assert!(!items.iter().any(|item| item["label"] == "acceptable-input"));

    let items = complete_at(
        "component App inherits Window { entry := LineEdit {} Button { enabled: entry.| } }",
    );
    let status = items
        .iter()
        .find(|item| item["label"] == "acceptable-input")
        .unwrap();
    assert!(
        status["detail"]
            .as_str()
            .unwrap()
            .contains("read-only bool")
    );

    let items = complete_at("component App inherits Window { Rectangle { | } }");
    assert!(!items.iter().any(|item| item["label"] == "submit-when"));
}

#[test]
fn single_line_editors_complete_numeric_validation_bindings() {
    let items = complete_at("component App inherits Window { TextInput { | } }");
    for (property, ty) in [
        ("minimum-value", "float"),
        ("maximum-value", "float"),
        ("decimals", "int"),
    ] {
        let binding = items.iter().find(|item| item["label"] == property).unwrap();
        assert_eq!(binding["detail"], ty);
    }
    let items = complete_at("component App inherits Window { TextArea { | } }");
    assert!(
        !items
            .iter()
            .any(|item| item["label"] == "minimum-value" || item["label"] == "decimals")
    );
}

#[test]
fn editor_selection_completions_offer_read_only_offsets_and_text() {
    let items = complete_at(
        "component App inherits Window { entry := LineEdit {} Text { text: entry.| } }",
    );
    for (property, ty) in [
        ("cursor-offset", "int"),
        ("selection-start-offset", "int"),
        ("selection-end-offset", "int"),
        ("selected-text", "string"),
    ] {
        let item = items.iter().find(|item| item["label"] == property).unwrap();
        assert_eq!(item["detail"], format!("read-only {ty}"));
    }
    let items = complete_at("component App inherits Window { LineEdit { | } }");
    assert!(!items.iter().any(|item| item["label"] == "selected-text"));
}

#[test]
fn editor_command_availability_completes_as_read_only() {
    let items = complete_at(
        "component App inherits Window { entry := TextEdit {} Button { enabled: entry.| } }",
    );
    for property in ["can-undo", "can-redo", "can-copy", "can-cut"] {
        let item = items.iter().find(|item| item["label"] == property).unwrap();
        assert_eq!(item["detail"], "read-only bool");
    }
    let items = complete_at("component App inherits Window { TextEdit { | } }");
    assert!(!items.iter().any(|item| item["label"] == "can-undo"));
}

#[test]
fn virtual_views_complete_source_and_projected_row_counts_as_read_only() {
    let items = complete_at(
        "component App inherits Window { rows := ListView { for item in 1 : Text {} } Label { text: rows.| } }",
    );
    for property in ["row-count", "source-row-count"] {
        let item = items.iter().find(|item| item["label"] == property).unwrap();
        assert_eq!(item["detail"], "read-only int");
    }
    let items = complete_at("component App inherits Window { ListView { | } }");
    assert!(!items.iter().any(|item| item["label"] == "row-count"));
}

#[test]
fn empty_state_is_suggested_inside_data_views() {
    for kind in [
        "ListView",
        "GridView",
        "StandardListView",
        "TableView",
        "StandardTableView",
        "TreeView",
    ] {
        let source = format!("component App inherits Window {{ {kind} {{ | }} }}");
        let items = complete_at(&source);
        assert!(
            items.iter().any(|item| item["label"] == "EmptyState"),
            "{kind}: {items:?}"
        );
    }
    let items = complete_at("component App inherits Window { Rectangle { | } }");
    assert!(!items.iter().any(|item| item["label"] == "EmptyState"));
}

#[test]
fn list_selection_key_bindings_are_available_on_the_three_list_controls() {
    for kind in ["ListView", "GridView", "StandardListView"] {
        let completion = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ | }} }}"
        ));
        for (name, ty) in [("row-key-field", "string"), ("current-key", "int")] {
            let item = completion
                .iter()
                .find(|item| item["label"] == name)
                .unwrap();
            assert_eq!(item["detail"], ty);
        }
    }
}

#[test]
fn keyed_navigation_completes_and_shows_scroll_signature() {
    let members = complete_at(
        "component App inherits Window { rows := GridView {} Button { clicked => { rows.| } } }",
    );
    for name in ["selected-view-index", "select-key", "position-view-at-key"] {
        assert!(
            members.iter().any(|item| item["label"] == name),
            "{name}: {members:?}"
        );
    }
    let source = "component App inherits Window { rows := GridView {} Button { clicked => { rows.position-view-at-key(";
    let info = completion::signature_help(source, (0, source.len()), None);
    assert_eq!(
        info["signatures"][0]["label"],
        "position-view-at-key(key: int, alignment: ScrollAlignment)"
    );
}

#[test]
fn layout_direction_completes_direction_names_for_nested_bindings() {
    let items = complete_at("component App inherits Window { HorizontalLayout { | } }");
    let item = items
        .iter()
        .find(|item| item["label"] == "layout-direction")
        .unwrap();
    assert_eq!(item["detail"], "layoutdirection");
    let items = complete_at("component App inherits Window { layout-direction: | }");
    for name in ["left-to-right", "right-to-left"] {
        assert!(
            items.iter().any(|item| item["label"] == name),
            "{name}: {items:?}"
        );
    }
}

#[test]
fn editors_offer_logical_and_physical_horizontal_alignment() {
    let items =
        complete_at("component App inherits Window { LineEdit { horizontal-alignment: | } }");
    for alignment in ["start", "end", "left", "center", "right"] {
        assert!(
            items.iter().any(|item| item["label"] == alignment),
            "{alignment}: {items:?}"
        );
    }
}

#[test]
fn scroll_visual_metrics_complete_as_read_only_values() {
    let items = complete_at(
        "component App inherits Window { bar := ScrollIndicator {} Label { text: bar.| } }",
    );
    for name in ["visual-position", "visual-size"] {
        assert!(items.iter().any(|item| {
            item["label"] == name
                && item["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.contains("read-only float"))
        }));
    }
    let bindings = complete_at("component App inherits Window { ScrollBar { | } }");
    assert!(!bindings.iter().any(|item| matches!(
        item["label"].as_str(),
        Some("visual-size" | "visual-position")
    )));
}

#[test]
fn scroll_artwork_completes_only_inside_scroll_controls() {
    for kind in ["ScrollBar", "ScrollIndicator"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ Scroll| }} }}"
        ));
        for element in ["ScrollTrack", "ScrollThumb"] {
            assert!(items.iter().any(|item| item["label"] == element));
        }
    }
    let unrelated = complete_at("component App inherits Window { Rectangle { Scroll| } }");
    assert!(
        !unrelated
            .iter()
            .any(|item| matches!(item["label"].as_str(), Some("ScrollTrack" | "ScrollThumb")))
    );
}

#[test]
fn scroll_interaction_completion_matches_the_scroll_control_kind() {
    for (kind, expected) in [
        ("ScrollBar", &["active", "pressed", "hovered"][..]),
        ("ScrollIndicator", &["active"][..]),
    ] {
        let values = complete_at(&format!(
            "component App inherits Window {{ bar := {kind} {{}} property <bool> state: bar.| }}"
        ));
        let bindings = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ | }} }}"
        ));
        for name in ["active", "pressed", "hovered"] {
            assert_eq!(
                values.iter().any(|item| item["label"] == name
                    && item["detail"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("read-only bool")),
                expected.contains(&name),
                "{kind}.{name}: {values:?}"
            );
            assert!(!bindings.iter().any(|item| item["label"] == name));
        }
    }
}

#[test]
fn scroll_commands_offer_targeted_completion_and_parameters() {
    for (kind, method, signature) in [
        ("ScrollBar", "increase", "increase()"),
        ("ScrollBar", "decrease", "decrease()"),
        (
            "ScrollView",
            "scroll-by",
            "scroll-by(dx: length, dy: length)",
        ),
    ] {
        let prefix = format!(
            "component App inherits Window {{ target := {kind} {{}} button := Button {{ clicked => {{ target."
        );
        assert!(
            complete_at(&format!("{prefix}| }} }} }}"))
                .iter()
                .any(|item| item["label"] == method),
            "{kind}.{method}"
        );
        let source = format!("{prefix}{method}(");
        let help = completion::signature_help(&source, (0, source.len()), None);
        assert_eq!(help["signatures"][0]["label"], signature);
    }
    let items = complete_at(
        "component App inherits Window { target := ScrollIndicator {} button := Button { clicked => { target.| } } }",
    );
    assert!(!items.iter().any(|item| matches!(
        item["label"].as_str(),
        Some("increase" | "decrease" | "scroll-by")
    )));
}

#[test]
fn scroll_view_reservation_and_read_only_bar_extent_complete_in_context() {
    let bindings = complete_at("component App inherits Window { ScrollView { | } }");
    assert!(
        bindings
            .iter()
            .any(|item| item["label"] == "reserve-scroll-bar-space" && item["detail"] == "bool")
    );
    let values = complete_at(
        "component App inherits Window { view := ScrollView {} property <length> inset: view.| }",
    );
    for metric in ["effective-scroll-bar-width", "effective-scroll-bar-height"] {
        assert!(
            values
                .iter()
                .any(|item| item["label"] == metric && item["detail"] == "read-only length")
        );
        assert!(!bindings.iter().any(|item| item["label"] == metric));
    }
    let other = complete_at("component App inherits Window { Flickable { | } }");
    assert!(
        !other
            .iter()
            .any(|item| item["label"] == "reserve-scroll-bar-space")
    );
}

#[test]
fn scroll_focus_reveal_completes_on_content_viewports() {
    for kind in ["ScrollView", "Flickable", "Rectangle"] {
        let items = complete_at(&format!(
            "component App inherits Window {{ {kind} {{ | }} }}"
        ));
        assert_eq!(
            items
                .iter()
                .any(|item| item["label"] == "auto-reveal-focus" && item["detail"] == "bool"),
            kind != "Rectangle"
        );
    }
}

#[test]
fn position_child_completes_and_resolves_only_viewport_descendants() {
    let source = r#"
        export component App inherits Window {
            view := ScrollView { Rectangle { item := Rectangle {} } }
            elsewhere := Rectangle {}
            go := Button {
                clicked => { view.position-view-at-child(item, center, contain); }
            }
        }
    "#;
    let items = complete_at(&source.replace("item, center", "|, center"));
    assert!(items.iter().any(|item| item["label"] == "item"));
    assert!(
        !items
            .iter()
            .any(|item| matches!(item["label"].as_str(), Some("view" | "elsewhere")))
    );
    let definition = request_at(
        "textDocument/definition",
        URI,
        &source.replace("child(item", "child(|item"),
        None,
    );
    assert_eq!(
        definition,
        location_at(URI, &source.replace("item :=", "|item :="), "item")
    );
    let hint = "component App inherits Window { view := ScrollView {} go := Button { clicked => { view.position-view-at-child(";
    let help = completion::signature_help(hint, (0, hint.len()), None);
    assert_eq!(
        help["signatures"][0]["label"],
        "position-view-at-child(child: element, horizontal: ScrollAlignment, vertical: ScrollAlignment)"
    );
}

#[test]
fn completes_enum_variants_and_callback_local_bindings() {
    let source = r#"enum Phase { Idle, Running }
component Demo inherits Window {
    property <Phase> phase: Phase::Idle;
    callback start();
    start => {
        let next-phase = Phase::Running;
        root.phase = next-phase;
    }
}"#;
    let diagnostics = analysis::analyze(URI, source, None).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let variants = complete_at(&source.replace("Phase::Idle", "Phase::Ru|"));
    assert!(
        variants.iter().any(|item| item["label"] == "Running"),
        "{variants:?}"
    );
    let locals = complete_at(&source.replace("= next-phase;", "= next-|"));
    assert!(
        locals.iter().any(|item| item["label"] == "next-phase"),
        "{locals:?}"
    );
    let edits = formatting::edits(&source.replace("Phase::Idle", "Phase :: Idle"), &json!({}));
    assert!(
        edits[0]["newText"]
            .as_str()
            .unwrap()
            .contains("Phase::Idle")
    );
}

#[test]
fn completes_named_slots_and_typed_function_parameters() {
    let source = r#"struct Item { title: string }
component Card inherits VerticalLayout {
    @slot(toolbar);
    pure function caption(item: Item) -> string { item.title }
}
component Demo inherits Window { Card { slot toolbar {} } }"#;
    let diagnostics = analysis::analyze(URI, source, None).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let slots = complete_at(&source.replace("slot toolbar {}", "slot to|"));
    assert!(
        slots.iter().any(|item| item["label"] == "toolbar"),
        "{slots:?}"
    );
    let fields = complete_at(&source.replace("item.title", "item.ti|"));
    assert!(
        fields.iter().any(|item| item["label"] == "title"),
        "{fields:?}"
    );
}

#[test]
fn payload_enum_completion_and_match_binding_definition() {
    let source = r#"enum Job {
    Pending,
    Failed { reason: string, retryable: bool },
    Ready(string),
}
component Demo inherits Window {
    in property <Job> work: Job::Ready("ok");
    out property <string> status: match root.work {
        Pending => "pending",
        Failed { reason: message, retryable: _ } => message,
        Ready(text) => text,
    };
}"#;
    let diagnostics = analysis::analyze(URI, source, None).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let items = complete_at(&source.replace("Job::Ready", "Job::|"));
    for name in ["Pending", "Failed", "Ready"] {
        assert!(items.iter().any(|item| item["label"] == name), "{items:?}");
    }
    for field in ["reason", "retryable", "string", "bool"] {
        assert!(
            !items.iter().any(|item| item["label"] == field),
            "{items:?}"
        );
    }
    let uri = "untitled:enum-payload.xr";
    let offset = source.find("=> message").unwrap() + 4;
    let (line, character) = analysis::Source::new(source).coordinates(offset);
    let replies = exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
            "textDocument":{"uri":uri,"version":1,"text":source}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{
            "textDocument":{"uri":uri},"position":{"line":line,"character":character}}}),
    ]);
    let reply = replies.iter().find(|reply| reply["id"] == 2).unwrap();
    assert_eq!(
        reply["result"],
        location_at(
            uri,
            &source.replace("reason: message", "reason: |message"),
            "message"
        ),
    );
}

#[test]
fn custom_render_completes_renderer_inputs() {
    let items = complete_at("component App inherits Window { CustomRender { | } }");
    for name in ["renderer", "data", "revision"] {
        assert!(items.iter().any(|item| item["label"] == name), "{items:?}");
    }
    let source = r#"component App inherits Window {
        CustomRender { renderer: "waveform"; data: "samples"; revision: 1; }
    }"#;
    let diagnostics = analysis::analyze(URI, source, None).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn model_lookup_completes_with_a_literal_field_path() {
    let source = r#"struct Entry { owner: string, label: string }
        export component Demo inherits Window {
            in property <[Entry]> rows: [];
            out property <[Entry]> selected: Model.lookup(root.rows, "owner", "group");
        }"#;
    let items = complete_at(&source.replace("Model.lookup", "Model.loo|"));
    assert!(
        items.iter().any(|item| item["label"] == "lookup"),
        "{items:?}"
    );
    let diagnostics = analysis::analyze(URI, source, None).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn gltf_material_variant_selectors_complete_and_hover_inside_scene_rows() {
    let directory = workspace_fixture("material-variant-selectors");
    std::fs::write(
        directory.join("sample.gltf"),
        r##"{"asset":{"version":"2.0"},"extensions":{"KHR_materials_variants":{"variants":[
            {"name":"Copper"},{"name":"Cool Silver"},{"name":"Repeat"},{"name":"Repeat"},{"name":"#1"}
        ]}}}"##,
    )
    .unwrap();
    let file = directory.join("app.xr");
    let uri = workspace_file_uri(&file);
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let prefix = r#"component App inherits Window { property <Actor> model: Actor {
        mesh: @mesh-url("sample.gltf", auto-lod),
        material-variant: ""#;
    let complete = |source: &str, cursor: usize| {
        let (line, column) = analysis::Source::new(source).coordinates(cursor);
        completion::complete(source, line, column, Some(&workspace))
    };
    let candidates = complete(prefix, prefix.len());
    let labels = candidates
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        ["#0", "Copper", "#1", "Cool Silver", "#2", "#3", "#4"]
    );
    let complete_source = format!("{prefix}Co\" }}; }}");
    let candidates = complete(&complete_source, prefix.len() + 2);
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates
            .iter()
            .any(|item| item["textEdit"]["newText"] == "Cool Silver")
    );
    let asset_cursor =
        analysis::Source::new(prefix).coordinates(prefix.find("sample.gltf").unwrap());
    let hover = completion::query(
        "textDocument/hover",
        &uri,
        prefix,
        Some(asset_cursor),
        Some(&workspace),
    );
    let details = hover["contents"]["value"].as_str().unwrap();
    assert!(details.contains("glTF material variants (material-variant)"));
    assert!(details.contains("#0  Copper"));
}

#[test]
fn mesh_asset_hover_reports_gpu_instance_groups_and_accessors() {
    let directory = workspace_fixture("gltf-instanced-asset-hover");
    std::fs::write(
        directory.join("forest.gltf"),
        r#"{"asset":{"version":"2.0"},"nodes":[{},{
        "name":"Forest","extensions":{"EXT_mesh_gpu_instancing":{"attributes":{"TRANSLATION":0,"ROTATION":1}}}
        }],"accessors":[{"count":128},{"count":128}]}"#,
    )
    .unwrap();
    let uri = workspace_file_uri(&directory.join("main.xr"));
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let source =
        r#"component App inherits Window { property <string> forest: @mesh-url("forest.gltf"); }"#;
    let cursor = analysis::Source::new(source).coordinates(source.find("forest.gltf").unwrap());
    let hover = completion::query(
        "textDocument/hover",
        &uri,
        source,
        Some(cursor),
        Some(&workspace),
    );
    let details = hover["contents"]["value"].as_str().unwrap();
    assert!(details.contains("EXT_mesh_gpu_instancing"));
    assert!(details.contains("Node #1  Forest  128 instances"));
}

#[test]
fn authored_instance_prototypes_complete_from_the_same_table() {
    let source = r#"struct Actor { id: string, mesh: string, instance-of: string, instance-template: bool }
        component App inherits Window {
            property <[Actor]> other: [Actor { id: "sh-outside", mesh: "primitive:box" }];
            property <[Actor]> world: [
                Actor { id: "sh-duplicate", mesh: "primitive:box" },
                Actor { id: "shape", mesh: "primitive:box", instance-template: true },
                Actor { id: "sh-duplicate", mesh: "primitive:sphere" },
                Actor { id: "sh-empty", mesh: "" },
                Actor { id: "sh-copy", mesh: "primitive:sphere", instance-of: "shape" },
                Actor { id: "sh-self", mesh: "primitive:box", instance-of: "sh|" },
                Actor { id: "sh-asset", mesh: @mesh-url("house.glb") }
            ];
        }"#;
    let items = complete_at(source);
    let candidates = items
        .iter()
        .map(|item| item["label"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(candidates, ["sh-asset", "shape"]);
    assert_eq!(items[1]["textEdit"]["newText"], "shape");
    let partial = r#"component App inherits Window {
        property <[Actor]> rows: [Actor { id: "prototype", mesh: "primitive:box" }, Actor { instance-of: "pr|"#;
    assert!(
        complete_at(partial)
            .iter()
            .any(|item| item["label"] == "prototype")
    );
}

#[test]
fn instance_picking_completion_and_signature_expose_the_full_address() {
    let partial =
        "component App inherits Window { property <hit3d> result: Scene3D.hit-instance-c|";
    let items = complete_at(partial);
    assert!(items.iter().any(|item| {
        item["label"] == "hit-instance-camera"
            && item["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("-> hit3d"))
    }));

    let call = "component App inherits Window { property <hit3d> result: Scene3D.ray-hit-instance(";
    let help = completion::signature_help(call, (0, call.len()), None);
    assert_eq!(
        help["signatures"][0]["label"],
        "Scene3D.ray-hit-instance(objects: [record], origin: point3d, direction: vector3d, max-distance: float, row-index: int, mesh-node-index: int, instance-index: int, layer-mask?: int) -> hit3d"
    );
}

#[test]
fn material_blend_modes_complete_in_record_values() {
    let template = r#"struct Finish { blend-mode: string }
        component App inherits Window { property <Finish> finish: Finish { blend-mode: VALUE } }"#;
    for (prefix, expected) in [("sc", "screen"), ("mul", "multiply"), ("so", "source-over")] {
        let text = template.replace("VALUE", &format!("\"{prefix}|\""));
        let items = complete_at(&text);
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
}

#[test]
fn material_depth_modes_complete_in_record_values() {
    let template = r#"struct Finish { depth-draw-mode: string }
        component App inherits Window { property <Finish> finish: Finish { depth-draw-mode: VALUE } }"#;
    for (prefix, expected) in [
        ("opaque-o", "opaque-only"),
        ("opaque-p", "opaque-prepass"),
        ("al", "always"),
        ("ne", "never"),
    ] {
        let text = template.replace("VALUE", &format!("\"{prefix}|\""));
        let items = complete_at(&text);
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
}

#[test]
fn material_cull_modes_complete_in_record_values() {
    let template = r#"struct Finish { cull-mode: string }
        component App inherits Window { property <Finish> finish: Finish { cull-mode: VALUE } }"#;
    for (prefix, expected) in [("ba", "back"), ("fr", "front"), ("no", "none")] {
        let text = template.replace("VALUE", &format!("\"{prefix}|\""));
        let items = complete_at(&text);
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
}

#[test]
fn material_texture_channels_complete_for_color_and_scalar_maps() {
    let template = r#"struct Finish { FIELD: string }
        component App inherits Window { property <Finish> finish: Finish { FIELD: "|"} }"#;
    for (field, rgb) in xrack_parser::builtins::SCENE_MATERIAL_CHANNEL_FIELDS {
        let items = complete_at(&template.replace("FIELD", field));
        for channel in ["r", "g", "b", "a"] {
            assert!(
                items.iter().any(|item| item["label"] == channel),
                "{field}: {items:?}"
            );
        }
        assert_eq!(
            items.iter().any(|item| item["label"] == "rgb"),
            *rgb,
            "{field}: {items:?}"
        );
    }
}

#[test]
fn vertex_color_mask_options_complete_for_full_and_partial_values() {
    let template = r#"struct Finish { FIELD: string }
        component App inherits Window { property <Finish> finish: Finish { FIELD: "VALUE"} }"#;
    for field in xrack_parser::builtins::SCENE_VERTEX_MASK_FIELDS {
        let source = template.replace("FIELD", field);
        let items = complete_at(&source.replace("VALUE", "|"));
        for channel in ["none", "r", "g", "b", "a"] {
            assert!(
                items.iter().any(|item| item["label"] == channel),
                "{field}: {items:?}"
            );
        }
        let partial = source.replace("VALUE\"", "no|");
        let items = complete_at(&partial);
        assert!(
            items.iter().any(|item| item["label"] == "none"),
            "{field}: {items:?}"
        );
    }
}

#[test]
fn fresnel_fields_offer_record_completion_hover_and_types() {
    let fields = r##"
struct Finish {
    fresnel-power: float, fresnel-bias: float,
    clearcoat-fresnel-power: float, clearcoat-fresnel-scale-bias-enabled: bool,
}
struct Actor {
    id: string, mesh: string, x: float, y: float, z: float,
    scale-x: float, scale-y: float, scale-z: float, yaw: float, pitch: float, roll: float,
    color: color, material: Finish,
}
export component FresnelScene inherits Window {
    property <Finish> finish: Finish { FIELD };
    SceneView3D { model: [
        Actor {
            id: "panel", mesh: "primitive:rectangle", x: 0, y: 0, z: 0,
            scale-x: 1, scale-y: 1, scale-z: 1, yaw: 0, pitch: 0, roll: 0,
            color: #fff, material: root.finish,
        }
    ]; }
}
"##;
    let items = complete_at(&fields.replace("FIELD", "clearcoat-fresnel-sc|"));
    assert!(
        items
            .iter()
            .any(|item| item["label"] == "clearcoat-fresnel-scale-bias-enabled"),
        "{items:?}"
    );
    let valid = fields.replace("FIELD",
        "fresnel-power: 5.0, fresnel-bias: 0.15, clearcoat-fresnel-power: 3.0, clearcoat-fresnel-scale-bias-enabled: true");
    let diagnostics = analysis::analyze(URI, &valid, None).diagnostics;
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let source = analysis::Source::new(&valid);
    let cursor = source.coordinates(valid.rfind("clearcoat-fresnel-power").unwrap());
    let hover = completion::query("textDocument/hover", URI, &valid, Some(cursor), None);
    assert!(
        hover["contents"]["value"]
            .as_str()
            .is_some_and(|value| value.contains("clearcoat-fresnel-power: float")),
        "{hover:?}"
    );
    let invalid = valid
        .replace(
            "clearcoat-fresnel-power: float",
            "clearcoat-fresnel-power: string",
        )
        .replace(
            "clearcoat-fresnel-power: 3.0",
            "clearcoat-fresnel-power: \"wrong\"",
        );
    let diagnostics = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(
        diagnostics.values().flatten().any(|item| {
            let message = item["message"].as_str().unwrap_or_default();
            message.contains("clearcoat-fresnel-power") && message.contains("float")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn parametric_geometry_completes_shapes_and_checks_field_types() {
    let template = r##"
struct Shape { shape: string, segments: int }
struct Actor {
    id: string, x: float, y: float, z: float, yaw: float, pitch: float, roll: float,
    scale-x: float, scale-y: float, scale-z: float, color: color, geometry: Shape,
}
export component Demo inherits Window {
    SceneView3D {
        model: [Actor { id: "shape", x: 0, y: 0, z: 0, yaw: 0, pitch: 0, roll: 0,
            scale-x: 1, scale-y: 1, scale-z: 1, color: #fff,
            geometry: Shape { shape: KIND, segments: VALUE },
        }];
    }
}
"##;
    for (prefix, shape) in [
        ("to", "torus"),
        ("ca", "capsule"),
        ("sp", "sphere"),
        ("cy", "cylinder"),
    ] {
        let items = complete_at(
            &template
                .replace("KIND", &format!("\"{prefix}|\""))
                .replace("VALUE", "16"),
        );
        assert!(items.iter().any(|item| item["label"] == shape), "{items:?}");
    }
    let good = template.replace("KIND", "\"torus\"").replace("VALUE", "16");
    assert!(analysis::analyze(URI, &good, None).diagnostics.is_empty());
    let bad = good
        .replace("segments: int", "segments: float")
        .replace("segments: 16", "segments: 16.5");
    let diagnostics = analysis::analyze(URI, &bad, None).diagnostics;
    assert!(
        diagnostics.values().flatten().any(|item| {
            let message = item["message"].as_str().unwrap_or_default();
            message.contains("3D geometry")
                && message.contains("segments")
                && message.contains("int")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn triangle_mesh_records_complete_nested_fields_and_validate_vertex_types() {
    let source = r##"
struct Vertex { position: point3d, normal: vector3d, u: float, v: float }
struct MeshData { vertices: [Vertex], indices: [int] }
struct Actor {
    id: string, x: float, y: float, z: float, yaw: float, pitch: float, roll: float,
    scale-x: float, scale-y: float, scale-z: float, color: color, geometry: MeshData,
}
export component App inherits Window {
    in property <[Vertex]> points: [];
    SceneView3D { model: [
        Actor { id: "panel", x: 0, y: 0, z: 0, yaw: 0, pitch: 0, roll: 0,
            scale-x: 1, scale-y: 1, scale-z: 1, color: #fff,
            geometry: MeshData { FIELD },
        },
    ]; }
}
"##;
    let items = complete_at(&source.replace("FIELD", "ver|"));
    assert!(
        items.iter().any(|item| item["label"] == "vertices"),
        "{items:?}"
    );
    let good = source.replace("FIELD", "vertices: root.points, indices: [0, 1, 2]");
    assert!(analysis::analyze(URI, &good, None).diagnostics.is_empty());
    let bad = good.replace("position: point3d", "position: vector3d");
    let diagnostics = analysis::analyze(URI, &bad, None).diagnostics;
    assert!(
        diagnostics.values().flatten().any(|item| {
            let message = item["message"].as_str().unwrap_or_default();
            message.contains("3D vertex")
                && message.contains("position")
                && message.contains("point3d")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn authored_mesh_slot_completion_uses_literal_subset_names() {
    let source = r##"
struct Vertex { position: point3d, u1: float, v1: float }
struct Subset { offset: int, count: int, name: string }
struct MeshData { vertices: [Vertex], subsets: [Subset] }
struct Finish { base-color: color, slot: string }
struct Slots { face: Finish }
struct Actor {
    id: string, x: float, y: float, z: float, yaw: float, pitch: float, roll: float,
    scale-x: float, scale-y: float, scale-z: float, color: color,
    geometry: MeshData, material-slots: Slots,
}
export component App inherits Window {
    SceneView3D { model: [
        Actor { id: "panel", x: 0, y: 0, z: 0, yaw: 0, pitch: 0, roll: 0,
            scale-x: 1, scale-y: 1, scale-z: 1, color: #fff,
            geometry: MeshData { vertices: [], subsets: [
                Subset { offset: 0, count: 6, name: "edge" },
                Subset { offset: 6, count: 6, name: "#trim" },
            ] },
            material-slots: Slots { face: Finish { base-color: #fff, slot: SLOT } },
        },
    ]; }
}
"##;
    let items = complete_at(&source.replace("SLOT", "\"|\""));
    for slot in ["edge", "name:#trim", "surface"] {
        assert!(items.iter().any(|item| item["label"] == slot), "{items:?}");
    }
    let items = complete_at(&source.replace("SLOT", "\"ed|\""));
    assert_eq!(
        items
            .iter()
            .map(|item| item["label"].as_str())
            .collect::<Vec<_>>(),
        [Some("edge")]
    );
    let valid = source.replace("SLOT", "\"edge\"");
    assert!(analysis::analyze(URI, &valid, None).diagnostics.is_empty());
    let invalid = valid
        .replace("count: int", "count: float")
        .replace("count: 6", "count: 6.0");
    let diagnostics = analysis::analyze(URI, &invalid, None).diagnostics;
    assert!(
        diagnostics.values().flatten().any(|item| {
            let message = item["message"].as_str().unwrap_or_default();
            message.contains("3D subset") && message.contains("count") && message.contains("int")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn scene_node_coordinate_mapping_has_typed_completion_and_signature_help() {
    for (member, input, kind) in [
        ("map-position", "position", "point3d"),
        ("map-direction", "direction", "vector3d"),
    ] {
        let partial =
            format!("component Demo inherits Window {{ property <{kind}> p: Scene3D.map-|");
        assert!(
            complete_at(&partial)
                .iter()
                .any(|item| item["label"] == member)
        );
        let call = format!(
            "component Demo inherits Window {{ property <{kind}> p: Scene3D.{member}(root.nodes, \"rig\", "
        );
        let help = completion::signature_help(&call, (0, call.len()), None);
        assert_eq!(help["activeParameter"], 2);
        assert_eq!(
            help["signatures"][0]["label"],
            format!(
                "Scene3D.{member}(objects: [record], from: string, to: string, {input}: {kind}, fallback: {kind}) -> {kind}"
            )
        );
    }
}

#[test]
fn scene_material_completes_live_texture_sources_in_the_current_component() {
    let property = complete_at("component App inherits Window { Rectangle { texture-so| } }");
    assert!(
        property
            .iter()
            .any(|item| item["label"] == "texture-source")
    );
    let mode = complete_at("component App inherits Window { Rectangle { texture-o| } }");
    assert!(mode.iter().any(|item| item["label"] == "texture-only"));
    let mipmaps = complete_at("component App inherits Window { Rectangle { texture-mi| } }");
    assert!(
        mipmaps
            .iter()
            .any(|item| item["label"] == "texture-mipmaps")
    );
    let items = complete_at(
        r#"
struct Finish { base-item: string, }
component Other inherits Window {
    Rectangle { texture-source: "dashboard-other"; }
}
component App inherits Window {
    Rectangle { texture-source: "dashboard"; }
    Rectangle { texture-source: "dial"; }
    property <Finish> finish: Finish { base-item: "dash|"; };
}
"#,
    );
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["label"], "dashboard");
}

#[test]
fn scene_effect_buffer_completion_uses_only_earlier_outputs_in_the_chain() {
    for request in [
        r#"input: "so|", buffers: []"#,
        r#"buffers: [BufferInput { name: "so|" }]"#,
    ] {
        let source = r#"
            component Demo inherits Window {
                SceneView3D { scene-effects: [
                    Effect { output: "soft-unrelated" }
                ]; }
                SceneView3D { scene-effects: [
                    Effect { output: "soft-main", buffers: [BufferInput { output: "soft-nested" }] },
                    Effect { output: "soft-current", CURSOR },
                    Effect { output: "soft-later" }
                ]; }
            }
        "#.replace("CURSOR", request);
        let items = complete_at(&source);
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["label"], "soft-main");
        assert_eq!(items[0]["textEdit"]["newText"], "soft-main");
    }
}

#[test]
fn previous_frame_effect_inputs_complete_persistent_outputs_including_self() {
    let source = r#"
        component Demo inherits Window {
            SceneView3D { scene-effects: [
                Effect { output: "trail-temporary", history: false },
                Effect { output: "trail-self", history: true,
                    buffers: [Buffer { previous-frame: true, name: "trail-|" }]
                },
                Effect { output: "trail-later", history: true }
            ]; }
        }
    "#;
    let items = complete_at(source);
    assert_eq!(
        items
            .iter()
            .filter_map(|item| item["label"].as_str())
            .collect::<Vec<_>>(),
        ["trail-later", "trail-self"]
    );
    for property in ["scene-effect-history-revision", "scene-effect-animate"] {
        let items =
            complete_at("component Demo inherits Window { SceneView3D { scene-effect-| } }");
        assert!(
            items.iter().any(|item| item["label"] == property),
            "{property}"
        );
    }
}

#[test]
fn scene_effect_buffers_complete_formats_and_sampling_options() {
    for (field, expected) in [
        (r#"output-format: "rg1|""#, "rg16f"),
        (r#"buffers: [Read { filter: "nea|" }]"#, "nearest"),
        (r#"buffers: [Read { wrap-u: "mir|" }]"#, "mirror"),
    ] {
        let source = r#"
            struct Read { name: string, filter: string, wrap-u: string }
            struct Pass { shader: string, output-format: string, buffers: [Read] }
            component App inherits Window { SceneView3D { scene-effects: [Pass { FIELD }]; } }
        "#
        .replace("FIELD", field);
        let items = complete_at(&source);
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
}

#[test]
fn scene_effect_texture_inputs_complete_panel_names_and_color_space() {
    let layout = r#"
        struct TextureInput { item: string, texture: string, color-space: string }
        struct Effect { shader: string, buffers: [TextureInput] }
        component App inherits Window {
            Rectangle { texture-source: "live-swatch"; }
            SceneView3D { scene-effects: [Effect { buffers: [TextureInput { FIELD }] }]; }
        }
    "#;
    for (field, expected) in [
        (r#"item: "live-|""#, "live-swatch"),
        (r#"color-space: "sr|""#, "srgb"),
    ] {
        let items = complete_at(&layout.replace("FIELD", field));
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
}

#[test]
fn material_shader_inputs_complete_panel_names_and_sampling() {
    let layout = r#"
        struct TextureInput { item: string, texture: string, filter: string, color-space: string }
        struct Style { shader-textures: [TextureInput] }
        component App inherits Window {
            Rectangle { texture-source: "live-swatch"; }
            property <Style> style: Style { shader-textures: [TextureInput { FIELD }] };
        }
    "#;
    for (field, expected) in [
        (r#"item: "live-|""#, "live-swatch"),
        (r#"color-space: "sr|""#, "srgb"),
        (r#"filter: "nea|""#, "nearest"),
    ] {
        let items = complete_at(&layout.replace("FIELD", field));
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
}

#[test]
fn material_shader_files_have_signature_path_completion_and_links() {
    let root = workspace_fixture("material-shader-path");
    std::fs::write(root.join("surface.wgsl"), "").unwrap();
    std::fs::write(root.join("surface.png"), "").unwrap();
    let workspace = completion::Workspace::new(&fixture_uri("app.xr"), std::iter::empty()).unwrap();
    let uri = workspace.uri(&root.join("app.xr")).unwrap();
    let workspace = completion::Workspace::new(&uri, std::iter::empty()).unwrap();
    let opening = "component App inherits Window { property <string> shader: @material-shader-url(";
    let hints = completion::signature_help(opening, (0, opening.len()), Some(&workspace));
    assert_eq!(
        hints["signatures"][0]["label"],
        "@material-shader-url(path: string literal) -> string"
    );
    let help = hints["signatures"][0]["documentation"].to_string();
    assert!(
        help.contains("scene_vertex")
            && help.contains("shader-displacement")
            && help.contains("scene_light")
            && help.contains("scene_ambient"),
        "{help}"
    );
    assert!(help.contains("material_texture_world_position"), "{help}");
    let prefix = opening.to_owned() + "\"sur";
    let completions = completion::complete(&prefix, 0, prefix.len(), Some(&workspace));
    assert_eq!(completions.len(), 1, "{completions:?}");
    assert_eq!(completions[0]["textEdit"]["newText"], "surface.wgsl");
    let completed = prefix + "face.wgsl\"); }";
    let links = completion::document_links(&completed, &workspace);
    assert_eq!(
        links[0]["target"],
        workspace.uri(&root.join("surface.wgsl")).unwrap()
    );
}

#[test]
fn named_scene_capture_completion_stays_within_the_view_and_offers_cameras() {
    let source = r#"
        component App inherits Window {
            SceneView3D { scene-captures: [Feed { name: "detail-other" }]; }
            SceneView3D {
                model: [
                    Object { id: "side-eye", camera-kind: "orthographic" },
                    Object { id: "side-mesh", mesh: "primitive:box" }
                ];
                scene-captures: [
                    Feed { name: "detail-main", camera: "CAMERA" },
                    Feed { name: "detail-disabled", enabled: false }
                ];
                scene-effects: [Pass { buffers: [Input { capture: "CAPTURE" }] }];
            }
        }
    "#;
    for (text, expected) in [
        (source.replace("CAPTURE", "detail-|"), "detail-main"),
        (source.replace("CAMERA", "side-|"), "side-eye"),
    ] {
        let items = complete_at(&text);
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["label"], expected);
    }
}

#[test]
fn shader_texture_capture_completion_uses_the_owning_viewport() {
    let items = complete_at(
        r#"
        component App inherits Window {
            SceneView3D { scene-captures: [Feed { name: "source-other" }]; }
            SceneView3D {
                scene-captures: [
                    Feed { name: "source-main" },
                    Feed { name: "source-off", enabled: false }
                ];
                model: [Object { material-slots: Slots {
                    body: Style { shader-textures: [Input { capture: "source-|" }] }
                } }];
            }
        }
    "#,
    );
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["label"], "source-main");
    let items = complete_at(
        r#"
        component App inherits Window {
            property <Style> screen: Style { shader-textures: [Input { capture: "source-|" }] };
            SceneView3D { scene-captures: [Feed { name: "source-main" }]; }
        }
    "#,
    );
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["label"], "source-main");
}

#[test]
fn named_capture_attachment_completion_works_in_effects_and_materials() {
    let source = r#"
        component App inherits Window {
            SceneView3D { FIELD }
        }
    "#;
    for (field, expected) in [
        (
            r#"scene-effects: [Pass { buffers: [Input { capture: "data", attachment: "dep|" }] }];"#,
            "depth",
        ),
        (
            r#"model: [Actor { material: Style { shader-textures: [Input { capture: "data", attachment: "norm|" }] } }];"#,
            "normal-roughness",
        ),
        (
            r#"scene-effects: [Pass { buffers: [Input { capture: "data", attachment: "base-col|" }] }];"#,
            "base-color-metalness",
        ),
        (
            r#"model: [Actor { material: Style { shader-textures: [Input { capture: "data", attachment: "emiss|" }] } }];"#,
            "emissive-occlusion",
        ),
    ] {
        let items = complete_at(&source.replace("FIELD", field));
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["label"], expected);
    }
}

#[test]
fn scene_capture_sources_complete_active_captures_from_the_same_view() {
    for (prefix, property) in [
        ("scene-color-cap", "scene-color-capture"),
        ("scene-depth-cap", "scene-depth-capture"),
    ] {
        let properties = complete_at(&format!(
            "component App inherits Window {{ SceneView3D {{ {prefix}| }} }}"
        ));
        assert!(properties.iter().any(|item| item["label"] == property));
        let source = r#"
        component App inherits Window {
            SceneView3D {
                scene-captures: [
                    Feed { name: "stage-live", enabled: true },
                    Feed { name: "stage-disabled", enabled: false },
                ];
                FIELD: "stage-|";
            }
            SceneView3D { scene-captures: [Feed { name: "stage-other" }]; }
        }
    "#;
        let items = complete_at(&source.replace("FIELD", property));
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["label"], "stage-live");
    }
}

#[test]
fn capture_input_names_complete_other_enabled_feeds_in_the_owning_view() {
    let source = r#"
        struct Feed { name: string, color-source: string, depth-source: string, enabled: bool }
        component App inherits Window {
            SceneView3D {
                scene-captures: [
                    Feed { name: "stage-output", FIELD: "stage-|" },
                    Feed { name: "stage-lighting", enabled: true },
                    Feed { name: "stage-depth", enabled: true },
                    Feed { name: "stage-disabled", enabled: false },
                ];
            }
            SceneView3D { scene-captures: [Feed { name: "stage-other" }]; }
        }
    "#;
    for field in ["color-source", "depth-source"] {
        let items = complete_at(&source.replace("FIELD", field));
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(items[0]["label"], "stage-depth");
        assert_eq!(items[1]["label"], "stage-lighting");
    }
}

#[test]
fn capture_renderables_completes_transparent_mode() {
    let items = complete_at(
        r#"component App inherits Window {
            SceneView3D { scene-captures: [Feed { name: "glass", renderables: "tran|" }]; }
        }"#,
    );
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["label"], "transparent");
}

#[test]
fn capture_override_shader_textures_use_the_owning_view() {
    let items = complete_at(
        r#"
        component App inherits Window {
            SceneView3D {
                scene-captures: [
                    Feed { name: "scene-color" },
                    Feed { name: "mask", override-material: Mat { shader-textures: [Input { capture: "scene-|" }] } },
                ];
            }
            SceneView3D { scene-captures: [Feed { name: "scene-other" }]; }
        }
    "#,
    );
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["label"], "scene-color");
}

#[test]
fn capture_pipeline_state_completes_depth_comparison_and_blend() {
    for (field, expected) in [
        (r#"depth-compare: "greater-e|""#, "greater-equal"),
        (r#"blend-mode: "add|""#, "additive"),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ SceneView3D {{ scene-captures: [Feed {{ name: \"glass\", pipeline-state: State {{ {field} }} }}]; }} }}"
        ));
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["label"], expected);
    }
}

#[test]
fn viewport_and_scissor_complete_typed_rectangle_fields() {
    for field in ["viewport", "scissor"] {
        let source = format!(
            r#"
            struct Bounds {{ x: float, y: float, width: float, height: float }}
            struct State {{ {field}: Bounds }}
            struct Feed {{ name: string, pipeline-state: State }}
            component App inherits Window {{
                SceneView3D {{ scene-captures: [Feed {{ name: "window", pipeline-state: State {{
                    {field}: Bounds {{ wid| }}
                }} }}]; }}
            }}
        "#
        );
        let items = complete_at(&source);
        assert!(
            items.iter().any(|item| item["label"] == "width"),
            "{items:?}"
        );
    }
}

#[test]
fn capture_record_completes_depth_clear_field() {
    let items = complete_at(
        r#"
        struct Feed { name: string, depth-clear: float }
        component App inherits Window {
            SceneView3D { scene-captures: [Feed { name: "scene", depth-cl| }]; }
        }
    "#,
    );
    assert!(
        items.iter().any(|item| item["label"] == "depth-clear"),
        "{items:?}"
    );
}

#[test]
fn stencil_source_and_operations_complete_inside_named_captures() {
    let sources = complete_at(
        r#"
        component App inherits Window {
            SceneView3D { scene-captures: [
                Feed { name: "mask-inner" },
                Feed { name: "panel", stencil-source: "mask-|" },
            ]; }
            SceneView3D { scene-captures: [Feed { name: "mask-other" }]; }
        }
    "#,
    );
    assert_eq!(sources.len(), 1, "{sources:?}");
    assert_eq!(sources[0]["label"], "mask-inner");
    for (field, expected) in [
        (r#"depth-fail: "increment-w|""#, "increment-wrap"),
        (r#"compare: "less-e|""#, "less-equal"),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ SceneView3D {{ scene-captures: [Feed {{ pipeline-state: Pass {{ stencil: Mask {{ {field} }} }} }}]; }} }}"
        ));
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["label"], expected);
    }
}

#[test]
fn capture_polygon_mode_completes_wireframe_values() {
    let items = complete_at(
        r#"
        component App inherits Window {
            SceneView3D { scene-captures: [Feed { name: "edges", pipeline-state: State {
                polygon-mode: "li|"
            } }]; }
        }
    "#,
    );
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["label"], "line");
}

#[test]
fn capture_target_blend_completes_operations_and_factors() {
    for (field, expected) in [
        (r#"destination-color: "one-minus-d|" "#, "one-minus-dst"),
        (r#"source-alpha: "cons|" "#, "constant"),
        (r#"alpha-operation: "rev|" "#, "reverse-subtract"),
        (r#"color-operation: "ma|" "#, "max"),
    ] {
        let items = complete_at(&format!(
            "component App inherits Window {{ SceneView3D {{ scene-captures: [Feed {{ pipeline-state: Pass {{ target-blend: Blend {{ {field} }} }} }}]; }} }}"
        ));
        assert!(
            items.iter().any(|item| item["label"] == expected),
            "{items:?}"
        );
    }
    let items = complete_at(
        r#"component App inherits Window { property <Style> style: Style { alpha-operation: "r|" }; }"#,
    );
    assert!(items.iter().all(|item| item["label"] != "reverse-subtract"));
}

#[test]
fn capture_color_format_completes_linear_attachment_formats() {
    let items = complete_at(
        r#"
        component App inherits Window {
            SceneView3D { scene-captures: [Feed { name: "mask", color-format: "rg|" }]; }
        }
        "#,
    );
    let labels: Vec<_> = items
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    for format in ["rg8", "rg16f", "rgba8", "rgba16f"] {
        assert!(labels.contains(&format), "{labels:?}");
    }
    assert!(!labels.contains(&"r16f"), "{labels:?}");
}
