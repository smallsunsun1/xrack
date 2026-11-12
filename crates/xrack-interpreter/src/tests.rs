use crate::*;
use std::cell::Cell;
use xrack_runtime::Component;

fn compile(source: &str) -> xrack_mlir::Module {
    let document = xrack_parser::parse(source).unwrap();
    xrack_mlir::lower_module(&document, "interpreter.xr").unwrap()
}

#[test]
fn execute_mlir_bindings_and_callbacks_after_releasing_module() {
    let source = r#"
        export component Counter inherits Window {
            in-out property <int> count: 0;
            out property <int> doubled: count * 2;
            callback increment(step: int);
            increment(step) => { root.count += step; }
            VerticalBox {
                Text { text: "" + root.doubled; }
                add := Button { text: "Add"; clicked => { root.increment(2); } }
            }
        }
    "#;
    let program = load_mlir(&compile(source).to_mlir()).unwrap();
    let mut first = program.create("Counter").unwrap();
    let second = program.create("Counter").unwrap();
    let observed = Rc::new(Cell::new(0));
    first
        .on("increment", {
            let observed = observed.clone();
            Rc::new(move |args| {
                assert_eq!(args, &[Value::Int(2)]);
                observed.set(observed.get() + 1);
                Ok(None)
            })
        })
        .unwrap();
    first.set("count", Value::Int(21)).unwrap();
    assert_eq!(first.get("doubled").unwrap(), Value::Int(42));
    assert!(first.set("doubled", Value::Int(0)).is_err());
    let frame = first.try_render().unwrap();
    assert_eq!(frame.at_path(&[0, 0]).unwrap().text("text"), Some("42"));
    assert!(first.dispatch_at(&frame, &[0, 1], "clicked"));
    assert_eq!(observed.get(), 1);
    assert_eq!(first.get("doubled").unwrap(), Value::Int(46));
    assert_eq!(second.get("count").unwrap(), Value::Int(0));
}

#[test]
fn hoisted_translation_tracks_locale_and_arguments() {
    let module = compile(
        r#"
        translations "catalog.toml";
        export component App inherits Window {
            in-out property <int> count: 1;
            for row in 3 : Text { text: @tr("label", count: root.count); }
        }
        "#,
    )
    .optimize()
    .unwrap();
    let catalog = TranslationCatalog::new(
        "en-US",
        &[
            ("en-US", "label = Count: { $count }"),
            ("zh-CN", "label = 数量：{ $count }"),
        ],
    );
    let ui = Program::from_module_with_translations(&module, Some(catalog))
        .unwrap()
        .create("App")
        .unwrap();
    let locale = xrack_runtime::LocaleContext::default();
    ui.set_locale_context(locale.clone());
    let check = |prefix: &str, count: char| {
        let scene = ui.try_render().unwrap();
        assert_eq!(scene.children[0].children.len(), 3);
        for row in &scene.children[0].children {
            let text = row.text("text").unwrap();
            assert!(text.starts_with(prefix) && text.contains(count), "{text}");
        }
    };
    check("Count:", '1');
    assert!(matches!(
        ui.visual_cache_state()
            .keys()
            .copied()
            .collect::<Vec<_>>()
            .as_slice(),
        [VisualCacheGroup::RepeatInvariant(_)]
    ));
    locale.set_locale("zh-CN").unwrap();
    check("数量：", '1');
    ui.set("count", Value::Int(2)).unwrap();
    check("数量：", '2');
}

#[test]
fn visual_caches_stay_lazy_and_use_the_component_or_row_scope() {
    let module = compile(
        r#"
        export component App inherits Window {
            in-out property <int> count: 1;
            in-out property <string> prefix: "A";
            in-out property <bool> show: false;
            in-out property <[string]> items: ["one", "two"];
            pure callback describe() -> string;
            out property <string> caption: root.describe();
            Text { text: "Count " + root.count; }
            Text { text: "Count " + root.count; }
            for item in root.items : VerticalLayout {
                Text { text: "Item " + item; }
                Text { text: "Item " + item; }
                Text { text: "Prefix " + root.prefix; }
                Text { text: root.show ? ("Caption " + root.caption + ": ") + item : item; }
                for inner in 1 : Text { text: "Prefix " + root.prefix; }
            }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let program = Program::from_module(&module).unwrap();
    let ui = program.create("App").unwrap();
    let another = program.create("App").unwrap();
    let calls = Rc::new(Cell::new(0));
    let install = |value: &'static str| {
        ui.on("describe", {
            let calls = calls.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Some(Value::String(value.into())))
            })
        })
        .unwrap();
    };
    install("first");
    let initial = ui.render();
    assert_eq!(
        initial.at_path(&[2, 0, 0]).unwrap().text("text"),
        Some("Item one")
    );
    assert_eq!(
        initial.at_path(&[2, 1, 0]).unwrap().text("text"),
        Some("Item two")
    );
    assert_eq!(calls.get(), 0);
    let root_slots = ui.visual_cache_state();
    assert_eq!(root_slots.len(), 2);
    assert!(
        root_slots
            .keys()
            .any(|group| matches!(group, VisualCacheGroup::Binding(_)))
    );
    assert!(
        root_slots
            .keys()
            .any(|group| matches!(group, VisualCacheGroup::RepeatInvariant(_)))
    );
    assert!(another.visual_cache_state().is_empty());

    let template = &ui.definition().root.children[2];
    let repeat = template.repeat.as_ref().unwrap();
    let items = ui.string_model("items").unwrap();
    let row = |index| {
        ui.repeat_state(template)
            .row(&ui, repeat, index, items.row_key(index).unwrap())
            .unwrap()
            .unwrap()
    };
    let first = row(0);
    let second = row(1);
    assert_eq!(first.visual_cache_state(), second.visual_cache_state());
    assert!(matches!(
        first.visual_cache_state().keys().next(),
        Some(VisualCacheGroup::Binding(_))
    ));
    assert_eq!(first.visual_cache_state().len(), 1);

    ui.set("count", Value::Int(3)).unwrap();
    assert!(
        ui.visual_cache_state()
            .iter()
            .any(|(group, dirty)| matches!(group, VisualCacheGroup::Binding(_)) && *dirty)
    );
    assert!(first.visual_cache_state().values().all(|dirty| !dirty));
    assert_eq!(another.render().children[0].text("text"), Some("Count 1"));

    ui.set("prefix", Value::String("B".into())).unwrap();
    items.set_row(0, "changed".into());
    ui.set("show", Value::Bool(true)).unwrap();
    let updated = ui.render();
    assert_eq!(updated.children[0].text("text"), Some("Count 3"));
    assert_eq!(
        updated.at_path(&[2, 0, 1]).unwrap().text("text"),
        Some("Item changed")
    );
    assert_eq!(
        updated.at_path(&[2, 1, 2]).unwrap().text("text"),
        Some("Prefix B")
    );
    assert_eq!(
        updated.at_path(&[2, 0, 3]).unwrap().text("text"),
        Some("Caption first: changed")
    );
    assert_eq!(
        updated.at_path(&[2, 1, 3]).unwrap().text("text"),
        Some("Caption first: two")
    );
    assert_eq!(
        updated.at_path(&[2, 1, 4, 0]).unwrap().text("text"),
        Some("Prefix B")
    );
    assert_eq!(calls.get(), 1);
    assert!(
        ui.visual_cache_state()
            .keys()
            .any(|group| matches!(group, VisualCacheGroup::RepeatSubexpression(_)))
    );

    ui.set("show", Value::Bool(false)).unwrap();
    ui.render();
    install("second");
    assert!(!ui.scene_dirty());
    ui.render();
    assert_eq!(calls.get(), 1);
    ui.set("show", Value::Bool(true)).unwrap();
    assert_eq!(
        ui.render().at_path(&[2, 0, 3]).unwrap().text("text"),
        Some("Caption second: changed")
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn child_instances_write_back_to_their_own_parent_input() {
    let module = compile(
        r#"
        component Field inherits LineEdit {
            in-out property <string> value: "";
            text <=> root.value;
        }
        export component App inherits Window {
            in-out property <string> left: "first";
            in-out property <string> right: "second";
            VerticalBox {
                Field { value <=> root.left; }
                Field { value <=> root.right; }
            }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let scene = ui.try_render().unwrap();
    assert_eq!(scene.at_path(&[0, 0]).unwrap().text("text"), Some("first"));
    assert!(ui.edit_at(&scene, &[0, 1], "edited".into()));
    assert_eq!(ui.get("left").unwrap(), Value::String("first".into()));
    assert_eq!(ui.get("right").unwrap(), Value::String("edited".into()));
    assert_eq!(
        ui.try_render()
            .unwrap()
            .at_path(&[0, 1])
            .unwrap()
            .text("text"),
        Some("edited")
    );
}

#[test]
fn inactive_branch_stays_lazy_and_errors_do_not_poison_other_exports() {
    let module = compile(
        r#"
        export component App inherits Window {
            in property <bool> ready: false;
            in property <int> denominator: 0;
            out property <int> result: ready ? 10 / denominator : 7;
        }
        export component Extra inherits Window {
            in property <int> denominator: 0;
            Text { text: "" + (10 / root.denominator); }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let program = Program::from_module(&module).unwrap();
    let ui = program.create("App").unwrap();
    assert_eq!(ui.get("result").unwrap(), Value::Int(7));
    ui.set("ready", Value::Bool(true)).unwrap();
    assert!(matches!(ui.get("result"), Err(Error::Evaluation(_))));
    ui.set("denominator", Value::Int(2)).unwrap();
    assert_eq!(ui.get("result").unwrap(), Value::Int(5));
    assert!(matches!(
        program.create("Extra").unwrap().try_render(),
        Err(Error::Evaluation(_))
    ));
    assert_eq!(ui.get("result").unwrap(), Value::Int(5));
}

#[test]
fn state_overrides_are_ordered_and_restore_live_component_inputs() {
    let module = compile(
        r#"
        component Badge inherits Rectangle {
            in-out property <string> caption: "Default";
            Text { text: root.caption; }
        }
        export component App inherits Window {
            in-out property <string> caption: "Ready";
            in-out property <bool> warning: false;
            in-out property <bool> urgent: false;
            badge := Badge { caption: root.caption; width: 100px; }
            states [
                warning when root.warning: {
                    badge.caption: "Warning";
                    badge.width: 160px;
                    in { animate badge.width { duration: 100ms; } }
                }
                urgent when root.urgent: { badge.caption: "Urgent"; badge.width: 200px; }
            ]
        }
    "#,
    )
    .optimize()
    .unwrap();
    let ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let assert_badge = |width, caption: &str| {
        let scene = ui.try_render().unwrap();
        let badge = scene.at_path(&[0]).unwrap();
        assert_eq!(badge.length("width"), Some(width));
        assert_eq!(badge.at_path(&[0]).unwrap().text("text"), Some(caption));
    };
    assert_badge(100.0, "Ready");
    ui.set("urgent", Value::Bool(true)).unwrap();
    assert_badge(200.0, "Urgent");
    let previous = ui.try_render().unwrap();
    ui.set("warning", Value::Bool(true)).unwrap();
    assert_badge(160.0, "Warning");
    let next = ui.try_render().unwrap();
    let transition = xrack_runtime::SceneTransition::between(&previous, &next);
    let midpoint = transition.frame(&next, std::time::Duration::from_millis(50));
    assert_eq!(midpoint.at_path(&[0]).unwrap().length("width"), Some(180.0));
    ui.set("caption", Value::String("Updated".into())).unwrap();
    ui.set("urgent", Value::Bool(false)).unwrap();
    assert_badge(160.0, "Warning");
    ui.set("warning", Value::Bool(false)).unwrap();
    assert_badge(100.0, "Updated");
}

#[test]
fn selected_combo_item_updates_binding_before_running_handler() {
    let module = compile(
        r#"
        export component App inherits Window {
            in-out property <string> choice: "one";
            out property <string> observed: "";
            picker := ComboBox {
                model: ["one", "two"];
                current-value <=> root.choice;
                selected(label) => { root.observed = label + "/" + root.choice; }
            }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let scene = ui.try_render().unwrap();
    assert!(ui.control_at(&scene, &[0], xrack_runtime::ControlAction::Select(1)));
    assert_eq!(ui.get("choice").unwrap(), Value::String("two".into()));
    assert_eq!(ui.get("observed").unwrap(), Value::String("two/two".into()));
    assert_eq!(
        xrack_runtime::combo_text(ui.try_render().unwrap().at_path(&[0]).unwrap()),
        "two"
    );
}

#[test]
fn spin_control_uses_runtime_wrapping_and_read_only_rules() {
    let module = compile(
        r#"
        export component App inherits Window {
            in-out property <int> count: 5;
            in-out property <bool> locked: false;
            out property <int> observed: -1;
            spin := SpinBox {
                minimum: 0;
                maximum: 5;
                wrap: true;
                value <=> root.count;
                read-only: root.locked;
                edited(next) => { root.observed = next; }
            }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let action = xrack_runtime::ControlAction::Spin(xrack_runtime::SpinAction::Increase);
    assert!(ui.control_at(&ui.try_render().unwrap(), &[0], action));
    assert_eq!(ui.get("count").unwrap(), Value::Int(0));
    assert_eq!(ui.get("observed").unwrap(), Value::Int(0));
    ui.set("locked", Value::Bool(true)).unwrap();
    assert!(!ui.control_at(&ui.try_render().unwrap(), &[0], action));
}

#[test]
fn virtual_delegate_retains_local_state_and_rejects_replaced_rows() {
    let module = compile(
        r#"
        component CounterRow inherits Button {
            in property <string> label;
            property <int> clicks: 0;
            text: root.label + ":" + root.clicks;
            clicked => { root.clicks += 1; }
        }
        export component App inherits Window {
            in-out property <[string]> items: ["A", "B", "C"];
            ListView {
                for item[index] in root.items : CounterRow { label: index + " " + item; }
            }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();

    let viewport = |ui: &Instance, index| {
        let mut scene = ui.try_render().unwrap().as_ref().clone();
        let list = &mut scene.children[0];
        list.list.as_mut().unwrap().first_row = index;
        list.children = ui
            .list_rows(&[0], index..index + 1)
            .into_iter()
            .map(|delegate| {
                let mut row = Node::new("ListRow", None);
                row.row_key = delegate.row_key;
                row.children.push(delegate);
                row
            })
            .collect();
        scene
    };

    let original_row = ui.list_rows(&[0], 1..2).pop().unwrap();
    let same_row = ui.list_rows(&[0], 1..2).pop().unwrap();
    assert!(original_row.shares_storage_with(&same_row));
    let previous = viewport(&ui, 1);
    let delegate = |node: &Node| {
        node.at_path(&[0, 0, 0])
            .unwrap()
            .text("text")
            .unwrap()
            .to_owned()
    };
    assert_eq!(delegate(&previous), "1 B:0");
    assert!(ui.dispatch_at(&previous, &[0, 0, 0], "clicked"));
    let changed_row = ui.list_rows(&[0], 1..2).pop().unwrap();
    assert_eq!(changed_row.text("text"), Some("1 B:1"));
    assert_eq!(delegate(&viewport(&ui, 2)), "2 C:0");
    let revisited = ui.list_rows(&[0], 1..2).pop().unwrap();
    assert_eq!(revisited.text("text"), Some("1 B:1"));
    assert!(!revisited.shares_storage_with(&changed_row));

    let items = ui.string_model("items").unwrap();
    assert!(items.sort_by(|a, b| (a != "B", a).cmp(&(b != "B", b))));
    let moved = ui.list_rows(&[0], 0..1).pop().unwrap();
    assert_eq!(moved.row_key, original_row.row_key);
    assert_eq!(moved.text("text"), Some("0 B:1"));
    assert!(!original_row.shares_storage_with(&moved));

    ui.set("items", Value::StringList(vec!["X".into(), "Y".into()]))
        .unwrap();
    assert!(!ui.dispatch_at(&previous, &[0, 0, 0], "clicked"));
    assert_eq!(delegate(&viewport(&ui, 1)), "1 Y:0");
}

#[test]
fn two_way_link_follows_forwarded_input_through_nested_aliases() {
    let module = compile(
        r#"
        component Entry inherits LineEdit {
            in-out property <string> value;
            text <=> root.value;
        }
        component Wrapper inherits Rectangle {
            in-out property <string> forwarded <=> field.value;
            field := Entry { value: "internal"; }
        }
        export component App inherits Window {
            in-out property <string> value: "parent";
            Wrapper { forwarded <=> root.value; }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let scene = ui.try_render().unwrap();
    let path = &[0, 0];
    assert_eq!(scene.at_path(path).unwrap().text("text"), Some("parent"));
    assert!(ui.edit_at(&scene, path, "edited".into()));
    assert_eq!(ui.get("value").unwrap(), Value::String("edited".into()));
    ui.set("value", Value::String("external".into())).unwrap();
    assert_eq!(
        ui.try_render().unwrap().at_path(path).unwrap().text("text"),
        Some("external")
    );
}

#[test]
fn record_row_links_update_only_the_live_nested_field() {
    let module = compile(
        r#"
        struct Flags { enabled: bool, retained: int }
        struct Entry { label: string, flags: Flags }
        export component App inherits Window {
            in-out property <[Entry]> records: [
                Entry { label: "one", flags: Flags { enabled: false, retained: 8 } },
                Entry { label: "two", flags: Flags { enabled: false, retained: 9 } }
            ];
            out property <int> edits: 0;
            ListView {
                row-height: 32px;
                for entry in root.records : CheckDelegate {
                    text: entry.label;
                    checked <=> entry.flags.enabled;
                    toggled => { root.edits += 1; }
                }
            }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let viewport = |ui: &Instance| {
        let mut scene = ui.try_render().unwrap().as_ref().clone();
        scene.children[0].children = ui
            .list_rows(&[0], 0..2)
            .into_iter()
            .map(|delegate| {
                let mut row = Node::new("ListRow", None);
                row.row_key = delegate.row_key;
                row.children.push(delegate);
                row
            })
            .collect();
        scene
    };
    let snapshot = viewport(&ui);
    let Value::RecordList(records) = ui.get("records").unwrap() else {
        unreachable!()
    };
    let model = records.model();
    let key = model.row_key(0).unwrap();
    let enabled = vec!["flags".into(), "enabled".into()];
    let retained = vec!["flags".into(), "retained".into()];
    assert!(ui.dispatch_at(&snapshot, &[0, 0, 0], "clicked"));
    let row = model.row(0).unwrap();
    assert_eq!(row.field_path(&enabled), Some(&Value::Bool(true)));
    assert_eq!(row.field_path(&retained), Some(&Value::Int(8)));
    assert_eq!(model.row_key(0), Some(key));
    assert_eq!(ui.get("edits").unwrap(), Value::Int(1));
    assert!(!ui.dispatch_at(&snapshot, &[0, 0, 0], "clicked"));
    assert_eq!(ui.get("edits").unwrap(), Value::Int(1));

    assert!(model.insert(0, model.row(1).unwrap()));
    assert!(!ui.dispatch_at(&snapshot, &[0, 0, 0], "clicked"));
    let updated = viewport(&ui);
    assert!(ui.dispatch_at(&updated, &[0, 1, 0], "clicked"));
    assert_eq!(model.row_key(1), Some(key));
    assert_eq!(
        model.row(1).unwrap().field_path(&enabled),
        Some(&Value::Bool(false))
    );
    assert_eq!(ui.get("edits").unwrap(), Value::Int(2));
}

#[test]
fn record_field_forwards_to_the_editable_component_input() {
    let module = compile(
        r#"
        struct Entry { label: string }
        component Editor inherits LineEdit {
            in-out property <string> label;
            text <=> root.label;
        }
        export component App inherits Window {
            in-out property <[Entry]> records: [Entry { label: "before" }];
            ListView {
                for entry in root.records : Editor { label <=> entry.label; }
            }
        }
    "#,
    )
    .optimize()
    .unwrap();
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let mut scene = ui.try_render().unwrap().as_ref().clone();
    let delegate = ui.list_rows(&[0], 0..1).pop().unwrap();
    let mut row = Node::new("ListRow", None);
    row.row_key = delegate.row_key;
    row.children.push(delegate);
    scene.children[0].children.push(row);
    assert!(ui.edit_at(&scene, &[0, 0, 0], "after".to_owned()));
    let Value::RecordList(records) = ui.get("records").unwrap() else {
        unreachable!()
    };
    assert_eq!(
        records.model().row(0).unwrap().field("label"),
        Some(&Value::String("after".into()))
    );
    let delegate = ui.list_rows(&[0], 0..1).pop().unwrap();
    assert_eq!(delegate.text("text"), Some("after"));
}

#[test]
fn custom_clipboard_parser_replays_only_changed_nested_fields() {
    let source = r#"
        export struct Details { label: string, enabled: bool }
        export struct PasteEntry { details: Details }
        struct PasteResult { accepted: bool, value: Details }

        component Cell inherits LineEdit {
            in-out property <Details> details;
            text: root.details.label;
        }

        export component App inherits Window {
            in-out property <[PasteEntry]> records: [
                PasteEntry { details: Details { label: "original", enabled: false } },
                PasteEntry { details: Details { label: "second", enabled: false } },
            ];
            view := TableView {
                pasteable: true;
                TableColumn { title: "Name"; }
                for entry in root.records : TableRow {
                    Cell {
                        details <=> entry.details;
                        clipboard-target: "details";
                        clipboard-paste(text) => {
                            PasteResult {
                                accepted: text != "reject",
                                value: Details { label: text, enabled: entry.details.enabled }
                            }
                        }
                    }
                }
            }
        }
    "#;
    let module = compile(source).optimize().unwrap();
    let ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let Value::RecordList(records) = ui.get("records").unwrap() else {
        panic!("expected record list");
    };
    let model = records.model();
    let label = ["details".into(), "label".into()];
    let enabled = ["details".into(), "enabled".into()];
    let paste = || {
        ui.prepare_table_paste(&ui.render_shared(), &[0], 0, vec![0])
            .unwrap()
    };
    let original = model.snapshot();
    assert!(!paste()("allowed\nreject"));
    assert_eq!(model.snapshot(), original);
    assert!(paste()("updated"));
    let changed = model.row(0).unwrap();
    assert_eq!(
        changed.field_path(&label),
        Some(&Value::String("updated".into()))
    );
    assert!(model.set_row(0, changed.with_field(&enabled, true.into()).unwrap()));
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &[0], false));
    let reverted = model.row(0).unwrap();
    assert_eq!(
        reverted.field_path(&label),
        Some(&Value::String("original".into()))
    );
    assert_eq!(reverted.field_path(&enabled), Some(&Value::Bool(true)));
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &[0], true));
    let replayed = model.row(0).unwrap();
    assert_eq!(
        replayed.field_path(&label),
        Some(&Value::String("updated".into()))
    );
    assert_eq!(replayed.field_path(&enabled), Some(&Value::Bool(true)));
}

#[test]
fn string_table_paste_replays_by_row_identity_after_reordering() {
    let source = r#"
        export component App inherits Window {
            in-out property <[string]> entries: ["alpha", "beta"];
            sheet := TableView {
                pasteable: true;
                edit-on-tab: true;
                TableColumn { title: "Entry"; }
                for entry in root.entries : TableRow { TableCell { text <=> entry; } }
            }
        }
    "#;
    let module = compile(source).optimize().unwrap();
    let ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let model = ui.string_model("entries").unwrap();
    let keys = [model.row_key(0), model.row_key(1)];
    let path = [0];
    let paste = || {
        ui.prepare_table_paste(&ui.render_shared(), &path, 0, vec![0])
            .unwrap()
    };
    assert!(paste()("zulu\nsecond"));
    assert_eq!(model.snapshot(), ["zulu", "second"]);
    assert!(model.sort_by(String::cmp));
    assert_eq!([model.row_key(0), model.row_key(1)], [keys[1], keys[0]]);
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &path, false));
    assert_eq!(model.snapshot(), ["beta", "alpha"]);
    assert!(ui.replay_table_edit_at(&ui.render_shared(), &path, true));
    assert_eq!(model.snapshot(), ["second", "zulu"]);
    let stale = paste();
    ui.set("entries", Value::StringList(model.snapshot()))
        .unwrap();
    assert!(!stale("replacement"));
}

#[test]
fn timer_immediate_and_completed_ticks_observe_running_in_the_right_order() {
    use std::time::{Duration, Instant};
    use xrack_runtime::TimerQueue;

    let source = r#"
        export component App inherits Window {
            out property <string> events: "";
            in-out property <bool> pending: true;
            pulse := Timer {
                interval: 100;
                running <=> root.pending;
                triggered-on-start: true;
                triggered() => { root.events = root.events + (root.pending ? "R" : "S"); }
            }
            Text { text: root.events; }
        }
    "#;
    let module = compile(source).optimize().unwrap();
    let instance = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let scene = instance.render_shared();
    assert_eq!(scene.children.len(), 1);
    assert_eq!(scene.timers.len(), 1);
    let started = Instant::now();
    let mut queue = TimerQueue::default();
    queue.sync(started, &scene.timers);
    assert!(queue.next_due(started).unwrap().fire());
    assert_eq!(instance.get("events"), Ok(Value::String("R".into())));
    assert!(
        queue
            .next_due(started + Duration::from_millis(99))
            .is_none()
    );
    assert!(
        queue
            .next_due(started + Duration::from_millis(100))
            .unwrap()
            .fire()
    );
    assert_eq!(instance.get("events"), Ok(Value::String("RS".into())));
    assert!(instance.render_shared().timers.is_empty());
}

#[test]
fn tumbler_literal_model_keeps_its_identity_between_render_and_selection() {
    use xrack_runtime::{ControlAction, Tumbler};

    let module = compile(
        r#"
        export component App inherits Window {
            in-out property <int> selected: 0;
            Tumbler {
                model: ["a", "b", "c"];
                current-index <=> root.selected;
            }
        }
    "#,
    );
    let mut ui = Program::from_module(&module)
        .unwrap()
        .create("App")
        .unwrap();
    let scene = ui.render();
    let action = Tumbler::from_node(scene.at_path(&[0]).unwrap())
        .select(2)
        .unwrap();
    assert!(matches!(action, ControlAction::PickTumbler { .. }));
    assert!(ui.control_at(&scene, &[0], action));
    assert_eq!(ui.get("selected").unwrap(), Value::Int(2));
}
