#![cfg(test)]

use xrack_interpreter::{Instance, Program};
use xrack_runtime::{Component, Node, Value};

#[test]
fn strict_properties_keep_their_sources_in_both_backends() {
    use property_ownership_ui::PropertyOwnership;
    use std::time::Instant;
    use xrack_layout::{EditAction, EditSource, Interaction, Size};
    use xrack_runtime::{Length, Orientation::Vertical};

    let preview = xrack_compiler::prepare_preview(
        include_str!("property_ownership.xr"),
        "property_ownership.xr",
        Default::default(),
    )
    .unwrap();
    let mut actual = Program::from_module(&preview.module)
        .unwrap()
        .create("PropertyOwnership")
        .unwrap();
    let mut generated = PropertyOwnership::new();
    assert_eq!(actual.get("count"), Ok(3_i64.into()));
    assert_eq!(generated.get_count(), 3);
    actual.set("count", 7_i64.into()).unwrap();
    generated.set_count(7);
    assert_eq!(actual.get("count"), Ok(7_i64.into()));
    assert_eq!(generated.get_count(), 7);
    let titles = |actual: &Instance, generated: &PropertyOwnership, live, draft, display| {
        assert_eq!(generated.get_live_title(), live);
        assert_eq!(generated.get_draft_title(), draft);
        assert_eq!(generated.get_display_title(), display);
        for (name, value) in [
            ("live-title", live),
            ("draft-title", draft),
            ("display-title", display),
        ] {
            assert_eq!(actual.get(name), Ok(Value::String(String::from(value))));
        }
    };

    actual.invoke("rename", &["A".to_owned().into()]).unwrap();
    generated.invoke_rename("A".into());
    actual.set("document-title", "B".to_owned().into()).unwrap();
    generated.set_document_title("B".into());
    // The draft did not need to be read before the source changed.
    titles(&actual, &generated, "B", "A", "A");
    assert_eq!(actual.get("snapshot-title"), Ok("A".to_owned().into()));
    assert_eq!(generated.get_snapshot_title(), "A");
    for scene in [actual.render_shared(), generated.render_shared()] {
        assert_eq!(scene.find("snapshot-text").unwrap().text("text"), Some("A"));
    }
    actual
        .invoke("edit-snapshot", &["X".to_owned().into()])
        .unwrap();
    generated.invoke_edit_snapshot("X".into());
    assert_eq!(actual.get("snapshot-title"), Ok("X".to_owned().into()));
    assert_eq!(generated.get_snapshot_title(), "X");
    assert_eq!(actual.get("document-title"), Ok("B".to_owned().into()));
    assert_eq!(generated.get_document_title(), "B");
    assert!(actual.set("display-title", "X".to_owned().into()).is_err());
    assert!(actual.reset("document-title").is_err());

    actual.invoke("restore-default", &[]).unwrap();
    generated.invoke_restore_default();
    titles(&actual, &generated, "B", "A", "B");

    actual
        .invoke("request-child-rename", &["X".to_owned().into()])
        .unwrap();
    generated.invoke_request_child_rename("X".into());
    assert_eq!(actual.get("probe-title"), Ok("B".to_owned().into()));
    assert_eq!(generated.get_probe_title(), "B");
    assert_eq!(actual.get("requested-title"), Ok("X".to_owned().into()));
    assert_eq!(generated.get_requested_title(), "X");

    actual.set("draft", "draft".to_owned().into()).unwrap();
    generated.set_draft("draft".into());
    titles(&actual, &generated, "B", "draft", "B");

    fn edit(ui: &mut impl Component, path: &[usize]) {
        let mut input = Interaction::default();
        let (scene, frame) = xrack_layout::prepare_scene_with_input(
            ui,
            ui.render_shared(),
            Size::new(240.0, 320.0),
            &mut xrack_testing::FixedText,
            &input,
        );
        let target = input.accessibility_target(&scene, &frame, path).unwrap();
        input.focus_target(&scene, &frame, target);
        assert!(input.edit_text(
            ui,
            &scene,
            EditAction::Insert("!"),
            EditSource::Keyboard,
            Instant::now(),
        ));
    }

    edit(&mut actual, &[2]);
    edit(&mut generated, &[2]);
    assert!(generated.get_reported().contains('!'));
    assert_eq!(actual.get("reported"), Ok(generated.get_reported().into()));
    for scene in [actual.render_shared(), generated.render_shared()] {
        assert_eq!(scene.children[2].text("text"), Some("B"));
    }
    edit(&mut actual, &[3]);
    edit(&mut generated, &[3]);
    let draft = generated.get_draft_title();
    assert!(draft.contains('!'));
    titles(&actual, &generated, "B", &draft, "B");

    for scene in [actual.render_shared(), generated.render_shared()] {
        let fixed = scene.children[4].viewport_handle.as_ref().unwrap();
        let linked = scene.children[5].viewport_handle.as_ref().unwrap();
        assert_eq!(fixed.bound_offset(Vertical), Some(10.0));
        fixed.write_bound_offset(Vertical, 30.0);
        assert_eq!(fixed.bound_offset(Vertical), Some(10.0));
        linked.write_bound_offset(Vertical, 50.0);
        assert_eq!(linked.bound_offset(Vertical), Some(50.0));
        assert_eq!(fixed.bound_offset(Vertical), Some(50.0));
    }
    assert_eq!(actual.get("offset"), Ok(Length(-50.0).into()));
    assert_eq!(generated.get_offset(), Length(-50.0));

    edit(&mut actual, &[7]);
    edit(&mut generated, &[7]);
    actual.set("document-title", "C".to_owned().into()).unwrap();
    generated.set_document_title("C".into());
    for scene in [actual.render_shared(), generated.render_shared()] {
        let text = scene.find("snapshot-text").unwrap().text("text").unwrap();
        assert!(text.contains('A') && text.contains('!'));
        fn labels<'a>(node: &'a Node, result: &mut Vec<&'a str>) {
            result.extend(node.text("text"));
            for child in &node.children {
                labels(child, result);
            }
        }
        let mut rows = Vec::new();
        labels(scene.find("snapshot-rows").unwrap(), &mut rows);
        assert_eq!(rows, ["B0", "B1"]);
    }
    assert_eq!(actual.take_error(), None);
}

#[test]
fn language_blocks_match_in_the_interpreter_and_rust_aot() {
    use property_ownership_ui::LanguageBlocks;
    use std::{cell::Cell, rc::Rc};
    use xrack_layout::{Size, layout};
    use xrack_runtime::Length;

    let preview = xrack_compiler::prepare_preview(
        include_str!("property_ownership.xr"),
        "property_ownership.xr",
        Default::default(),
    )
    .unwrap();
    let mut interpreted = Program::from_module(&preview.module)
        .unwrap()
        .create("LanguageBlocks")
        .unwrap();
    let mut generated = LanguageBlocks::new();
    assert_eq!(generated.invoke_measure(Length(6.0)), Length(10.0));
    assert_eq!(
        interpreted.invoke("measure", &[Length(6.0).into()]),
        Ok(Some(Length(10.0).into()))
    );

    let aot_calls = Rc::new(Cell::new(0));
    let interp_calls = Rc::new(Cell::new(0));
    generated.on_row_count({
        let calls = aot_calls.clone();
        move || {
            calls.set(calls.get() + 1);
            2
        }
    });
    interpreted
        .on("row-count", {
            let calls = interp_calls.clone();
            Rc::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Some(2_i64.into()))
            })
        })
        .unwrap();

    assert_eq!(generated.get_message(), "Hidden");
    assert_eq!(interpreted.get("message"), Ok("Hidden".to_owned().into()));
    for scene in [generated.render(), interpreted.render()] {
        assert!(scene.find("hidden").is_some());
        assert!(scene.find("summary").is_none());
        assert!(scene.find("details").is_none());
    }
    assert_eq!(aot_calls.get(), 0);
    assert_eq!(interp_calls.get(), 0);

    generated.set_show_summary(true);
    interpreted.set("show-summary", true.into()).unwrap();
    assert_eq!(generated.get_message(), "Summary");
    assert_eq!(interpreted.get("message"), Ok("Summary".to_owned().into()));
    for scene in [generated.render(), interpreted.render()] {
        assert!(scene.find("hidden").is_none());
        assert!(scene.find("summary").is_some());
    }

    generated.invoke_flip();
    interpreted.invoke("flip", &[]).unwrap();
    assert_eq!(generated.get_message(), "Details");
    assert_eq!(interpreted.get("message"), Ok("Details".to_owned().into()));
    for scene in [generated.render(), interpreted.render()] {
        let parent = scene.find("controls").unwrap();
        let layout = layout(
            parent,
            Size::new(300.0, 40.0),
            &mut xrack_testing::FixedText,
        );
        let positions = parent
            .children
            .iter()
            .zip(&layout.children)
            .filter_map(|(node, layout)| {
                layout
                    .visible
                    .then_some((node.element_id(), layout.bounds.x))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            positions,
            [
                (Some("details"), 0.0),
                (Some("more"), 25.0),
                (Some("tail"), 50.0)
            ]
        );
    }
    assert!(aot_calls.get() > 0);
    assert!(interp_calls.get() > 0);

    fn rows(scene: &Node) -> &[Node] {
        &scene.find("repeated").unwrap().children[0].children
    }

    let before_aot = generated.render();
    let before_interp = interpreted.render();
    for scene in [&before_aot, &before_interp] {
        let rows = rows(scene);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].children[0].text("text"), Some("0 A"));
        assert_eq!(rows[0].children[1].text("text"), Some("A!"));
        assert_eq!(rows[1].children[0].text("text"), Some("1 B"));
        assert_eq!(rows[1].children[1].text("text"), Some("B!"));
    }
    generated.invoke_swap();
    interpreted.invoke("swap", &[]).unwrap();
    for (ui, before) in [
        (&mut generated as &mut dyn Component, &before_aot),
        (&mut interpreted, &before_interp),
    ] {
        let scene = ui.render();
        let moved = rows(&scene);
        assert_eq!(moved[0].children[0].text("text"), Some("0 B"));
        assert_eq!(moved[1].children[0].text("text"), Some("1 A"));
        assert!(moved[0].routing_row_key().is_some());
        assert_eq!(
            moved[0].routing_row_key(),
            rows(before)[1].routing_row_key()
        );
        let parent = scene
            .children
            .iter()
            .position(|child| child.element_id() == Some("repeated"))
            .unwrap();
        assert!(ui.dispatch_at(&scene, &[parent, 0, 0, 0], "clicked"));
    }
    assert_eq!(generated.get_selected(), "B");
    assert_eq!(interpreted.get("selected"), Ok("B".to_owned().into()));
    assert_eq!(interpreted.take_error(), None);
}
