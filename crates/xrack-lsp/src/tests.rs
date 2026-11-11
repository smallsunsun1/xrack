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
    assert_eq!(
        serve(&mut input.as_slice(), &mut output).unwrap(),
        ExitCode::SUCCESS
    );
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
    let incomplete = analysis::analyze(URI, "component Demo inherits Window {\n");
    assert_eq!(incomplete.diagnostics[0]["code"], "syntax");
    assert!(incomplete.symbols.is_empty());

    let imported = analysis::analyze(
        URI,
        concat!(
            "import { Card } from \"widgets.xr\";\n",
            "export /* 中文 */ component\nDemo inherits Card {}",
        ),
    );
    assert!(imported.diagnostics.is_empty());
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
