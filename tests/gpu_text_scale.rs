use super::{assert_same_pixels, interpret_example};
use bundled_ui::BundledFontsDemo;
use menu_ui::MenuDemo;
use xrack_layout::{Interaction, Rect, Size, TextPosition};
use xrack_renderer_femtovg::{Renderer, Snapshot, TextSystem};
use xrack_runtime::{Component, Length, Node};
use xrack_testing::UiDriver;

#[test]
fn system_button_shapes_outline_transparent_buttons_without_relayout() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut window = Node::new("Window", None);
    window
        .properties
        .insert("background", xrack_runtime::Color(0x203047ff).into());
    for (id, y, enabled) in [("active", 12.0, true), ("disabled", 62.0, false)] {
        let mut button = Node::new("Button", Some(id));
        button.properties.insert("x", Length(12.0).into());
        button.properties.insert("y", Length(y).into());
        button.properties.insert("width", Length(164.0).into());
        button.properties.insert("height", Length(44.0).into());
        button.properties.insert("border-width", Length(0.0).into());
        button
            .properties
            .insert("background", xrack_runtime::Color(0).into());
        button.properties.insert("enabled", enabled.into());
        if enabled {
            button.properties.insert("padding", Length(0.0).into());
            let mut content = Node::new("ButtonContent", None);
            let mut surface = Node::new("Rectangle", None);
            surface.properties.insert("width", Length(164.0).into());
            surface.properties.insert("height", Length(44.0).into());
            surface
                .properties
                .insert("background", xrack_runtime::Color(0x203047ff).into());
            content.children.push(surface);
            button.children.push(content);
        }
        window.children.push(button);
    }
    let input = Interaction::default();
    let before = renderer.snapshot(&window, 220, 126, 1.0, &input).unwrap();
    assert!(renderer.set_system_button_shapes(true));
    let after = renderer.snapshot(&window, 220, 126, 1.0, &input).unwrap();
    assert_eq!(before.layout, after.layout);
    let offset = |y: usize| (y * 220 + 80) * 4;
    for y in [12, 62] {
        let index = offset(y);
        assert_ne!(
            &before.rgba[index..index + 4],
            &after.rgba[index..index + 4]
        );
        let interior = offset(y + 16);
        assert_eq!(
            &before.rgba[interior..interior + 4],
            &after.rgba[interior..interior + 4]
        );
    }
    assert_ne!(
        &after.rgba[offset(12)..offset(12) + 4],
        &after.rgba[offset(62)..offset(62) + 4]
    );
    assert!(renderer.set_system_button_shapes(false));
    let restored = renderer.snapshot(&window, 220, 126, 1.0, &input).unwrap();
    assert_same_pixels(&before, &restored);
}

#[test]
fn platform_text_scaling_remeasures_and_paints_both_execution_backends() {
    let mut renderer = pollster::block_on(Renderer::with_text(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
        TextSystem::bundled_only(),
    ))
    .unwrap();
    let source = BundledFontsDemo::new().render();
    let interpreter = interpret_example("examples/layout/bundled.xr")
        .create("BundledFontsDemo")
        .unwrap();
    let input = Interaction::default();
    let normal = renderer.snapshot(&source, 420, 190, 1.0, &input).unwrap();
    let normal_width = renderer
        .text
        .advance_node(source.find("regular").unwrap(), "WWWW");

    assert!(renderer.set_text_scale(1.5));
    let enlarged = renderer.snapshot(&source, 420, 190, 1.0, &input).unwrap();
    let enlarged_width = renderer
        .text
        .advance_node(source.find("regular").unwrap(), "WWWW");
    assert!((enlarged_width / normal_width - 1.5).abs() < 0.02);
    assert!(
        enlarged.layout.at_path(&[0, 0]).unwrap().bounds.height
            > normal.layout.at_path(&[0, 0]).unwrap().bounds.height
    );
    assert_ne!(normal.rgba, enlarged.rgba);
    let interpreted = renderer
        .snapshot(&interpreter.render(), 420, 190, 1.0, &input)
        .unwrap();
    assert_same_pixels(&enlarged, &interpreted);

    for invalid in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
        assert!(!renderer.set_text_scale(invalid));
    }
    assert!(!renderer.set_text_scale(1.5));
    assert!(renderer.set_text_scale(1.0));
    let restored = renderer.snapshot(&source, 420, 190, 1.0, &input).unwrap();
    assert_same_pixels(&normal, &restored);

    let mut window = Node::new("Window", None);
    let mut dialog = Node::new("Dialog", Some("dialog"));
    dialog.properties.insert("open", true.into());
    dialog
        .properties
        .insert("title", "Inspection date".to_owned().into());
    dialog.properties.insert("font-size", Length(20.0).into());
    dialog.children.push(Node::new("Calendar", None));
    window.children.push(dialog);
    let before = renderer.layout(&window, Size::new(440.0, 520.0));
    let before_title = before.children[0].dialog_title_height(&window.children[0]);
    assert!(renderer.set_text_scale(1.8));
    let after = renderer.layout(&window, Size::new(440.0, 520.0));
    let after_title = after.children[0].dialog_title_height(&window.children[0]);
    assert!(after_title > before_title);
    let before_dialog = &before.children[0];
    let after_dialog = &after.children[0];
    let before_calendar = before_dialog.children[0].bounds;
    let after_calendar = after_dialog.children[0].bounds;
    assert!(after_calendar.y - after_dialog.bounds.y > before_calendar.y - before_dialog.bounds.y);
    assert!(after_calendar.height > before_calendar.height);
}

#[test]
fn modal_titles_wrap_and_keep_their_ink_inside_the_reserved_header() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut window = Node::new("Window", None);
    let mut dialog = Node::new("Dialog", Some("details"));
    dialog.properties.insert("open", true.into());
    dialog.properties.insert("width", Length(222.0).into());
    dialog.properties.insert("height", Length(236.0).into());
    dialog.properties.insert(
        "title",
        "Calibration review details for maintenance"
            .to_owned()
            .into(),
    );
    dialog
        .properties
        .insert("color", xrack_runtime::Color(0xff00ffff).into());
    window.children.push(dialog);
    assert!(renderer.set_text_scale(2.6));
    let snapshot = renderer
        .snapshot(&window, 390, 600, 1.0, &Interaction::default())
        .unwrap();
    let panel = &snapshot.layout.children[0];
    let header_end = panel.bounds.y + panel.dialog_title_height(&window.children[0]);
    let lines = panel.dialog_title_lines();
    assert_eq!(lines.len(), 2);
    assert!(lines[1].ellipsis);
    let mut has_ink = [false; 2];
    let second_line = panel.bounds.y + lines[1].origin.y;
    for (index, pixel) in snapshot.rgba.chunks_exact(4).enumerate() {
        if pixel[0] > 170 && pixel[1] < 90 && pixel[2] > 170 {
            let x = (index % snapshot.width as usize) as f32 + 0.5;
            let y = (index / snapshot.width as usize) as f32 + 0.5;
            assert!(x >= panel.bounds.x && x < panel.bounds.x + panel.bounds.width);
            assert!(y >= panel.bounds.y && y < header_end);
            has_ink[usize::from(y >= second_line)] = true;
        }
    }
    assert_eq!(has_ink, [true, true]);
}

#[test]
fn group_titles_paint_two_lines_in_their_header() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    assert!(renderer.set_text_scale(2.6));
    for (kind, direction) in [
        ("Expander", xrack_runtime::LayoutDirection::LeftToRight),
        ("GroupBox", xrack_runtime::LayoutDirection::RightToLeft),
    ] {
        let mut window = Node::new("Window", None);
        let mut group = Node::new(kind, Some("settings"));
        group.properties.insert("width", Length(268.0).into());
        group.properties.insert("height", Length(160.0).into());
        group
            .properties
            .insert("layout-direction", direction.into());
        group.properties.insert("checkable", true.into());
        group
            .properties
            .insert("title", "Inspection options".to_owned().into());
        group
            .properties
            .insert("color", xrack_runtime::Color(0xff00ffff).into());
        window.children.push(group);
        let snapshot = renderer
            .snapshot(&window, 306, 194, 1.0, &Interaction::default())
            .unwrap();
        let panel = &snapshot.layout.children[0];
        let header = panel.group_header(&window.children[0]);
        let (_, lines) = panel.group_title().unwrap();
        assert_eq!(lines.len(), 2);
        let second_line = header.y + lines[1].origin.y;
        let mut has_ink = [false; 2];
        for (index, pixel) in snapshot.rgba.chunks_exact(4).enumerate() {
            if pixel[0] > 170 && pixel[1] < 90 && pixel[2] > 170 {
                let x = (index % snapshot.width as usize) as f32 + 0.5;
                let y = (index / snapshot.width as usize) as f32 + 0.5;
                assert!(header.contains(xrack_layout::Point { x, y }));
                if direction == xrack_runtime::LayoutDirection::RightToLeft {
                    assert!(x < header.x + header.width - 30.0);
                }
                has_ink[usize::from(y >= second_line)] = true;
            }
        }
        assert_eq!(has_ink, [true, true]);
    }
}

#[test]
fn platform_bold_text_remeasures_and_paints_both_execution_backends() {
    let mut renderer = pollster::block_on(Renderer::with_text(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
        TextSystem::bundled_only(),
    ))
    .unwrap();
    let source = BundledFontsDemo::new().render();
    let interpreter = interpret_example("examples/layout/bundled.xr")
        .create("BundledFontsDemo")
        .unwrap();
    let input = Interaction::default();
    let entry = source.find("entry").unwrap();
    let text = entry.text("text").unwrap();
    let end = TextPosition {
        offset: text.len(),
        trailing: true,
    };
    let normal = renderer.snapshot(&source, 420, 190, 1.0, &input).unwrap();
    let normal_caret = renderer.text.single_line_layout(entry, text).caret_x(end);

    assert!(renderer.set_system_bold_text(true));
    let bold = renderer.snapshot(&source, 420, 190, 1.0, &input).unwrap();
    let bold_caret = renderer.text.single_line_layout(entry, text).caret_x(end);
    assert!((bold_caret - normal_caret).abs() > 0.5);
    assert!((bold_caret - renderer.text.advance_node(entry, text)).abs() < 0.1);
    assert_ne!(normal.rgba, bold.rgba);
    let interpreted = renderer
        .snapshot(&interpreter.render(), 420, 190, 1.0, &input)
        .unwrap();
    assert_same_pixels(&bold, &interpreted);

    assert!(!renderer.set_system_bold_text(true));
    assert!(renderer.set_system_bold_text(false));
    let restored = renderer.snapshot(&source, 420, 190, 1.0, &input).unwrap();
    assert_same_pixels(&normal, &restored);
    let restored_caret = renderer.text.single_line_layout(entry, text).caret_x(end);
    assert!((restored_caret - normal_caret).abs() < 0.1);
}

#[test]
fn large_text_data_rows_paint_complete_glyphs_and_elide_overflow() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let full = "Pressure calibration gauge";
    let draft = "Bay 7";
    let editor = xrack_layout::TextEdit::at_end(draft);
    let font = 32.0_f32 / 1.4;
    for kind in ["TableCell", "TreeCell", "StandardListItem"] {
        let mut window = Node::new("Window", None);
        let mut row = Node::new(kind, Some("instrument"));
        row.properties.insert("width", Length(126.0).into());
        row.properties.insert("height", Length(36.0).into());
        row.properties.insert("text", full.to_owned().into());
        window.children.push(row);
        assert!(renderer.set_text_scale(2.6));
        let actual = renderer.snapshot(&window, 144, 52, 1.0, &input).unwrap();
        let bounds = actual.layout.children[0].bounds;
        assert_eq!(bounds.height, 36.0);

        let row = &mut window.children[0];
        let editable = kind != "StandardListItem";
        let width = if editable {
            xrack_layout::text_editor_viewport(row, bounds).width
        } else {
            bounds.width - 24.0
        };
        let caret = |renderer: &Renderer, row: &Node| {
            renderer
                .text
                .single_line_caret_x(row, bounds, draft, Some(&editor))
        };
        let actual_caret = editable.then(|| caret(&renderer, row));
        row.properties
            .insert("font-size", Length(f64::from(font)).into());
        assert!(renderer.set_text_scale(1.0));
        if let Some(actual) = actual_caret {
            assert!((actual - caret(&renderer, row)).abs() < 0.01);
        }
        let shortened = xrack_layout::elide_to_width(full, width, |value| {
            renderer.text.advance_node(row, value)
        });
        assert!(shortened.ends_with('…'));
        row.properties.insert("text", shortened.into_owned().into());
        let expected = renderer.snapshot(&window, 144, 52, 1.0, &input).unwrap();
        assert_same_pixels(&actual, &expected);
    }
}

#[test]
fn large_text_toggles_fit_the_label_around_the_indicator() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    let font = 40.0_f32 / 1.4;
    for kind in ["CheckBox", "RadioButton", "Switch"] {
        for direction in [
            xrack_runtime::LayoutDirection::LeftToRight,
            xrack_runtime::LayoutDirection::RightToLeft,
        ] {
            let mut window = Node::new("Window", None);
            let mut toggle = Node::new(kind, Some("toggle"));
            toggle.properties.insert(
                "layout-direction",
                xrack_runtime::Value::LayoutDirection(direction),
            );
            toggle.properties.insert("width", Length(184.0).into());
            toggle.properties.insert("height", Length(44.0).into());
            let full = "Include scheduled maintenance";
            toggle.properties.insert("text", full.to_owned().into());
            toggle.properties.insert("checked", true.into());
            window.children.push(toggle);

            assert!(renderer.set_text_scale(2.6));
            let actual = renderer.snapshot(&window, 194, 54, 1.0, &input).unwrap();
            let bounds = actual.layout.children[0].bounds;
            assert_eq!(bounds.height, 44.0);
            let toggle = &mut window.children[0];
            let label = xrack_layout::ToggleGeometry::new(toggle, bounds, direction).label;
            toggle
                .properties
                .insert("font-size", Length(f64::from(font)).into());
            assert!(renderer.set_text_scale(1.0));
            let shortened = xrack_layout::elide_to_width(full, label.width, |value| {
                renderer.text.advance_node(toggle, value)
            });
            assert!(shortened.ends_with('…'));
            toggle
                .properties
                .insert("text", shortened.into_owned().into());
            let expected = renderer.snapshot(&window, 194, 54, 1.0, &input).unwrap();
            assert_same_pixels(&actual, &expected);
        }
    }
}

#[test]
fn fixed_height_buttons_paint_complete_scaled_lines_with_an_ellipsis() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let input = Interaction::default();
    for kind in ["Button", "ToolButton", "DialogButton"] {
        let mut window = Node::new("Window", None);
        let mut button = Node::new(kind, None);
        button.properties.insert("width", Length(220.0).into());
        button.properties.insert("height", Length(44.0).into());
        button
            .properties
            .insert("text", "Review\nMore".to_owned().into());
        window.children.push(button);
        assert!(renderer.set_text_scale(2.6));
        let actual = renderer.snapshot(&window, 240, 60, 1.0, &input).unwrap();
        assert_eq!(actual.layout.children[0].bounds.height, 44.0);

        let button = &mut window.children[0];
        button.properties.insert("padding", Length(4.0).into());
        button
            .properties
            .insert("font-size", Length(f64::from(36.0_f32 / 1.4)).into());
        button
            .properties
            .insert("text", "Review…".to_owned().into());
        assert!(renderer.set_text_scale(1.0));
        let expected = renderer.snapshot(&window, 240, 60, 1.0, &input).unwrap();
        assert_same_pixels(&actual, &expected);
    }
}

#[test]
fn platform_text_scale_grows_tab_header_ink_and_leaves_room_for_close() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let mut window = Node::new("Window", None);
    let mut tabs = Node::new("TabWidget", None);
    tabs.properties.insert("width", Length(248.0).into());
    tabs.properties.insert("height", Length(134.0).into());
    tabs.properties.insert("tabs-closable", true.into());
    let mut tab = Node::new("Tab", Some("draft"));
    tab.properties.insert("title", "Draft".to_owned().into());
    tabs.children.push(tab);
    window.children.push(tabs);
    let input = Interaction::default();

    let ink = |snapshot: &Snapshot| {
        let header = snapshot
            .layout
            .at_path(&[0, 0])
            .unwrap()
            .tab_header
            .unwrap();
        let background = snapshot.pixel(200, (header.y + header.height * 0.5) as u32);
        ((header.y + 5.0) as u32..(header.y + header.height - 5.0) as u32)
            .filter(|&y| (10..132).any(|x| snapshot.pixel(x, y) != background))
            .count()
    };
    let normal = renderer.snapshot(&window, 260, 160, 1.0, &input).unwrap();
    assert!(renderer.set_text_scale(2.6));
    let scaled = renderer.snapshot(&window, 260, 160, 1.0, &input).unwrap();
    let normal_ink = ink(&normal);
    let scaled_ink = ink(&scaled);
    assert!(
        normal_ink > 0 && scaled_ink > normal_ink * 2,
        "{normal_ink} → {scaled_ink}"
    );
    let close = scaled.layout.at_path(&[0, 0]).unwrap().tab_close.unwrap();
    assert_eq!((close.width, close.height), (44.0, 44.0));
}

#[test]
fn platform_text_scale_grows_menu_rows_and_the_painted_label() {
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))
    .unwrap();
    let interpreted = interpret_example("examples/menu/menu.xr")
        .create("MenuDemo")
        .unwrap();
    let size = Size::new(620.0, 366.0);
    let mut aot = UiDriver::headless(MenuDemo::new(), size);
    let mut interpreter = UiDriver::headless(interpreted, size);
    aot.click("file-menu");
    interpreter.click("file-menu");

    let row =
        |frame: &Snapshot| aot.input().menu_panels(aot.scene(), &frame.layout)[0].row_bounds(0);
    let ink_height = |frame: &Snapshot, row: Rect| {
        let left = row.x as u32;
        let background = frame.pixel(left + 20, (row.y + row.height * 0.5) as u32);
        ((row.y + 4.0) as u32..(row.y + row.height - 4.0) as u32)
            .filter(|&y| (left + 26..left + 95).any(|x| frame.pixel(x, y) != background))
            .count()
    };
    let normal = renderer
        .snapshot(aot.scene(), 620, 366, 1.0, aot.input())
        .unwrap();
    assert!(renderer.set_text_scale(2.6));
    let enlarged = renderer
        .snapshot(aot.scene(), 620, 366, 1.0, aot.input())
        .unwrap();
    assert!(row(&enlarged).height > row(&normal).height);
    let normal_ink = ink_height(&normal, row(&normal));
    let enlarged_ink = ink_height(&enlarged, row(&enlarged));
    if let Some(dir) = std::env::var_os("TEST_UNDECLARED_OUTPUTS_DIR") {
        let dir = std::path::PathBuf::from(dir);
        normal.save_png(dir.join("menu-standard-text.png")).unwrap();
        enlarged.save_png(dir.join("menu-large-text.png")).unwrap();
    }
    assert!(
        enlarged_ink > normal_ink * 2,
        "menu ink {normal_ink} → {enlarged_ink}, rows {:?} → {:?}",
        row(&normal),
        row(&enlarged)
    );
    let interpreted = renderer
        .snapshot(interpreter.scene(), 620, 366, 1.0, interpreter.input())
        .unwrap();
    assert_same_pixels(&enlarged, &interpreted);
}
