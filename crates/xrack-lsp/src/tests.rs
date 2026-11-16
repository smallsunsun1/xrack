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
    ] {
        let text =
            format!("component Demo inherits Window {{ button := Button {{ clicked => {{ {call}");
        let source = analysis::Source::new(&text);
        let hint = completion::signature_help(&text, source.coordinates(text.len()), None);
        assert_eq!(hint["signatures"][0]["label"], signature);
        assert_eq!(hint["activeParameter"], parameter);
        assert_eq!(hint["signatures"][0]["documentation"]["kind"], "markdown");
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
