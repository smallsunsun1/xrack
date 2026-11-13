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
