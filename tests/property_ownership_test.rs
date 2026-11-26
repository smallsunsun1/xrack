#![cfg(test)]

use xrack_interpreter::{Instance, Program};
use xrack_runtime::{Component, Value};

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
    assert_eq!(actual.take_error(), None);
}
