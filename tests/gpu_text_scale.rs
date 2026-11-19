use super::{assert_same_pixels, interpret_example};
use bundled_ui::BundledFontsDemo;
use xrack_layout::{Interaction, Size, TextPosition};
use xrack_renderer_femtovg::{Renderer, TextSystem};
use xrack_runtime::{Component, Length, Node};

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
    assert!(after.children[0].children[0].bounds.y > before.children[0].children[0].bounds.y);
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
