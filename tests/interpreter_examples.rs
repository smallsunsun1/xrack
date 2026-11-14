use super::{Component, Program, assert_same_scene, prepare_list};
use std::{collections::BTreeSet, panic::catch_unwind};
use xrack_layout::{Interaction, LayoutNode, Point, Size, Target};
use xrack_runtime::{Node, Orientation, RowSelection};

type Example = (&'static str, fn() -> Box<dyn Component>);

fn assert_same_layout(actual: &LayoutNode, expected: &LayoutNode, path: &mut Vec<usize>) {
    assert_eq!(actual.bounds, expected.bounds, "bounds at {path:?}");
    assert_eq!(actual.clip, expected.clip, "clip at {path:?}");
    assert_eq!(actual.visible, expected.visible, "visibility at {path:?}");
    assert_eq!(actual.enabled, expected.enabled, "enabled at {path:?}");
    assert_eq!(
        actual.tab_header, expected.tab_header,
        "tab header at {path:?}"
    );
    for axis in [Orientation::Horizontal, Orientation::Vertical] {
        assert_eq!(
            actual.scroll_on(axis),
            expected.scroll_on(axis),
            "scroll at {path:?}"
        );
    }
    assert!(
        actual.child_paint_order().eq(expected.child_paint_order()),
        "paint order at {path:?}"
    );
    assert_eq!(
        actual.children.len(),
        expected.children.len(),
        "child count at {path:?}"
    );
    for (index, (left, right)) in actual.children.iter().zip(&expected.children).enumerate() {
        path.push(index);
        assert_same_layout(left, right, path);
        path.pop();
    }
}

fn assert_same_target(actual: Option<&Target>, expected: Option<&Target>, context: &str) {
    type Location<'a> = (&'a [usize], Option<&'a str>, bool, Option<usize>);
    fn location(target: Option<&Target>) -> Option<Location<'_>> {
        let target = target?;
        Some((
            &target.path,
            target.element_id(),
            target.focusable,
            target.text_link_index(),
        ))
    }
    assert_eq!(location(actual), location(expected), "{context}");
}

fn assert_same_input_routes(
    actual: (&Node, &LayoutNode),
    expected: (&Node, &LayoutNode),
    size: Size,
) -> Vec<Point> {
    fn collect_centers(scene: &Node, layout: &LayoutNode, points: &mut Vec<Point>) {
        if !layout.visible || !layout.enabled {
            return;
        }
        if matches!(
            scene.kind,
            "Button" | "ToolButton" | "RoundButton" | "CheckBox" | "Switch"
        ) {
            let bounds = layout.visible_window_bounds(layout.bounds);
            if bounds.width > 0.0 && bounds.height > 0.0 {
                points.push(Point {
                    x: bounds.x + bounds.width / 2.0,
                    y: bounds.y + bounds.height / 2.0,
                });
            }
        }
        for (_, child, geometry) in layout.children_in_paint_order(scene) {
            collect_centers(child, geometry, points);
        }
    }

    let mut left = Interaction::default();
    let mut right = Interaction::default();
    left.reconcile(actual.0, actual.1);
    right.reconcile(expected.0, expected.1);
    for step in 0..16 {
        assert_eq!(
            left.focus_next(actual.0, actual.1, false),
            right.focus_next(expected.0, expected.1, false),
            "focus layout request at step {step}"
        );
        assert_same_target(left.focused.as_ref(), right.focused.as_ref(), "tab focus");
    }

    let mut points = Vec::new();
    collect_centers(actual.0, actual.1, &mut points);
    for row in 0..8 {
        for column in 0..8 {
            points.push(Point {
                x: (column as f32 + 0.5) * size.width / 8.0,
                y: (row as f32 + 0.5) * size.height / 8.0,
            });
        }
    }
    let mut visited = BTreeSet::new();
    let mut clicks = Vec::new();
    for point in points {
        left.pointer_move(actual.0, actual.1, point);
        right.pointer_move(expected.0, expected.1, point);
        assert_same_target(left.hovered.as_ref(), right.hovered.as_ref(), "pointer hit");
        if let Some(target) = &left.hovered
            && actual
                .0
                .interactive_at_path(&target.path)
                .is_some_and(|node| {
                    matches!(
                        node.kind,
                        "Button" | "ToolButton" | "RoundButton" | "CheckBox" | "Switch"
                    )
                })
            && visited.insert(target.path.clone())
        {
            clicks.push(point);
        }
    }
    clicks
}

fn activate_pointer(
    ui: &mut impl Component,
    scene: &Node,
    layout: &LayoutNode,
    point: Point,
) -> (Target, bool) {
    let mut input = Interaction::default();
    input.reconcile(scene, layout);
    input.pointer_move(scene, layout, point);
    input.pointer_press_in_scene(scene, layout);
    let target = input
        .pointer_release_target()
        .expect("actionable pointer target");
    if let Some(selection) =
        input.table_delegate_selection(scene, layout, &target, RowSelection::Replace)
    {
        ui.control_at(scene, &selection.path, selection.action);
    }
    let handled = ui.dispatch_at(scene, &target.path, "clicked");
    (target, handled)
}

fn example_program(
    mut preview: xrack_compiler::PreviewModule,
    file: &str,
    expected: &BTreeSet<&str>,
) -> Program {
    let catalog = preview.take_translation_catalog();
    let (program, _) =
        Program::from_module_with_file_resources(&preview.module, file, catalog).unwrap();
    let declared: BTreeSet<_> = preview.components.iter().map(String::as_str).collect();
    let exported: BTreeSet<_> = program
        .exported_components()
        .filter(|name| declared.contains(name))
        .collect();
    assert_eq!(*expected, exported, "{file} export inventory");
    program
}

fn compare_program(
    program: &Program,
    file: &str,
    examples: &[Example],
    failures: &mut Vec<String>,
) -> usize {
    let mut activated = 0;
    for &(name, construct) in examples {
        let comparison = catch_unwind(std::panic::AssertUnwindSafe(|| {
            let interpreted = program.create(name).unwrap();
            let compiled = construct();
            let scene = interpreted.try_render().unwrap();
            assert_same_scene(&scene, &compiled.render_shared());

            let dimension = |name, default| {
                scene
                    .length(name)
                    .filter(|&size| size > 0.0)
                    .unwrap_or(default)
                    .ceil()
                    .clamp(1.0, 2048.0)
            };
            let size = Size::new(dimension("width", 640.0), dimension("height", 480.0));
            let (actual, actual_layout) = prepare_list(&interpreted, &Interaction::default(), size);
            let (expected, expected_layout) =
                prepare_list(&compiled, &Interaction::default(), size);
            assert_same_scene(&actual, &expected);
            assert_same_layout(&actual_layout, &expected_layout, &mut Vec::new());
            assert_eq!(interpreted.take_error(), None, "preparing visible rows");
            let clicks = assert_same_input_routes(
                (&actual, &actual_layout),
                (&expected, &expected_layout),
                size,
            );
            for point in clicks {
                // Start each activation with a fresh model and component state.
                let mut interpreted = program.create(name).unwrap();
                let mut compiled = construct();
                let (actual, actual_layout) =
                    prepare_list(&interpreted, &Interaction::default(), size);
                let (expected, expected_layout) =
                    prepare_list(&compiled, &Interaction::default(), size);
                let (left, left_handled) =
                    activate_pointer(&mut interpreted, &actual, &actual_layout, point);
                let (right, right_handled) =
                    activate_pointer(&mut compiled, &expected, &expected_layout, point);
                assert_same_target(Some(&left), Some(&right), "click release");
                assert_eq!(
                    left_handled, right_handled,
                    "clicked dispatch at {:?}",
                    left.path
                );
                assert_same_scene(
                    &interpreted.try_render().unwrap(),
                    &compiled.render_shared(),
                );
                let (actual, actual_layout) =
                    prepare_list(&interpreted, &Interaction::default(), size);
                let (expected, expected_layout) =
                    prepare_list(&compiled, &Interaction::default(), size);
                assert_same_scene(&actual, &expected);
                assert_same_layout(&actual_layout, &expected_layout, &mut Vec::new());
                assert_eq!(
                    interpreted.take_error(),
                    None,
                    "preparing visible rows after activation"
                );
                activated += 1;
            }
        }));
        if let Err(error) = comparison {
            let message = error
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| error.downcast_ref::<&str>().copied())
                .unwrap_or("scene comparison panicked");
            failures.push(format!("{file} {name}\n{message}"));
        }
    }
    activated
}

fn compare_file(
    file: &str,
    source: &str,
    examples: &[Example],
    failures: &mut Vec<String>,
) -> usize {
    let preview = xrack_compiler::prepare_preview(source, file, Default::default()).unwrap();
    let expected: BTreeSet<_> = examples.iter().map(|(name, _)| *name).collect();
    let mlir = preview.module.to_mlir();
    let path = std::path::Path::new(file).with_extension("mlir");
    let mlir_file = path.to_str().unwrap();
    let from_text = xrack_compiler::prepare_mlir_preview(&mlir, mlir_file)
        .unwrap_or_else(|error| panic!("{mlir_file}: {error}"));
    let from_text = example_program(from_text, mlir_file, &expected);
    let from_xr = example_program(preview, file, &expected);
    compare_program(&from_xr, file, examples, failures)
        + compare_program(&from_text, mlir_file, examples, failures)
}

#[test]
fn all_example_exports_match_generated_scenes_layout_and_input_routes() {
    let mut failures = Vec::new();
    let mut activated = 0;
    macro_rules! check {
        ($file:literal, $module:ident, [$($component:ident),+ $(,)?]) => {
            activated += compare_file(
                concat!("examples/", $file),
                include_str!(concat!("../examples/", $file)),
                &[$((stringify!($component), || Box::new($module::$component::new()))),+],
                &mut failures,
            );
        };
    }
    check!("async_demo/async.xr", async_ui, [AsyncDemo]);
    check!("cargo-consumer/ui/counter.xr", cargo_counter_ui, [Counter]);
    check!("cargo-consumer/ui/label.xr", cargo_label_ui, [CounterLabel]);
    check!(
        "controls/controls.xr",
        controls_ui,
        [
            ControlsDemo,
            CalendarAgendaDemo,
            MonthGridDemo,
            CalendarDemo,
            DecimalSpinDemo,
            TumblerDemo,
            DelayDemo,
            CheckStateDemo,
            CheckGroupDemo,
            GroupBoxDemo,
            IndicatorsDemo,
            TimeDemo,
            ColorDemo,
            FontPickerDemo,
            ThemeDemo,
            TimerDemo,
            PaneFrameDemo,
            WheelControlsDemo,
            SnapSlidersDemo
        ]
    );
    check!(
        "counter/counter.xr",
        counter_ui,
        [Counter, DesktopCounter, SecondaryWindowDemo]
    );
    check!(
        "delegates/delegates.xr",
        delegates_ui,
        [DelegatesDemo, SwipeDemo]
    );
    check!(
        "dialog/dialog.xr",
        dialog_ui,
        [DialogDemo, StandardDialogDemo, PopupDemo]
    );
    check!(
        "form/form.xr",
        form_ui,
        [
            FileDialogDemo,
            MessageDialogDemo,
            CloseGuardDemo,
            BidiEditorDemo,
            EditorCommandsDemo,
            EditorMenuDemo,
            FormDemo,
            PasswordDemo,
            ValidationDemo,
            DragDropDemo,
            VirtualDragDemo,
            FileDropDemo
        ]
    );
    check!("form/widgets.xr", widgets_ui, [LabelCard]);
    check!("layout/bundled.xr", bundled_ui, [BundledFontsDemo]);
    check!("layout/fonts/sample.xr", font_sample_ui, [FontSample]);
    check!(
        "layout/layout.xr",
        layout_ui,
        [
            LayoutDemo,
            FlowDemo,
            FlexDemo,
            LabelsDemo,
            FontsDemo,
            AnimationsDemo,
            StatesDemo,
            ComponentStatesDemo,
            FramelessDemo,
            FocusScopesDemo,
            ScopedKeysDemo,
            StyledTextDemo,
            CompositingDemo,
            RectangleShadowDemo,
            ClippingDemo,
            FlickableDemo,
            SwipeGestureDemo,
            ScaleRotateDemo,
            TransformDemo,
            GeometryDemo,
            WheelDemo,
            HoverDemo,
            PointerDemo,
            PointerPassThroughDemo
        ]
    );
    check!(
        "list/list.xr",
        list_ui,
        [
            ListVisibilityDemo,
            NestedListsDemo,
            NestedDataDemo,
            ItemDelegatesDemo,
            ListDemo,
            ScrollBarsDemo,
            StandaloneScrollDemo,
            ScrollPositionDemo,
            LinkedScrollDemo,
            AnchoredListDemo,
            StandardListDemo,
            GridDemo
        ]
    );
    check!(
        "media/media.xr",
        media_ui,
        [
            MediaDemo,
            NineSliceDemo,
            AnimatedImageDemo,
            SpriteAtlasDemo,
            ImageColorizeDemo
        ]
    );
    check!(
        "menu/menu.xr",
        menu_ui,
        [MenuDemo, RoundToolbarDemo, ActionGroupDemo, ShortcutDemo]
    );
    check!(
        "navigation/navigation.xr",
        navigation_ui,
        [
            NavigationDemo,
            StackDemo,
            StackGuardDemo,
            TabBarDemo,
            ApplicationWindowDemo,
            PageDemo
        ]
    );
    check!("notes/notes.xr", notes_ui, [NotesDemo]);
    check!("painting/gallery.xr", painting_ui, [Gallery]);
    check!(
        "selection/selection.xr",
        selection_ui,
        [
            SelectionDemo,
            RecordComboDemo,
            GroupedRadiosDemo,
            EditableChoiceDemo
        ]
    );
    check!("slots/slots.xr", slots_ui, [SlotDemo, GlobalDemo]);
    check!(
        "split/split.xr",
        split_ui,
        [SplitDemo, CustomSplitDemo, FillSplitDemo]
    );
    check!(
        "stepper/stepper.xr",
        stepper_ui,
        [
            StepperDemo,
            LinkedControlsDemo,
            AliasDefaultsDemo,
            CallbackProbe
        ]
    );
    check!(
        "table/records.xr",
        records_ui,
        [RecordTableDemo, DelegateTableDemo, NestedRecordsDemo]
    );
    check!(
        "table/table.xr",
        table_ui,
        [
            TableDemo,
            RowVisibilityDemo,
            WideTableDemo,
            ProjectedRowsDemo,
            StandardTableDemo,
            TableSortViewsDemo
        ]
    );
    check!("tooltip/tooltip.xr", tooltip_ui, [TooltipDemo]);
    check!("tray/tray.xr", tray_ui, [TrayDemo, TrayWindow]);
    check!(
        "tree/tree.xr",
        tree_ui,
        [
            VisibilityTreeDemo,
            DelegateTreeDemo,
            SizedTreeDemo,
            TreeDemo,
            TreeCommandsDemo,
            TreeBranchesDemo
        ]
    );
    check!("video/video.xr", video_ui, [VideoDemo]);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    println!("Compared {activated} pointer activations across source and text MLIR inputs");
}
