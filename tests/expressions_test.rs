use expressions_ui::{
    EmbeddedList, Expressions, FixedTextUpdates, GradientPlans, GridPlans, LazyList,
    LazyProperties, LocalAliasUpdates, ModelBindings, RadioByValue, ScenePlans, SelectionByValue,
    SharedVisuals,
};
use std::{cell::Cell, rc::Rc};
use xrack_runtime::{
    Color, Component, ControlAction, Length, Value, VecModel, combo_index, group_index,
};

#[test]
fn anonymous_static_scenes_and_gradient_stops_share_storage() {
    use std::sync::Arc;
    use xrack_runtime::Brush;

    let first = expressions_ui::InternedScenes::new().render_shared();
    let second = expressions_ui::InternedScenes::new().render_shared();
    assert!(first.children[0].shares_storage_with(&first.children[1]));
    assert!(first.children[0].shares_storage_with(&second.children[1]));
    let mut edited = first.children[0].clone();
    edited
        .properties
        .insert("text", "changed".to_owned().into());
    assert_eq!(second.children[1].text("text"), Some("Same"));
    assert!(!edited.shares_storage_with(&second.children[1]));
    fn stops(node: &xrack_runtime::Node) -> &Arc<[xrack_runtime::GradientStop]> {
        match node.properties.get("background").unwrap() {
            Value::Brush(Brush::Linear { stops, .. }) => stops,
            _ => panic!("expected linear gradient"),
        }
    }
    assert!(Arc::ptr_eq(
        stops(&first.children[2]),
        stops(&second.children[3])
    ));
    let before = first
        .find("observed")
        .unwrap()
        .geometry_handle
        .as_ref()
        .unwrap();
    let after = second
        .find("observed")
        .unwrap()
        .geometry_handle
        .as_ref()
        .unwrap();
    assert_ne!(before, after);
}

#[test]
fn fixed_image_source_updates_preserve_layout_but_intrinsic_sources_do_not() {
    use xrack_runtime::{Image, ImageFormat};

    let mut ui = expressions_ui::FixedImageUpdates::new();
    let before = ui.render_shared();
    let image = Image::Embedded {
        format: ImageFormat::Png,
        bytes: &[],
        size: [48, 24],
    };
    ui.set_picture(image.into());
    let fixed = ui.render_shared();
    assert_eq!(
        fixed.children[0].image_asset("source").unwrap().size(),
        [48, 24]
    );
    assert!(before.same_frame_inputs(&fixed));
    ui.set_intrinsic_picture(image.into());
    assert!(!fixed.same_frame_inputs(&ui.render_shared()));
}

#[test]
fn builtin_constant_projection_matches_runtime_time_normalization() {
    let mut ui = expressions_ui::StaticBuiltinValues::new();
    assert_eq!(
        (
            ui.get_fixed_hour(),
            ui.get_fixed_minute(),
            ui.get_fixed_second()
        ),
        (23, 0, 59)
    );
    assert_eq!(ui.get_live_hour(), 23);
    ui.set_live(xrack_runtime::Time::new(8, 0, 0).unwrap());
    assert_eq!(ui.get_live_hour(), 8);
    assert_eq!(ui.get_fixed_hour(), 23);
    assert_eq!(ui.get_date_day(), 12);
    assert_eq!(ui.get_font_size_value(), Length(18.0));
}

#[test]
fn empty_repeats_keep_slots_and_skip_rows_without_changing_dynamic_or_virtual_views() {
    let mut ui = expressions_ui::EmptyOrdinaryRepeats::new();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    ui.on_describe(move |index| {
        observed.set(observed.get() + 1);
        format!("row {index}")
    });
    let initial = ui.render_shared();
    assert_eq!(initial.children.len(), 7);
    assert!(
        initial.children[..4]
            .iter()
            .all(|child| child.kind == "Empty")
    );
    assert_eq!(initial.children[4].list.unwrap().row_count, 0);
    assert!(initial.children[5].has_natural_paint_order());
    assert_eq!(calls.get(), 0);
    ui.set_count(2);
    let scene = ui.render_shared();
    assert_eq!(scene.children[3].children[1].text("text"), Some("row 1"));
    assert_eq!(calls.get(), 2);
    assert!(ui.dispatch_at(&scene, &[6], "clicked"));
    assert_eq!(ui.get_clicks(), 1);
}

#[test]
fn record_projection_caches_invalidate_siblings_and_visible_rows() {
    let mut ui = expressions_ui::RecordVisualCaches::new();
    let before = ui.render_shared();
    assert_eq!(before.children[0].text("text"), Some("old end"));
    assert_eq!(before.children[1].text("text"), Some("old end"));
    assert_eq!(before.children[2].children[0].text("text"), Some("old A"));
    ui.set_style(expressions_ui::VisualStrings {
        prefix: "new ".into(),
        suffix: "start".into(),
    });
    let after = ui.render_shared();
    assert_eq!(after.children[0].text("text"), Some("new start"));
    assert_eq!(after.children[1].text("text"), Some("new start"));
    assert_eq!(after.children[2].children[1].text("text"), Some("new B"));
}

#[test]
fn materialized_state_uses_static_layout_and_skips_the_superseded_binding() {
    let mut ui = expressions_ui::StaticStatePlans::new();
    ui.on_fallback_width(|| panic!("inactive base binding was evaluated"));
    let initial = ui.render_shared();
    let grid = initial.find("grid").unwrap();
    assert!(grid.static_grid().is_some());
    let tile = initial.find("tile").unwrap();
    assert_eq!(tile.int("col"), Some(1));
    assert_eq!(tile.length("width"), Some(40.0));
    assert_eq!(initial.find("status").unwrap().length("height"), Some(24.0));

    ui.set_message("Updating…".into());
    let next = ui.render_shared();
    assert!(initial.same_frame_inputs(&next));
    assert!(tile.shares_storage_with(next.find("tile").unwrap()));
}

#[test]
fn collapsed_local_aliases_keep_the_public_endpoint_and_shared_state() {
    let mut ui = LocalAliasUpdates::new();
    assert_eq!((ui.get_exposed(), ui.get_observed()), (7, 7));
    ui.set_exposed(10);
    ui.invoke_step();
    assert_eq!((ui.get_exposed(), ui.get_observed()), (12, 12));
}

#[test]
fn fixed_text_updates_keep_the_generated_frame_layout_valid() {
    let mut ui = FixedTextUpdates::new();
    let before = ui.render_shared();
    ui.set_message("A longer status message".into());
    let updated = ui.render_shared();
    assert!(before.same_frame_inputs(&updated));
    for node in &updated.children[..2] {
        assert_eq!(node.text("text"), Some("A longer status message"));
    }
    ui.set_summary("A longer summary".into());
    assert!(!updated.same_frame_inputs(&ui.render_shared()));
}

#[test]
fn constant_record_projections_generate_typed_values() {
    let ui = constant_record_ui::ConstantRecord::new();
    assert_eq!(ui.get_answer(), 85);
    assert_eq!(ui.get_padding(), Length(12.0));
}

#[test]
fn shared_visual_bindings_invalidate_and_keep_repeat_rows_independent() {
    let mut ui = SharedVisuals::new();
    let reads = Rc::new(Cell::new(0));
    let observed = reads.clone();
    ui.on_describe(move || {
        observed.set(observed.get() + 1);
        "shared".into()
    });
    ui.set_items(Vec::new());
    assert_eq!(ui.render_shared().at_path(&[2]).unwrap().kind, "Empty");
    assert_eq!(reads.get(), 0);
    ui.set_items(vec!["A".into(), "B".into()]);

    let check = |ui: &SharedVisuals, count: &str, expected_rows: [&str; 2], caption: &str| {
        let scene = ui.render_shared();
        for id in ["first", "second"] {
            assert_eq!(scene.find(id).unwrap().text("text"), Some(count));
        }
        for (index, expected) in expected_rows.into_iter().enumerate() {
            let row = scene.at_path(&[2, index]).unwrap();
            assert_eq!(row.children.len(), 3);
            assert!(
                row.children[..2]
                    .iter()
                    .all(|node| node.text("text") == Some(expected))
            );
            assert_eq!(row.children[2].text("text"), Some(caption));
        }
        assert_eq!(scene.at_path(&[3]).unwrap().kind, "Empty");
        assert_eq!(scene.at_path(&[4]).unwrap().id, Some("tail"));
    };
    check(&ui, "Count 0", ["Row A", "Row B"], "Caption shared");
    ui.set_count(3);
    check(&ui, "Count 3", ["Row A", "Row B"], "Caption shared");
    ui.set_items(vec!["C".into(), "D".into()]);
    check(&ui, "Count 3", ["Row C", "Row D"], "Caption shared");
    assert_eq!(reads.get(), 1);
    ui.on_describe(|| "changed".into());
    check(&ui, "Count 3", ["Row C", "Row D"], "Caption changed");
}

#[test]
fn scene_plans_share_static_nodes_and_keep_hoisted_branches_lazy() {
    let mut ui = ScenePlans::new();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    ui.on_describe(move || {
        observed.set(observed.get() + 1);
        "shared".into()
    });
    let initial = ui.render_shared();
    assert_eq!(initial.at_path(&[2, 0]).unwrap().text("text"), Some("A"));
    assert_eq!(calls.get(), 0);

    ui.set_tint(Color(0x112233ff));
    let recolored = ui.render_shared();
    assert!(!Rc::ptr_eq(&initial, &recolored));
    assert!(initial.same_frame_inputs(&recolored));
    assert!(
        initial
            .find("panel")
            .unwrap()
            .shares_storage_with(recolored.find("panel").unwrap())
    );

    let mut detached = recolored.find("panel").unwrap().clone();
    detached.children[0]
        .properties
        .insert("text", Value::String("detached".into()));
    assert!(!detached.shares_storage_with(recolored.find("panel").unwrap()));
    assert!(detached.children[1].shares_storage_with(recolored.at_path(&[1, 1]).unwrap()));
    assert_eq!(
        recolored.at_path(&[1, 0]).unwrap().text("text"),
        Some("static label")
    );

    ui.set_swatch_width(Length(70.0));
    let resized = ui.render_shared();
    assert!(!recolored.same_frame_inputs(&resized));
    ui.set_show_caption(true);
    let visible = ui.render_shared();
    assert_eq!(calls.get(), 1);
    assert_eq!(
        visible.at_path(&[2, 0]).unwrap().text("text"),
        Some("Caption shared: A")
    );
    assert_eq!(
        visible.at_path(&[2, 1]).unwrap().text("text"),
        Some("Caption shared: B")
    );
}

#[test]
fn generated_gradients_reuse_fixed_stops_and_update_live_stops() {
    use std::sync::Arc;
    use xrack_runtime::Brush;

    let mut ui = GradientPlans::new();
    let before = ui.render();
    let brush =
        |scene: &xrack_runtime::Node, id| scene.find(id).unwrap().properties["background"].clone();
    let Value::Brush(Brush::Linear {
        stops: original, ..
    }) = brush(&before, "fixed")
    else {
        panic!("expected a linear brush");
    };
    ui.set_swatch_width(Length(60.0));
    ui.set_tint(Color(0x123456ff));
    let after = ui.render();
    assert_eq!(after.find("fixed").unwrap().length("width"), Some(60.0));
    let Value::Brush(Brush::Linear { stops: resized, .. }) = brush(&after, "fixed") else {
        panic!("expected a linear brush");
    };
    assert!(Arc::ptr_eq(&original, &resized));
    let other = GradientPlans::new().render();
    let Value::Brush(Brush::Linear { stops: other, .. }) = brush(&other, "fixed") else {
        panic!("expected a linear brush");
    };
    assert!(Arc::ptr_eq(&original, &other));
    let Value::Brush(Brush::Radial { stops }) = brush(&after, "live") else {
        panic!("expected a radial brush");
    };
    assert_eq!(stops[1].color, Color(0x123456ff));
}

#[test]
fn generated_static_grid_matches_dynamic_placement_and_invalidates_on_edits() {
    use xrack_layout::{Size, TextMeasurer, layout};

    struct Measure;
    impl TextMeasurer for Measure {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }

    let mut ui = GridPlans::new();
    let scene = ui.render();
    let plan = scene.children[0].static_grid().unwrap();
    assert_eq!(plan.len(), 4);
    assert_eq!(plan[0].position.rows, 2);
    assert_eq!(plan[1].position.cols, 2);
    assert_eq!(plan[2].child_index, Some(1));
    assert_eq!((plan[2].position.row, plan[2].position.col), (1, 1));
    assert!(scene.children[1].static_grid().is_none());

    let viewport = Size::new(160.0, 200.0);
    let expected = layout(&scene, viewport, &mut Measure);
    let mut fallback = scene.clone();
    fallback.children[0]
        .properties
        .insert("visible", Value::Bool(true));
    assert!(fallback.children[0].static_grid().is_none());
    assert_eq!(layout(&fallback, viewport, &mut Measure), expected);

    let mut edited = scene.clone();
    edited.children[0].children[0]
        .properties
        .insert("rowspan", Value::Int(1));
    assert!(edited.children[0].static_grid().is_none());
    let changed = layout(&edited, viewport, &mut Measure);
    assert_ne!(
        changed.at_path(&[0, 2, 1]).unwrap().bounds,
        expected.at_path(&[0, 2, 1]).unwrap().bounds
    );
    ui.set_show_extra(false);
    assert!(!ui.render().at_path(&[1, 0]).unwrap().is_visible());
}

#[test]
fn string_model_preserves_snapshot_and_callback_api() {
    let mut ui = ModelBindings::new();
    let model = VecModel::from(vec!["first".to_owned(), "second".to_owned()]);
    ui.bind_model_records(model.clone());
    assert!(ui.get_equal());
    assert_eq!(ui.get_mirrored(), ["first", "second"]);
    assert!(model.set_row(1, "changed".into()));
    assert_eq!(ui.get_records(), ["first", "changed"]);
    assert_eq!(ui.get_mirrored(), ["first", "changed"]);
    ui.on_mirror(|mut rows: Vec<String>| {
        rows.reverse();
        rows
    });
    assert_eq!(
        ui.invoke_mirror(vec!["one".into(), "two".into()]),
        ["two", "one"]
    );
    assert_eq!(ui.get_mirrored(), ["changed", "first"]);
    assert!(!ui.get_equal());
    ui.set_records(vec!["first".into(), "changed".into()]);
    assert!(model.set_row(0, "old model".into()));
    assert_eq!(ui.get_records(), ["first", "changed"]);
}

#[test]
fn virtual_string_rows_route_through_component_instances() {
    let mut ui = EmbeddedList::new();
    let scene = ui.render_shared();
    let records = scene.find("records").unwrap();
    assert!(records.children.is_empty());
    assert_eq!(records.list.unwrap().row_count, 3);
    let rows = ui.list_rows(&[0], 1..3);
    assert_eq!(rows[0].text("text"), Some("1: b"));
    assert_eq!(rows[1].text("text"), Some("2: c"));
    ui.set_items(vec!["a".into(), "updated".into()]);
    let scene = ui.render_shared();
    assert_eq!(scene.find("records").unwrap().list.unwrap().row_count, 2);
    let rows = ui.list_rows(&[0], 1..3);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text("text"), Some("1: updated"));
}

#[test]
fn radio_selection_by_value_projects_checked_state() {
    let mut ui = RadioByValue::new();
    let scene = ui.render_shared();
    assert_eq!(group_index(scene.find("group").unwrap()), Some(1));
    assert!(ui.control_at(&scene, &[0], ControlAction::Select(0)));
    assert_eq!(ui.get_selection(), "alpha");
    let scene = ui.render_shared();
    let options = &scene.find("group").unwrap().children;
    assert!(options[0].boolean("checked", false));
    assert!(!options[1].boolean("checked", false));
}

#[test]
fn combobox_resolves_and_updates_value_bindings() {
    let mut ui = SelectionByValue::new();
    let scene = ui.render_shared();
    assert_eq!(combo_index(scene.find("choice").unwrap()), Some(1));
    assert!(ui.control_at(&scene, &[0], ControlAction::Select(0)));
    assert_eq!(ui.get_selection(), "alpha");
    ui.set_selection("missing".into());
    assert_eq!(combo_index(ui.render().find("choice").unwrap()), None);
}

#[test]
fn generated_rust_preserves_expression_types_and_semantics() {
    let mut ui = Expressions::new();
    assert_eq!(ui.get_precedence(), 7);
    assert_eq!(ui.get_left_associative(), 5);
    assert_eq!(ui.get_promoted(), 3.5);
    assert_eq!(ui.get_ratio(), 3.0);
    assert_eq!(ui.get_scaled(), Length(16.0));
    assert_eq!(ui.get_scalar_left(), Length(3.0));
    assert_eq!(ui.get_negative(), Length(-3.0));
    assert_eq!(ui.get_combined(), Length(3.0));
    assert!(ui.get_comparison());
    assert!(ui.get_same_color());
    assert_eq!(ui.get_foreground(), Color(0x00ff0088));
    assert_eq!(ui.get_greeting(), "hello 你好\n\"UI\"");
    assert_eq!(ui.get_selected(), ui.get_greeting());
    assert_eq!(ui.get_numbers(), "n=3, f=3.5, ok=true");
    assert_eq!(ui.render().properties["width"], Value::Length(Length(16.0)));
    ui.invoke_update(1);
    assert_eq!(ui.get_promoted(), 1.5);
    assert!(!ui.get_comparison());
    assert_eq!(ui.get_selected(), "other");
    assert_eq!(ui.get_numbers(), "n=1, f=1.5, ok=false");
    assert_eq!(expressions_ui::String::new().get_value(), "ok");
    assert_eq!(expressions_ui::Option::new().get_value(), 7);
    assert_eq!(expressions_ui::r#type::new().get_value(), 1.0);
}

#[test]
fn ssa_codegen_keeps_lazy_branches_and_observes_prior_assignments() {
    let mut ui = Expressions::new();
    ui.set_base_value(0);
    assert_eq!(ui.get_safe_ratio(), 42);
    assert!(!ui.get_guarded());
    ui.invoke_step();
    assert_eq!(ui.get_base_value(), 2);
    assert_eq!(ui.get_safe_ratio(), 6);
    assert!(ui.get_guarded());
    ui.invoke_run_branch(false);
    assert_eq!(ui.get_calls(), 0);
    assert!(!ui.get_branch_result());
    ui.invoke_run_branch(true);
    assert_eq!(ui.get_calls(), 1);
    assert!(ui.get_branch_result());
    ui.invoke_run_equal_branches();
    assert_eq!(ui.get_calls(), 2);
    assert_eq!(ui.get_branch_value(), 7);
}

#[test]
fn generated_weak_component_mutates_shared_state_without_keeping_it_alive() {
    let ui = Expressions::new();
    let weak = ui.as_weak();
    weak.upgrade().unwrap().set_base_value(10);
    assert_eq!(ui.get_base_value(), 10);
    drop(ui);
    assert!(weak.upgrade().is_none());
}

#[test]
fn generated_bindings_are_lazy_across_instances_and_rendering() {
    let mut ui = LazyProperties::new();
    let observed = Rc::new(Cell::new(0));
    let formatted = Rc::new(Cell::new(0));
    let formatted_right = Rc::new(Cell::new(0));
    let count = observed.clone();
    ui.on_observe(move |value| {
        count.set(count.get() + 1);
        value
    });
    let count = formatted.clone();
    ui.on_format_value(move |value| {
        count.set(count.get() + 1);
        value.to_string()
    });
    let count = formatted_right.clone();
    ui.on_format_right(move |value| {
        count.set(count.get() + 1);
        value.to_string()
    });
    let label = |ui: &LazyProperties| {
        ui.render()
            .find("label")
            .unwrap()
            .text("text")
            .unwrap()
            .to_owned()
    };

    assert_eq!((observed.get(), formatted.get()), (0, 0));
    assert_eq!(ui.get_shown(), 6);
    assert_eq!(ui.get_shown(), 6);
    assert_eq!(label(&ui), "6");
    assert_eq!(label(&ui), "6");
    assert_eq!((observed.get(), formatted.get()), (1, 1));
    let original = ui.render_shared();
    assert!(Rc::ptr_eq(&original, &ui.render_shared()));
    ui.set_unused(42);
    assert!(Rc::ptr_eq(&original, &ui.render_shared()));

    ui.set_right(9);
    ui.set_left(3);
    assert_eq!(label(&ui), "6");
    assert_eq!((observed.get(), formatted.get()), (1, 1));
    assert_eq!(formatted_right.get(), 2);
    let right_changed = ui.render_shared();
    assert!(!Rc::ptr_eq(&original, &right_changed));

    ui.set_left(4);
    assert_eq!((observed.get(), formatted.get()), (1, 1));
    assert_eq!(ui.get_shown(), 8);
    assert_eq!((observed.get(), formatted.get()), (2, 1));
    assert_eq!(label(&ui), "8");
    assert_eq!(formatted_right.get(), 2);
    ui.set_use_left(false);
    assert_eq!(label(&ui), "18");
    ui.set_left(100);
    assert_eq!(label(&ui), "18");
    assert_eq!((observed.get(), formatted.get()), (3, 3));

    ui.set_result(9);
    ui.set_right(10);
    assert_eq!(label(&ui), "18");
    assert_eq!((observed.get(), formatted.get()), (3, 3));
    ui.reset_result();
    assert_eq!(label(&ui), "20");
    assert_eq!((observed.get(), formatted.get()), (4, 4));

    ui.on_observe(|value| value + 1);
    assert_eq!(ui.get_shown(), 22);
    assert_eq!(label(&ui), "22");
    ui.on_format_value(|value| format!("updated:{value}"));
    assert_eq!(label(&ui), "updated:22");
    assert_eq!((observed.get(), formatted.get()), (4, 5));
}

#[test]
fn repeat_bindings_update_only_rows_and_expressions_that_read_changed_inputs() {
    let mut ui = LazyList::new();
    let row_reads = Rc::new(Cell::new(0));
    let index_reads = Rc::new(Cell::new(0));
    let detail_reads = Rc::new(Cell::new(0));
    let count = row_reads.clone();
    ui.on_row_label(move |item, index, suffix| {
        count.set(count.get() + 1);
        format!("{index} {item}{suffix}")
    });
    let count = index_reads.clone();
    ui.on_row_index(move |index| {
        count.set(count.get() + 1);
        format!("#{index}")
    });
    let count = detail_reads.clone();
    ui.on_detail_label(move |item, index, detail| {
        count.set(count.get() + 1);
        format!("{item} {index}/{detail}")
    });
    let reads = || (row_reads.get(), index_reads.get(), detail_reads.get());
    let label = |scene: &xrack_runtime::Node, row| {
        scene
            .at_path(&[1, 0, row, 0])
            .unwrap()
            .text("text")
            .unwrap()
            .to_owned()
    };
    let mut items = vec!["row".to_owned(); 1000];
    ui.set_items(items.clone());
    assert_eq!(reads(), (0, 0, 0));
    let initial = ui.render_shared();
    assert_eq!(reads(), (1000, 1000, 1));
    assert_eq!(label(&initial, 2), "2 row!");
    assert!(Rc::ptr_eq(&initial, &ui.render_shared()));

    ui.set_unrelated(1);
    assert!(!Rc::ptr_eq(&initial, &ui.render_shared()));
    assert_eq!(reads(), (1000, 1000, 1));

    items[2] = "updated".into();
    ui.set_items(items.clone());
    assert_eq!(reads(), (1000, 1000, 1));
    let updated = ui.render_shared();
    assert_eq!(reads(), (1001, 1000, 2));
    assert_eq!(label(&updated, 1), "1 row");
    assert_eq!(label(&updated, 2), "2 updated!");
    assert_eq!(label(&initial, 2), "2 row!");
    assert_eq!(
        updated.at_path(&[1, 0, 2, 2, 0, 0]).unwrap().text("text"),
        Some("updated 2/0")
    );

    ui.set_suffix("?".into());
    assert_eq!(label(&ui.render_shared(), 2), "2 updated?");
    assert_eq!(reads(), (1002, 1000, 2));
    ui.set_use_suffix(false);
    let no_suffix = ui.render_shared();
    ui.set_suffix("unused".into());
    assert!(Rc::ptr_eq(&no_suffix, &ui.render_shared()));
    assert_eq!(reads(), (1003, 1000, 2));

    ui.set_show_list(false);
    let hidden = ui.render_shared();
    ui.set_details(2);
    assert!(Rc::ptr_eq(&hidden, &ui.render_shared()));
    assert_eq!(reads(), (1003, 1000, 2));
    ui.set_show_list(true);
    let visible = ui.render_shared();
    assert_eq!(
        visible.at_path(&[1, 0, 2, 2, 0, 1]).unwrap().text("text"),
        Some("updated 2/1")
    );
    assert_eq!(reads(), (1003, 1000, 3));

    ui.set_items(Vec::new());
    assert!(ui.render_shared().at_path(&[1, 0, 2]).is_none());
    ui.set_items(items[..3].to_vec());
    assert_eq!(label(&ui.render_shared(), 2), "2 updated");
    assert_eq!(reads(), (1006, 1003, 5));
    assert!(ui.model_items().set_row(2, "live update".into()));
    assert_eq!(reads(), (1006, 1003, 5));
    assert_eq!(label(&ui.render_shared(), 2), "2 live update");
    assert_eq!(reads(), (1007, 1003, 7));
}
