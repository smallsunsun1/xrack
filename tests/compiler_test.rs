use xrack_compiler::{Compilation, CompilerError, ir, mlir, parse, semantic};

fn compile(source: &str, filename: &str) -> Result<Compilation, String> {
    xrack_compiler::compile(source, filename).map_err(|error| error.to_string())
}

fn analyze(source: &str) -> Result<ir::Module, String> {
    let ast = parse(source).map_err(|e| format!("{e:?}"))?;
    semantic::analyze(&ast).map_err(|e| format!("{e:?}"))
}

#[test]
fn compiler_errors_retain_source_diagnostics() {
    let Err(CompilerError::Diagnostics {
        filename,
        diagnostics,
    }) = xrack_compiler::compile("component App inherits Window { width: true; }", "bad.xr")
    else {
        panic!("expected a compiler diagnostic");
    };
    assert_eq!(filename, "bad.xr");
    assert_eq!(diagnostics[0].span.line, 1);
    assert!(diagnostics[0].message.contains("type mismatch"));
}

#[test]
fn decimal_spin_box_uses_float_links_and_typed_precision() {
    let source = r#"
        export component App inherits Window {
            in-out property <float> amount: 0.5;
            out property <float> last: 0.0;
            amount-field := DoubleSpinBox {
                value <=> root.amount;
                decimals: 2;
                step-size: 0.05;
                edited(value) => { root.last = value; }
            }
        }
    "#;
    let compiled = compile(source, "decimal-spin.xr").unwrap();
    assert!(compiled.mlir.contains("xr.control_link"));
    assert!(
        compiled
            .rust
            .to_string()
            .contains("decimal_spin_value_after")
    );
    let raw = lower(source, "decimal-spin.xr");
    assert!(
        mlir::process(&raw.replace("name = \"decimals\"", "name = \"maximum\""))
            .unwrap_err()
            .contains("incorrect DoubleSpinBox property type")
    );
    let wrong = source.replace("<float> amount: 0.5", "<int> amount: 1");
    assert!(
        compile(&wrong, "decimal-spin.xr")
            .err()
            .unwrap()
            .contains("two-way binding requires a writable float property")
    );
    assert!(
        compile(
            "export component Amount inherits DoubleSpinBox { edited(value) => {} }",
            "decimal-root.xr",
        )
        .is_ok()
    );
}

#[test]
fn calendar_lowers_date_values_and_rejects_invalid_schema_and_aliases() {
    let source = r#"
        export component App inherits Window {
            in-out property <Date> selection: Date { year: 2024, month: 2, day: 29 };
            in-out property <Date> month: Date { year: 2024, month: 2, day: 1 };
            out property <int> picked: 0;
            picker := Calendar {
                date <=> root.selection;
                display-month <=> root.month;
                changed(value) => { root.picked = value.day; }
            }
        }
    "#;
    let result = compile(source, "calendar.xr").unwrap();
    assert!(result.mlir.contains("!xr.date"));
    assert!(result.mlir.contains("xr.make_date"));
    assert!(result.mlir.contains("xr.date_field"));
    assert!(result.mlir.contains("xr.control_link"));
    assert!(result.rust.to_string().contains("xrack_runtime :: Date"));
    let raw = lower(source, "calendar.xr");
    assert!(
        mlir::process(&raw.replace("value = \"day\"", "value = \"hours\""))
            .unwrap_err()
            .contains("requires year, month, or day")
    );
    assert!(
        compile(&source.replace("value.day", "value.hours"), "calendar.xr")
            .err()
            .unwrap()
            .contains("Date has no field")
    );
    assert!(
        compile(
            &source.replace(
                "display-month <=> root.month",
                "display-month <=> root.selection"
            ),
            "calendar.xr"
        )
        .err()
        .unwrap()
        .contains("require distinct writable properties")
    );
}

#[test]
fn flow_layout_spacing_and_orientation_are_typed() {
    let source = r#"
        component App inherits Window {
            FlowLayout {
                orientation: vertical;
                horizontal-spacing: 12px;
                alignment: space-evenly;
                cross-axis-alignment: center;
                Text { text: "Item"; }
            }
        }
    "#;
    let raw = lower(source, "flow.xr");
    assert!(mlir::process(&raw).is_ok());
    let wrong_type = raw.replace("name = \"orientation\"", "name = \"vertical-spacing\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("incorrect FlowLayout property type")
    );
    let wrong_source = source.replace("12px", "true");
    assert!(
        compile(&wrong_source, "flow.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn toolbar_accepts_typed_icons_and_action_references() {
    let source = r#"
        component App inherits Window {
            property <image> marker;
            grid := Action { icon: root.marker; text: "Grid"; }
            ToolBar {
                orientation: vertical;
                wrap: true;
                grid-button := ToolButton { action: grid; icon-only: true; icon-size: 20px; }
            }
        }
    "#;
    assert!(compile(source, "toolbar.xr").is_ok());
    let raw = lower(source, "toolbar.xr");
    let invalid_icon = raw.replace("name = \"icon-size\"", "name = \"icon\"");
    assert!(
        mlir::process(&invalid_icon)
            .unwrap_err()
            .contains("icon requires an image")
    );
    let invalid_wrap = source.replace("wrap: true", "wrap: 10px");
    assert!(
        compile(&invalid_wrap, "toolbar.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn editable_combo_box_validates_text_types_and_keeps_links_independent() {
    let source = r#"
        component App inherits Window {
            in-out property <string> draft;
            in-out property <string> chosen;
            choices := ComboBox {
                editable: true;
                auto-complete: true;
                edit-text <=> root.draft;
                current-value <=> root.chosen;
                accepted(text) => {}
                edited(text) => {}
            }
        }
    "#;
    assert!(compile(source, "combo.xr").is_ok());
    let alias = source.replace(
        "current-value <=> root.chosen",
        "current-value <=> root.draft",
    );
    assert!(
        compile(&alias, "combo.xr")
            .err()
            .unwrap()
            .contains("distinct writable properties")
    );
    let raw = lower(source, "combo.xr");
    assert!(raw.contains("name = \"auto-complete\""));
    let native_alias = raw.replace("value = \"chosen\"", "value = \"draft\"");
    assert!(
        mlir::process(&native_alias)
            .unwrap_err()
            .contains("distinct writable properties")
    );
    let wrong_type = raw.replace("name = \"editable\"", "name = \"placeholder-text\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("incorrect editable ComboBox property type")
    );
    let invalid_completion = source.replace("auto-complete: true", "auto-complete: 1");
    assert!(
        compile(&invalid_completion, "combo.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

fn lower(source: &str, filename: &str) -> String {
    mlir::lower(&parse(source).unwrap(), filename).unwrap()
}

#[test]
fn parses_comments_unicode_and_operator_precedence() {
    let source = r#"/* banner */ export component App inherits Window {
        in-out property <int> count: 1 + 2 * 3;
        property <string> name: "你好\n\"XRack\"";
        property <color> color: #abc;
        title: name;
        Text { text: count > 2 && true ? "yes" : "no"; color: root.color; }
    }"#;
    let model = analyze(source).unwrap();
    let e = &model.components[0].properties[0].init;
    assert_eq!(e.value, "+");
    assert_eq!(e.args[1].value, "*");
    assert_eq!(model.components[0].properties[1].ty.name(), "string");
    assert_eq!(
        model.components[0].root.children[0].bindings[1]
            .value
            .ty
            .name(),
        "color"
    );
}

#[test]
fn reports_both_lexer_and_recovered_parser_errors() {
    for source in [
        "component A inherits Window { @ }",
        "component A inherits Window { width: ; }",
        "component A inherits Window {} trailing",
        "component A inherits Window { title: \"broken; }",
        "",
    ] {
        let errors = parse(source).unwrap_err();
        assert!(!errors.is_empty(), "{source}");
        assert!(errors[0].span.line >= 1);
        assert!(errors[0].span.column >= 1);
    }
}

#[test]
fn rejects_invalid_ui_programs_before_codegen() {
    let cases = [
        ("property <int> a: true;", "type mismatch"),
        ("property <int> _;", "reserved identifier"),
        ("property <int> a: missing;", "unknown property"),
        ("property <int> a: b; property <int> b: a;", "cyclic"),
        ("property <int> a; property <int> a;", "duplicate"),
        (
            "property <int> foo-bar; property <int> foo_bar;",
            "duplicate",
        ),
        ("width: 5;", "type mismatch"),
        (
            "MenuBar { Menu { MenuItem { shortcut: 1; } } }",
            "type mismatch",
        ),
        ("wrong: 5;", "unknown property"),
        ("width: 5px; width: 6px;", "duplicate binding"),
        ("property <bool> a: 1 && true;", "invalid operands"),
        ("property <color> c: #12;", "colors require"),
        (
            "property <int> a: 999999999999999999999999999;",
            "i64 range",
        ),
        ("property <float> a: 1e999;", "non-finite"),
        ("property <string> a: \"\\uD800\";", "invalid string"),
        ("property <int> a: other.count;", "unknown element id"),
        ("foo := Text {} foo := Text {}", "duplicate element"),
        ("Unknown {}", "unknown element"),
        ("ListView { Text {} }", "requires exactly one for element"),
        (
            "TableColumn { title: \"stray\"; }",
            "must be a direct child of TableView",
        ),
        (
            "TableView { TableColumn {} }",
            "requires at least one TableColumn and one repeated TableRow",
        ),
        (
            "TableView { TableColumn {} TableColumn {} for row in 3 : TableRow { Text {} } }",
            "one cell element per column",
        ),
        (
            "TableView { for col in 3 : TableColumn {} for row in 3 : TableRow { Text {} } }",
            "requires static TableColumn",
        ),
        (
            "TableView { resizable-columns: 1; TableColumn {} for row in 3 : TableRow { Text {} } }",
            "type mismatch",
        ),
        (
            "TableView { multi-select: \"yes\"; TableColumn {} for row in 3 : TableRow { Text {} } }",
            "type mismatch",
        ),
        (
            "TableView { selection-target: 1; TableColumn {} for row in 3 : TableRow { Text {} } }",
            "type mismatch",
        ),
        (
            "TableCell { text: \"outside\"; }",
            "must be a direct cell of TableRow",
        ),
        (
            "TableView { TableColumn {} for item in [\"one\"] : TableRow { TableCell { text <=> item; } } }",
            "requires the repeated item of a writable [string] property",
        ),
        (
            "in property <[string]> rows; TableView { TableColumn {} for item in rows : TableRow { TableCell { text <=> item; } } }",
            "requires the repeated item of a writable [string] property",
        ),
        (
            "property <[string]> rows; TableView { TableColumn {} for item[index] in rows : TableRow { TableCell { text <=> index; } } }",
            "requires the repeated item of a writable [string] property",
        ),
        (
            "ListView { for row in 3 : Text {} Text {} }",
            "requires exactly one for element",
        ),
        (
            "for row in 3 : ListView { for item in 4 : Text {} }",
            "repeat templates require anonymous display elements",
        ),
        ("Text { callback hit(); }", "component scope"),
        ("Button { clicked() => {} }", "require an id"),
        ("callback hit(step: int); hit() => {}", "expects 1"),
        ("callback hit(a: int, a: int);", "duplicate parameter"),
        (
            "callback hit(); hit() => { root.missing = 1; }",
            "unknown property",
        ),
    ];
    for (body, expected) in cases {
        let source = format!("component App inherits Window {{ {body} }}");
        let error = analyze(&source).unwrap_err();
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
}

#[test]
fn native_mlir_checks_menu_shortcut_binding_type() {
    let raw = lower(
        "export component App inherits Window { MenuBar { Menu { MenuItem { checked: true; } } } }",
        "shortcut.xr",
    );
    let invalid = raw.replace("name = \"checked\"", "name = \"shortcut\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("menu shortcut requires a string")
    );
}

#[test]
fn action_references_are_validated_before_and_after_lowering() {
    let source = r#"
        export component App inherits Window {
            toggle := Action { text: "切换"; checkable: true; }
            button := Button { action: toggle; enabled: true; }
        }
    "#;
    let raw = lower(source, "action.xr");
    assert!(raw.contains("xr.action_ref"));
    assert!(compile(source, "action.xr").is_ok());
    let wrong_owner = raw.replace("owner = \"toggle\"", "owner = \"button\"");
    assert!(
        mlir::process(&wrong_owner)
            .unwrap_err()
            .contains("requires an Action owner")
    );
    for (bad, diagnostic) in [
        (
            source.replace("action: toggle", "action: button"),
            "requires the id of an Action",
        ),
        (
            source.replace("enabled: true;", "clicked() => {}"),
            "activation handler belongs",
        ),
        (
            source.replace("toggle := Action", "Action"),
            "named static declaration",
        ),
    ] {
        assert!(
            compile(&bad, "action.xr")
                .err()
                .unwrap()
                .contains(diagnostic)
        );
    }
}

#[test]
fn table_column_and_cell_structure_is_verified_in_mlir() {
    let source = r#"component App inherits Window {
        TableView {
            row-height: 32px;
            selection-target: cells;
            TableColumn { title: "A"; resizable: false; }
            for row in 3 : TableRow { Text { text: "Row " + row; } }
        }
    }"#;
    let lowered = lower(source, "table.xr");
    assert!(mlir::process(&lowered).is_ok());
    assert!(lowered.contains("!xr.selectiontarget"));
    let invalid_target = lowered.replace("value = \"cells\"", "value = \"diagonal\"");
    assert!(mlir::process(&invalid_target).is_err());
    for flag in ["resizable-columns", "multi-select"] {
        let invalid_flag = lowered.replace("name = \"row-height\"", &format!("name = \"{flag}\""));
        assert!(
            mlir::process(&invalid_flag)
                .unwrap_err()
                .contains("table interaction flags require a bool value")
        );
    }
    let malformed = lowered.replace("kind = \"TableColumn\"", "kind = \"Rectangle\"");
    assert_ne!(malformed, lowered);
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("TableView requires static TableColumn")
    );
}

#[test]
fn table_edit_links_a_writable_model_to_the_repeat_item() {
    let source = r#"component App inherits Window {
        in-out property <[string]> rows: ["one"];
        TableView {
            TableColumn { title: "A"; }
            for entry[index] in root.rows : TableRow { TableCell { text <=> entry; } }
        }
    }"#;
    let mlir = lower(source, "table-edit.xr");
    assert!(mlir.contains("\"xr.row_link\""));
    assert!(mlir::process(&mlir).is_ok());
    let wrong_item = mlir.replace("item = \"entry\"", "item = \"other\"");
    assert!(
        mlir::process(&wrong_item)
            .unwrap_err()
            .contains("requires the repeated string item")
    );
    let read_only = mlir.replace("visibility = \"in-out\"", "visibility = \"in\"");
    assert!(mlir::process(&read_only).is_err());
}

#[test]
fn record_fields_are_typed_through_mlir_and_rust_generation() {
    let source = r#"
        export struct Entry { name: string, count: int }
        export component App inherits Window {
            property <Entry> active: Entry { count: 2, name: "alpha" };
            in-out property <[Entry]> entries: [Entry { name: "beta", count: 3 }];
            pure callback label(Entry) -> string;
            label(value) => { value.name + value.count }
            title: root.label(root.active);
            TableView {
                TableColumn { title: "名称"; }
                for entry in root.entries : TableRow { TableCell { text <=> entry.name; } }
            }
        }
    "#;
    let ir = lower(source, "record.xr");
    assert!(ir.contains("\"xr.record_decl\""));
    assert!(ir.contains("\"xr.get_field\""));
    assert!(ir.contains("!xr.recordlist<\"Entry\">"));
    assert!(mlir::process(&ir).is_ok());
    let invalid = ir.replace("value = \"name\"", "value = \"missing\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("unknown record field")
    );
    let invalid = source.replace("text <=> entry.name", "text <=> entry.count");
    assert!(
        compile(&invalid, "record.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid = source.replace(
        "Entry { count: 2, name: \"alpha\" }",
        "Entry { name: \"alpha\" }",
    );
    assert!(
        compile(&invalid, "record.xr")
            .err()
            .unwrap()
            .contains("missing record field")
    );
    assert!(compile(source, "record.xr").is_ok());
}

#[test]
fn table_editors_verify_typed_record_links() {
    let source = r#"
        struct RowData { units: int, available: bool, load: float }
        export component App inherits Window {
            in-out property <[RowData]> items;
            TableView {
                TableColumn { title: "数量"; }
                TableColumn { title: "可用"; }
                TableColumn { title: "负载"; }
                for row in root.items : TableRow {
                    SpinBox { value <=> row.units; }
                    CheckBox { checked <=> row.available; }
                    Slider { value <=> row.load; }
                }
            }
        }
    "#;
    let ir = lower(source, "editors.xr");
    assert!(mlir::process(&ir).is_ok());
    assert_eq!(ir.matches("\"xr.row_link\"").count(), 3);
    assert!(compile(&source.replace("CheckBox", "Switch"), "editors.xr").is_ok());
    assert!(compile(&source.replace("Slider", "Dial"), "editors.xr").is_ok());
    let malformed = ir.replace("name = \"value\"", "name = \"checked\"");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("matching field of a table model")
    );
    let wrong_type = source.replace("value <=> row.units", "value <=> row.available");
    assert!(
        compile(&wrong_type, "editors.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let int_slider = source.replace("value <=> row.load", "value <=> row.units");
    assert!(
        compile(&int_slider, "editors.xr")
            .err()
            .unwrap()
            .contains("table editor expects float, found int")
    );
    let read_only = source.replace("in-out property", "in property");
    assert!(
        compile(&read_only, "editors.xr")
            .err()
            .unwrap()
            .contains("writable record list")
    );
    let unlinked = source.replace("value <=> row.units", "value: row.units");
    assert!(
        compile(&unlinked, "editors.xr")
            .err()
            .unwrap()
            .contains("require an anonymous editor")
    );
}

#[test]
fn split_view_requires_static_panes_and_typed_handle_properties() {
    let source = r#"export component App inherits SplitView {
        orientation: vertical;
        handle-size: 10px;
        Rectangle { min-height: 35px; }
        Rectangle { preferred-height: 60px; }
        resized() => { }
    }"#;
    let lowered = lower(source, "split.xr");
    assert!(mlir::process(&lowered).is_ok());
    assert!(compile(source, "split.xr").is_ok());
    let invalid = lowered.replace("name = \"handle-size\"", "name = \"handle-color\"");
    assert!(
        mlir::process(&invalid)
            .unwrap_err()
            .contains("incorrect SplitView property type")
    );
    let repeated = source.replace(
        "Rectangle { min-height",
        "for pane in 2 : Rectangle { min-height",
    );
    assert!(
        compile(&repeated, "split.xr")
            .err()
            .unwrap()
            .contains("stable child positions")
    );
}

#[test]
fn tooltip_requires_one_attachment_and_exclusive_content() {
    let source = r#"component App inherits Window {
        Button { text: "Save"; Tooltip { text: "Saves the document"; delay: 20; timeout: 100; } }
        Rectangle { Tooltip { VerticalLayout { Text { text: "Details"; } } } }
    }"#;
    let ir = lower(source, "tooltip.xr");
    assert!(mlir::process(&ir).is_ok());
    assert!(compile(source, "tooltip.xr").is_ok());
    let wrong_type = ir.replace("name = \"delay\"", "name = \"text\"");
    assert!(
        mlir::process(&wrong_type)
            .unwrap_err()
            .contains("incorrect Tooltip property type")
    );
    for (source, expected) in [
        (
            "component A inherits Tooltip {}",
            "Tooltip requires a visual owner",
        ),
        (
            "component A inherits Window { Rectangle { Tooltip {} Tooltip {} } }",
            "at most one Tooltip",
        ),
        (
            "component A inherits Window { Rectangle { Tooltip { text: \"A\"; Text {} } } }",
            "Tooltip requires text or a single content child",
        ),
        (
            "component A inherits Window { Rectangle { for index in 3 : Tooltip {} } }",
            "Tooltip cannot be a repeated child",
        ),
        (
            "component Popup inherits Dialog {} component A inherits Window { Rectangle { Tooltip { Popup {} } } }",
            "Tooltip content cannot be a popup or virtual view",
        ),
    ] {
        assert!(
            compile(source, "tooltip.xr")
                .err()
                .unwrap()
                .contains(expected)
        );
    }
}

#[test]
fn tree_view_roles_require_int_fields_from_a_record_model() {
    let source = r#"
        struct Entry { id: int, parent: int, label: string }
        component App inherits Window {
            property <[Entry]> records;
            TreeView {
                for row in root.records : TreeRow {
                    key: row.id;
                    parent-key: row.parent;
                    Text { text: row.label; }
                }
            }
        }
    "#;
    let ir = lower(source, "tree.xr");
    assert!(mlir::process(&ir).is_ok());
    assert!(compile(source, "tree.xr").is_ok());
    let wrong_type = source.replace("key: row.id", "key: row.label");
    assert!(
        compile(&wrong_type, "tree.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let invalid_role = source.replace("parent-key: row.parent", "parent-key: 0");
    assert!(
        compile(&invalid_role, "tree.xr")
            .err()
            .unwrap()
            .contains("requires an integer field of the repeated record")
    );
    let malformed = ir.replace("name = \"parent-key\"", "name = \"other\"");
    assert!(
        mlir::process(&malformed)
            .unwrap_err()
            .contains("TreeRow requires integer key and parent-key")
    );
}

#[test]
fn tree_cells_lower_to_verified_writable_record_links() {
    let source = r#"
        struct Entry { id: int, parent: int, label: string, quota: int, active: bool, load: float }
        component App inherits Window {
            in-out property <[Entry]> records;
            tree := TreeView {
                multi-select: true;
                selection-changed(count) => {}
                for row in root.records : TreeRow {
                    key: row.id;
                    parent-key: row.parent;
                    HorizontalLayout {
                        TreeCell { text <=> row.label; }
                        SpinBox { value <=> row.quota; }
                        CheckBox { checked <=> row.active; }
                        if row.active : HorizontalLayout {
                            Slider { value <=> row.load; }
                        }
                    }
                }
            }
        }
    "#;
    let ir = lower(source, "tree-edit.xr");
    let optimized = mlir::process(&ir).unwrap().to_mlir();
    assert_eq!(optimized.matches("xr.row_link").count(), 4);
    assert!(optimized.contains("selection-changed"));
    assert!(compile(source, "tree-edit.xr").is_ok());
    assert!(compile(&source.replace("CheckBox", "Switch"), "tree-edit.xr").is_ok());
    let invalid_kind = ir.replace("kind = \"TreeCell\"", "kind = \"TableCell\"");
    assert_ne!(invalid_kind, ir);
    assert!(mlir::process(&invalid_kind).is_err());
    let invalid_link = ir.replace("name = \"value\"", "name = \"checked\"");
    assert!(
        mlir::process(&invalid_link)
            .unwrap_err()
            .contains("matching field of a writable tree model")
    );
    let read_only = source.replace("in-out property", "in property");
    assert!(
        compile(&read_only, "tree-edit.xr")
            .err()
            .unwrap()
            .contains("writable record list")
    );
    let invalid_field = source.replace("text <=> row.label", "text <=> row.id");
    assert!(
        compile(&invalid_field, "tree-edit.xr")
            .err()
            .unwrap()
            .contains("TreeCell editor expects string, found int")
    );
    let unlinked = source.replace("value <=> row.quota", "value: row.quota");
    assert!(
        compile(&unlinked, "tree-edit.xr")
            .err()
            .unwrap()
            .contains("require an anonymous editor")
    );
    let repeated = source.replace(
        "if row.active : HorizontalLayout",
        "for nested in 2 : HorizontalLayout",
    );
    assert!(
        compile(&repeated, "tree-edit.xr")
            .err()
            .unwrap()
            .contains("repeat templates require anonymous display elements")
    );
}

#[test]
fn range_slider_verifies_both_links_and_pair_events() {
    let source = r#"
        export component App inherits Window {
            in-out property <float> lo: 15;
            in-out property <float> hi: 85;
            range := RangeSlider {
                lower-value <=> root.lo;
                upper-value <=> root.hi;
                orientation: vertical;
                changed(first, second) => {}
                released(first, second) => {}
            }
        }
    "#;
    let ir = lower(source, "interval.xr");
    let module = mlir::process(&ir).unwrap();
    assert_eq!(module.to_mlir().matches("xr.control_link").count(), 2);
    assert!(compile(source, "interval.xr").is_ok());
    let alias = source.replace("upper-value <=> root.hi", "upper-value <=> root.lo");
    assert!(
        compile(&alias, "interval.xr")
            .err()
            .unwrap()
            .contains("distinct writable properties")
    );
    let native_alias = ir.replace("value = \"hi\"", "value = \"lo\"");
    assert_ne!(native_alias, ir);
    assert!(
        mlir::process(&native_alias)
            .unwrap_err()
            .contains("distinct writable properties")
    );
    let wrong_arity = source.replace("released(first, second)", "released(first)");
    assert!(
        compile(&wrong_arity, "interval.xr")
            .err()
            .unwrap()
            .contains("two float parameters")
    );
    let native_event = ir.replace("name = \"released\"", "name = \"unexpected\"");
    assert!(
        mlir::process(&native_event)
            .unwrap_err()
            .contains("two float parameters")
    );
}

#[test]
fn dial_keeps_its_typed_input_mode_through_mlir() {
    let source = r#"
        export component App inherits Window {
            in-out property <float> value: 20;
            in-out property <DialInputMode> mode: DialInputMode.vertical;
            knob := Dial {
                value <=> root.value;
                input-mode: root.mode;
                start-angle: -120;
                end-angle: 100;
                wrap: true;
                changed(next) => {}
                released(last) => {}
            }
        }
    "#;
    let raw = lower(source, "dial.xr");
    let optimized = mlir::process(&raw).unwrap().to_mlir();
    assert!(optimized.contains("!xr.dialinputmode"));
    assert!(optimized.contains("xr.control_link"));
    assert!(compile(source, "dial.xr").is_ok());
    let invalid_mode = raw.replace("value = \"vertical\"", "value = \"diagonal\"");
    assert_ne!(invalid_mode, raw);
    assert!(mlir::process(&invalid_mode).is_err());
    let invalid_binding = source.replace("input-mode: root.mode", "input-mode: 42");
    assert!(
        compile(&invalid_binding, "dial.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn scroll_bar_policy_is_typed_and_requires_a_supported_axis() {
    let source = r#"
        export component App inherits Window {
            in-out property <ScrollBarPolicy> policy: ScrollBarPolicy.always-on;
            ScrollView {
                vertical-scroll-bar-policy: root.policy;
                horizontal-scroll-bar-policy: as-needed;
                content-width: 150%;
                content-height: 500px;
                Rectangle { height: 500px; }
            }
        }
    "#;
    let raw = lower(source, "scroll-bar.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("!xr.scrollbarpolicy")
    );
    assert!(compile(source, "scroll-bar.xr").is_ok());
    let invalid_axis = raw.replace("kind = \"ScrollView\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid_axis)
            .unwrap_err()
            .contains("on a scrollable axis")
    );
    let invalid_policy = raw.replace("value = \"always-on\"", "value = \"temporary\"");
    assert_ne!(invalid_policy, raw);
    assert!(mlir::process(&invalid_policy).is_err());
    let invalid_extent = source.replace("content-height: 500px", "content-height: true");
    assert!(
        compile(&invalid_extent, "scroll-bar.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn drawer_checks_the_edge_and_its_open_link_in_mlir() {
    let source = r#"
        component App inherits Window {
            in-out property <bool> showing: false;
            in property <DrawerEdge> side: DrawerEdge.right;
            drawer := Drawer {
                open <=> root.showing;
                edge: root.side;
                drag-margin: 16px;
                opened() => {}
                closed() => {}
            }
        }
    "#;
    let raw = lower(source, "drawer.xr");
    let optimized = mlir::process(&raw).unwrap().to_mlir();
    assert!(optimized.contains("!xr.draweredge"));
    assert!(optimized.contains("xr.control_link"));
    assert!(compile(source, "drawer.xr").is_ok());
    let invalid_edge = raw.replace("value = \"right\"", "value = \"center\"");
    assert_ne!(invalid_edge, raw);
    assert!(mlir::process(&invalid_edge).is_err());
    let invalid_event = raw.replace("name = \"closed\"", "name = \"rejected\"");
    assert!(
        mlir::process(&invalid_event)
            .unwrap_err()
            .contains("requires opened() or closed()")
    );
    let invalid_margin = source.replace("drag-margin: 16px", "drag-margin: true");
    assert!(
        compile(&invalid_margin, "drawer.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn swipe_view_verifies_static_pages_and_its_index_link() {
    let source = r#"
        component App inherits Window {
            in-out property <int> page: 0;
            views := SwipeView {
                current-index <=> root.page;
                orientation: vertical;
                interactive: true;
                changed(index) => {}
                SwipePage { Text { text: "first"; } }
                SwipePage { visible: false; Text { text: "second"; } }
            }
        }
    "#;
    let raw = lower(source, "swipe.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("xr.control_link")
    );
    assert!(compile(source, "swipe.xr").is_ok());
    let invalid_child = raw.replace("kind = \"SwipePage\"", "kind = \"Rectangle\"");
    assert!(
        mlir::process(&invalid_child)
            .unwrap_err()
            .contains("requires a SwipePage child")
    );
    let repeat = source.replace("SwipePage { Text", "for page in 3 : SwipePage { Text");
    assert!(
        compile(&repeat, "swipe.xr")
            .err()
            .unwrap()
            .contains("static SwipePage children")
    );
    let invalid_event = raw.replace("name = \"changed\"", "name = \"released\"");
    assert!(
        mlir::process(&invalid_event)
            .unwrap_err()
            .contains("one int parameter")
    );
}

#[test]
fn page_indicator_verifies_count_style_and_index_binding() {
    let source = r#"
        component App inherits Window {
            in-out property <int> page: 0;
            indicator := PageIndicator {
                count: 8;
                current-index <=> root.page;
                orientation: vertical;
                dot-size: 10px;
                spacing: 4px;
                color: #abc;
                current-color: #345;
                interactive: true;
                changed(index) => {}
            }
        }
    "#;
    let raw = lower(source, "indicator.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("xr.control_link")
    );
    assert!(compile(source, "indicator.xr").is_ok());
    let invalid_type = raw.replace("name = \"count\"", "name = \"dot-size\"");
    assert!(
        mlir::process(&invalid_type)
            .unwrap_err()
            .contains("incorrect PageIndicator property type")
    );
    let invalid_child = source.replace("count: 8;", "count: 8; Rectangle {}");
    assert!(
        compile(&invalid_child, "indicator.xr")
            .err()
            .unwrap()
            .contains("only accepts Tooltip children")
    );
}

#[test]
fn stack_navigation_uses_verified_effects_and_named_routes() {
    let source = r#"
        export component App inherits Window {
            go := Button {
                clicked() => { nav.push("detail"); }
            }
            nav := StackView {
                initial-page: "home";
                transition-duration: 180;
                changed(page, depth) => {}
                home := StackPage {}
                detail := StackPage {}
            }
        }
    "#;
    let raw = lower(source, "stack.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("xr.navigate")
    );
    assert!(compile(source, "stack.xr").is_ok());
    let bad_duration = source.replace("transition-duration: 180", "transition-duration: true");
    assert!(
        compile(&bad_duration, "stack.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    let bad_native_duration =
        raw.replace("name = \"initial-page\"", "name = \"transition-duration\"");
    assert!(
        mlir::process(&bad_native_duration)
            .unwrap_err()
            .contains("transition-duration requires an integer")
    );
    let bad_owner = raw.replace("owner = \"nav\"", "owner = \"root\"");
    assert!(
        mlir::process(&bad_owner)
            .unwrap_err()
            .contains("requires a StackView owner")
    );
    let bad_args = raw.replace("method = \"push\"", "method = \"pop\"");
    assert!(
        mlir::process(&bad_args)
            .unwrap_err()
            .contains("navigation method or arguments")
    );
    let bad_route = source.replace("push(\"detail\")", "push(\"missing\")");
    assert!(
        compile(&bad_route, "stack.xr")
            .err()
            .unwrap()
            .contains("unknown StackPage route")
    );
    let pure = source.replace(
        "go := Button {",
        "pure callback probe() -> int; probe() => { nav.pop(); 0 } go := Button {",
    );
    assert!(
        compile(&pure, "stack.xr")
            .err()
            .unwrap()
            .contains("not allowed in a pure callback")
    );
}

#[test]
fn stack_state_reads_are_typed_and_detect_initialization_cycles() {
    let source = r#"
        component App inherits Window {
            out property <int> level: nav.depth;
            pure callback route() -> string;
            route() => { nav.current-page }
            nav := StackView {
                initial-page: "home";
                home := StackPage {}
                next := StackPage {}
            }
            back := Button { enabled: nav.can-go-back; }
        }
    "#;
    let raw = lower(source, "stack-read.xr");
    assert_eq!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .matches("xr.stack_read")
            .count(),
        3
    );
    assert!(compile(source, "stack-read.xr").is_ok());
    let invalid_type = raw.replace("value = \"depth\"", "value = \"current-page\"");
    assert!(
        mlir::process(&invalid_type)
            .unwrap_err()
            .contains("typed StackView state property")
    );
    let write = source.replace("initial-page: \"home\";", "depth: 1;");
    assert!(
        compile(&write, "stack-read.xr")
            .err()
            .unwrap()
            .contains("read-only")
    );
    for initializer in [
        "nav.current-page",
        "root.route()",
        "nav.depth > 0 ? \"home\" : \"next\"",
    ] {
        let cycle = source.replace(
            "initial-page: \"home\"",
            &format!("initial-page: {initializer}"),
        );
        assert!(
            compile(&cycle, "stack-read.xr")
                .err()
                .unwrap()
                .contains("cyclic")
        );
    }
    let cross_component = r#"
        component Child inherits StackView {
            in property <string> seed: "home";
            out property <int> level: root.depth;
            initial-page: root.seed;
            home := StackPage {}
        }
        component App inherits Window {
            child := Child { seed: child.level > 0 ? "home" : "home"; }
        }
    "#;
    assert!(
        compile(cross_component, "stack-cycle.xr")
            .err()
            .unwrap()
            .contains("cyclic")
    );
}

#[test]
fn stack_path_operations_verify_list_arguments_and_literal_routes() {
    let source = r#"
        component App inherits Window {
            go := Button { clicked() => { nav.reset-to(["home", "detail"]); } }
            nav := StackView {
                home := StackPage {}
                detail := StackPage {}
            }
        }
    "#;
    let raw = lower(source, "stack-path.xr");
    assert!(
        mlir::process(&raw)
            .unwrap()
            .to_mlir()
            .contains("method = \"reset-to\"")
    );
    assert!(compile(source, "stack-path.xr").is_ok());
    let invalid_method = raw.replace("method = \"reset-to\"", "method = \"push\"");
    assert!(
        mlir::process(&invalid_method)
            .unwrap_err()
            .contains("navigation method or arguments")
    );
    let duplicate = source.replace("[\"home\", \"detail\"]", "[\"home\", \"home\"]");
    assert!(
        compile(&duplicate, "stack-path.xr")
            .err()
            .unwrap()
            .contains("duplicate StackPage route")
    );
}

#[test]
fn exported_record_types_and_literals_resolve_through_import_aliases() {
    let directory = std::env::var_os("TEST_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("xrack-record-import-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(
        directory.join("types.xr"),
        "export struct Data { label: string }",
    )
    .unwrap();
    let source = r#"import { Data as Row } from "types.xr";
        export component App inherits Window {
            in property <[Row]> rows: [Row { label: "one" }];
            VerticalLayout { for item in root.rows : Text { text: item.label; } }
        }
    "#;
    let result = compile(source, &directory.join("app.xr").to_string_lossy());
    std::fs::remove_dir_all(directory).unwrap();
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn selection_groups_enforce_child_structure() {
    for (body, expected) in [
        (
            "Tab { title: \"outside\"; }",
            "matching selection container",
        ),
        ("TabWidget { Button {} }", "requires static Tab children"),
        (
            "TabButton { text: \"outside\"; }",
            "matching selection container",
        ),
        ("TabBar { Tab {} }", "requires static TabButton children"),
        (
            "TabBar { TabButton { Rectangle {} } }",
            "TabButton cannot contain child elements",
        ),
        (
            "TabBar { TabButton {} for option in [\"a\"] : TabButton { text: option; } }",
            "repeated TabButton must be the only option",
        ),
        (
            "RadioGroup { if true : RadioButton {} }",
            "requires static RadioButton children",
        ),
        (
            "RadioGroup { RadioButton {} for option in [\"a\"] : RadioButton { text: option; } }",
            "repeated RadioButton must be the only option",
        ),
    ] {
        let source = format!("component Demo inherits Window {{ {body} }}");
        let error = compile(&source, "groups.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn repeated_components_require_a_direct_list_view_slot() {
    for (source, expected) in [
        (
            "component RowEditor inherits Rectangle {} component App inherits Window { for row in 3 : RowEditor {} }",
            "require an anonymous ListView template",
        ),
        (
            "component RowEditor inherits Rectangle {} component App inherits ListView { for row in 3 : named := RowEditor {} }",
            "require an anonymous ListView template",
        ),
        (
            "component Inner inherits ListView { for row in 2 : Text {} } component RowEditor inherits Rectangle { Inner {} } component App inherits ListView { for row in 3 : RowEditor {} }",
            "cannot contain another ListView",
        ),
    ] {
        let error = compile(source, "delegates.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn dialog_open_and_button_role_are_typed() {
    let source = r#"export component Confirm inherits Dialog {
        in-out property <bool> showing: false;
        open <=> root.showing;
        accepted() => { }
        DialogButton { role: DialogRole.accept; }
    }"#;
    let result = compile(source, "dialog.xr").unwrap();
    assert!(result.mlir.contains("!xr.dialogrole"));
    assert!(result.mlir.contains("xr.control_link"));
    assert!(
        compile(&source.replace("DialogRole.accept", "true"), "dialog.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
    assert!(
        compile(
            &source.replace("accepted()", "accepted(value)"),
            "dialog.xr"
        )
        .err()
        .unwrap()
        .contains("takes no parameters")
    );
}

#[test]
fn multiline_edit_has_a_typed_wrap_and_edit_callback() {
    let source = r#"export component Editor inherits TextArea {
        in-out property <string> value;
        wrap: TextWrap.char-wrap;
        text <=> root.value;
        edited(text) => { root.value = text; }
    }"#;
    let result = compile(source, "editor.xr").unwrap();
    assert!(result.mlir.contains("!xr.textwrap"));
    assert!(result.mlir.contains("xr.control_link"));
    assert!(
        compile(&source.replace("edited(text)", "edited()"), "editor.xr")
            .err()
            .unwrap()
            .contains("expects one string parameter")
    );
    assert!(
        compile(&source.replace("TextWrap.char-wrap", "true"), "editor.xr")
            .err()
            .unwrap()
            .contains("type mismatch")
    );
}

#[test]
fn numeric_and_choice_controls_validate_bindings_and_events() {
    let source = r#"export component Demo inherits Window {
        in-out property <int> quantity: 2;
        in-out property <int> selection-index: 0;
        spin := SpinBox { value <=> root.quantity; edited(value) => { root.quantity = value; } }
        picker := ComboBox { model: ["alpha"]; current-index <=> root.selection-index; selected(value) => {} }
    }"#;
    let compilation = compile(source, "selection.xr").unwrap();
    assert!(compilation.mlir.contains("!xr.stringlist"));
    assert!(compilation.mlir.contains("\"xr.control_link\""));
    let bad = source.replace("edited(value)", "edited()");
    assert!(
        compile(&bad, "selection.xr")
            .err()
            .unwrap()
            .contains("expects one int parameter")
    );
    let bad = source.replace(
        "current-index <=> root.selection-index;",
        "current-index <=> root.selection-index; current-value: \"alpha\";",
    );
    assert!(
        compile(&bad, "selection.xr")
            .err()
            .unwrap()
            .contains("either current-index or current-value")
    );
}

#[test]
fn slider_callbacks_and_progress_ratios_have_typed_mlir() {
    let source = r#"export component Demo inherits Window {
        in-out property <float> level: 12;
        adjustment := Slider {
            orientation: Orientation.vertical;
            value <=> root.level;
            changed(value) => { root.level = value; }
        }
        ProgressIndicator { progress: 65%; }
    }"#;
    let compilation = compile(source, "controls.xr").unwrap();
    assert!(compilation.mlir.contains("!xr.orientation"));
    assert!(compilation.mlir.contains("\"xr.to_ratio\""));
    assert!(compilation.mlir.contains("\"xr.control_link\""));
    let bad = source.replace("changed(value)", "changed()");
    assert!(
        compile(&bad, "controls.xr")
            .err()
            .unwrap()
            .contains("expects one float parameter")
    );
    let bad = source.replace("property <float> level", "property <int> level");
    assert!(
        compile(&bad, "controls.xr")
            .err()
            .unwrap()
            .contains("writable float property")
    );
}

#[test]
fn callback_calls_enforce_signatures_and_purity() {
    for (body, expected) in [
        (
            "callback count() -> int; property <int> value: count();",
            "impure callback",
        ),
        (
            "pure callback count(int) -> int; property <int> value: count();",
            "expects 1 arguments",
        ),
        (
            "pure callback count(int) -> int; property <int> value: count(true);",
            "expected int",
        ),
        (
            "pure callback ping; property <int> value: ping();",
            "no return value",
        ),
        (
            "property <int> n; pure callback ping; ping => { root.n = 3; }",
            "assignment is not allowed in a pure callback",
        ),
        (
            "pure callback count() -> int; count => { root.n } property <int> n: count();",
            "cyclic property binding or callback call",
        ),
        (
            "callback ping; ping => { root.pong() } callback pong; pong => { root.ping(); }",
            "cyclic property binding or callback call",
        ),
    ] {
        let source = format!("component Demo inherits Window {{ {body} }}");
        let error = compile(&source, "calls.xr").err().unwrap();
        assert!(error.contains(expected), "{error}");
    }

    let source = r#"export component Demo inherits Window {
        pure callback number(int) -> int;
        pure callback text(string) -> string;
        callback read-number(int) -> int;
        out property <int> result: number(3);
    }"#;
    let compilation = compile(source, "calls.xr").unwrap();
    assert!(compilation.mlir.contains("\"xr.call\""));
    assert!(
        compilation
            .mlir
            .contains("owner = \"root\", value = \"number\"")
    );
    for (target, expected) in [
        ("text", "incorrect argument type"),
        ("read-number", "impure callback in a pure context"),
        ("missing", "unknown callback"),
    ] {
        let corrupted = compilation.mlir.replace(
            "owner = \"root\", value = \"number\"",
            &format!("owner = \"root\", value = \"{target}\""),
        );
        let error = mlir::process(&corrupted).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }

    let cross_instance_cycle = r#"component Child inherits Rectangle {
        in property <int> input: 0;
        pure callback value() -> int;
        value => { input }
    }
    export component Demo inherits Window {
        property <int> result: child.value();
        child := Child { input: root.result; }
    }"#;
    let error = compile(cross_instance_cycle, "calls.xr").err().unwrap();
    assert!(
        error.contains("cyclic property binding or callback call"),
        "{error}"
    );
}

#[test]
fn component_instances_resolve_and_verify_typed_property_references() {
    let source = r#"
        export component App inherits Window {
            out property <int> result: child.result;
            child := Child { input: 3; max-width: 140px; }
        }
        component Child inherits Rectangle {
            in property <int> input;
            out property <int> result: local.value + input;
            local := Text { property <int> value: 2; text: "" + self.value; }
        }
    "#;
    let compilation = compile(source, "instances.xr").unwrap();
    assert!(compilation.mlir.contains("\"xr.instance\""));
    assert!(compilation.mlir.contains("\"xr.layout_bind\""));
    assert!(compilation.mlir.contains("component = @Child"));
    assert!(compilation.mlir.contains("owner = \"child\""));
    assert!(compilation.mlir.contains("owner = \"local\""));
    let parsed = mlir::process(&compilation.mlir).unwrap();
    assert_eq!(
        xrack_compiler::codegen::generate(&parsed)
            .unwrap()
            .to_string(),
        compilation.rust.to_string(),
    );
    for (from, to, expected) in [
        (
            "component = @Child",
            "component = @Missing",
            "unknown component",
        ),
        (
            "owner = \"child\"",
            "owner = \"missing\"",
            "unknown property owner",
        ),
        (
            "value = \"result\"",
            "value = \"missing\"",
            "unknown property",
        ),
        (
            "name = \"max-width\"",
            "name = \"bogus-width\"",
            "unknown instance layout property",
        ),
    ] {
        let err = mlir::process(&compilation.mlir.replace(from, to)).unwrap_err();
        assert!(err.contains(expected), "{err}");
    }
    let private = source.replace(
        "out property <int> result: local",
        "private property <int> result: local",
    );
    assert!(analyze(&private).unwrap_err().contains("private"));
    let read_only = source.replace("input: 3", "result: 3");
    assert!(analyze(&read_only).unwrap_err().contains("read-only"));
}

#[test]
fn rejects_recursive_instances_and_cross_instance_binding_cycles() {
    let recursive =
        "component A inherits Rectangle { B {} } component B inherits Rectangle { A {} }";
    assert!(
        analyze(recursive)
            .unwrap_err()
            .contains("cyclic component instantiation")
    );

    let cycle = r#"
        component App inherits Window {
            property <int> feedback: child.result;
            child := Child { input: root.feedback; }
        }
        component Child inherits Rectangle {
            in property <int> input;
            out property <int> result: input;
        }
    "#;
    assert!(
        analyze(cycle)
            .unwrap_err()
            .contains("cyclic property binding")
    );
}

#[test]
fn mlir_roundtrip_preserves_typed_expressions_and_unicode() {
    let source = r#"export component App inherits Window {
        in-out property <int> n: 2;
        out property <float> scale: n + 0.5;
        title: "你好\n\"MLIR\"";
        width: 100px * scale;
        callback add(step: int);
        add(step) => { root.n += step; }
    }"#;
    let text = lower(source, "中文 file.xr");
    let processed = mlir::process(&text).unwrap();
    let reparsed = mlir::process(&processed.to_mlir()).unwrap();
    let rust = compile(source, "test.xr").unwrap().rust.to_string();
    assert_eq!(
        xrack_compiler::codegen::generate(&reparsed)
            .unwrap()
            .to_string(),
        rust,
    );
    assert!(processed.to_mlir().contains("xr.component"));
    assert!(rust.contains("pub struct App"));
    assert!(rust.contains("pub fn invoke_add"));
    assert!(rust.contains("你好"));
}

#[test]
fn mlir_rejects_invalid_structure_and_unknown_operations() {
    assert!(mlir::process("this is not MLIR").is_err());
    assert!(mlir::process("module { \"xr.unknown\"() : () -> () }").is_err());
    assert!(mlir::process("module { %a = \"xr.read\"() {value = \"a\"} : () -> !xr.int\n \"xr.bind\"(%missing) {name = \"x\"} : (!xr.int) -> () }").is_err());
    let valid = lower(
        "component App inherits Window { property <int> count: 7; }",
        "test.xr",
    );
    assert!(mlir::process(&valid.replace("!xr.int", "!xr.unknown_type")).is_err());
    assert!(mlir::process(&valid.replace("name = \"count\"", "extra = \"count\"")).is_err());
    assert!(
        mlir::process(
            &valid
                .replace("value = \"7\"", "value = \"invalid\"")
                .as_str()
        )
        .unwrap_err()
        .contains("literal spelling")
    );
}

#[test]
fn multiple_components_forward_references_and_float_promotion() {
    let m = analyze("component A inherits Rectangle { property <int> derived: seed * 2; property <int> seed: 4; } export component B inherits Window { property <float> scale: 1; }").unwrap();
    assert_eq!(m.components.len(), 2);
    assert_eq!(m.components[1].properties[0].init.op, "cast");
}

#[test]
fn gradient_stops_are_typed_and_verified_in_mlir() {
    let source = r#"export component Gallery inherits Window {
        in property <color> accent: #123;
        property <brush> fill: @linear-gradient(-90deg, #000 20%, #fff, accent 80%);
        background: fill;
    }"#;
    let output = compile(source, "gallery.xr").unwrap();
    assert!(output.mlir.contains("\"xr.linear_gradient\""));
    assert!(output.mlir.contains("!xr.gradientstop"));
    assert!(output.mlir.contains("value = \"0.5\""));
    assert!(output.mlir.contains("value = \"-90\""));
    let malformed = output.mlir.replace("value = \"0.8\"", "value = \"1.8\"");
    assert!(mlir::process(&malformed).unwrap_err().contains("gradient"));

    for (stops, diagnostic) in [
        ("#fff 120%", "between 0% and 100%"),
        ("#fff 70%, #000 40%", "nondecreasing"),
        ("true 0%", "expected color"),
    ] {
        let source = format!(
            "component Gallery inherits Window {{ background: @radial-gradient(circle, {stops}); }}"
        );
        let err = compile(&source, "invalid-gradient.xr").err().unwrap();
        assert!(err.contains(diagnostic), "{err}");
    }
}

#[test]
fn image_and_path_bindings_report_resource_errors() {
    let source = r#"component Demo inherits Window {
        Image { source: @image-url("missing.png"); image-fit: contain; }
    }"#;
    let ir = mlir::process(&lower(source, "image.xr")).unwrap();
    assert!(ir.to_mlir().contains("\"xr.image_url\""));
    assert!(ir.to_mlir().contains("!xr.imagefit"));
    assert!(ir.to_mlir().contains("!xr.image"));
    assert!(
        compile(source, "image.xr")
            .err()
            .unwrap()
            .contains("missing.png")
    );
    let empty = source.replace("missing.png", "");
    assert!(compile(&empty, "image.xr").is_ok());

    let bad_path = r#"component Demo inherits Window { Path { commands: "M0 0 L10"; } }"#;
    assert!(
        compile(bad_path, "path.xr")
            .err()
            .unwrap()
            .contains("invalid Path commands")
    );
}

#[test]
fn native_frontend_and_mlir_are_safe_to_call_from_multiple_threads() {
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                compile(
                    "export component A inherits Window { title: \"ok\"; }",
                    "thread.xr",
                )
                .unwrap();
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
}
