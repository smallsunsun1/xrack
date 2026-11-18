use super::*;
use xrack_runtime::{ControlAction, StackNavigation, StackState};

fn stack(id: &'static str, child: Node) -> Node {
    let state = StackState::default();
    let pages = ["home", "detail"];
    assert!(state.navigate(
        &pages,
        || "home".into(),
        None,
        StackNavigation::Push("detail"),
        |_| {}
    ));
    let mut view = node("StackView", id);
    view.stack = Some(state.snapshot(&pages, || "home".into()));
    length(&mut view, "width", 220.0);
    length(&mut view, "height", 130.0);
    let mut detail = node("StackPage", "detail");
    detail.children.push(child);
    view.children.extend([node("StackPage", "home"), detail]);
    view
}

fn frame(scene: &Node) -> LayoutNode {
    layout(scene, Size::new(640.0, 400.0), &mut Text)
}

#[test]
fn outside_focus_finds_a_single_stack_and_keeps_its_revision() {
    let mut scene = node("Window", "root");
    let mut view = stack("route", button("detail-action"));
    length(&mut view, "y", 40.0);
    let revision = view.stack.as_ref().unwrap().revision();
    scene.children.extend([button("toolbar"), view]);
    let frame = frame(&scene);
    let mut input = Interaction::default();
    let target = input.accessibility_target(&scene, &frame, &[0]).unwrap();
    input.focus_target(&scene, &frame, target);
    assert!(input.stack_back(&scene, &frame).is_none());
    let action = input.stack_back_in_window(&scene, &frame).unwrap();
    assert_eq!(action.path, [1]);
    assert_eq!(action.action, ControlAction::StackBack { revision });
}

#[test]
fn sibling_stacks_require_focus_instead_of_choosing_by_declaration_order() {
    let mut scene = node("Window", "root");
    let first = stack("first", button("first-action"));
    let mut second = stack("second", button("second-action"));
    length(&mut second, "x", 280.0);
    scene.children.extend([first, second]);
    let frame = frame(&scene);
    let mut input = Interaction::default();
    assert!(input.stack_back_in_window(&scene, &frame).is_none());
    let target = input
        .accessibility_target(&scene, &frame, &[1, 1, 0])
        .unwrap();
    input.focus_target(&scene, &frame, target);
    assert_eq!(
        input.stack_back_in_window(&scene, &frame).unwrap().path,
        [1]
    );
}

#[test]
fn nested_stacks_choose_the_inner_active_page() {
    let inner = stack("inner", button("inner-action"));
    let outer = stack("outer", inner);
    let mut scene = node("Window", "root");
    scene.children.push(outer);
    let frame = frame(&scene);
    let input = Interaction::default();
    assert_eq!(
        input.stack_back_in_window(&scene, &frame).unwrap().path,
        [0, 1, 0]
    );
}

#[test]
fn hidden_disabled_and_offscreen_stacks_do_not_capture_window_back() {
    let mut hidden = stack("hidden", button("hidden-action"));
    boolean(&mut hidden, "visible", false);
    let mut disabled = stack("disabled", button("disabled-action"));
    boolean(&mut disabled, "enabled", false);
    let mut offscreen = stack("offscreen", button("offscreen-action"));
    length(&mut offscreen, "x", 900.0);
    for excluded in [hidden, disabled, offscreen] {
        let mut scene = node("Window", "root");
        scene.children.push(excluded);
        let frame = frame(&scene);
        assert!(
            Interaction::default()
                .stack_back_in_window(&scene, &frame)
                .is_none()
        );
    }
}

#[test]
fn a_stack_inside_an_inactive_page_does_not_hide_the_outer_stack() {
    let inner = stack("inner", button("inactive-action"));
    let mut outer = stack("outer", button("active-action"));
    outer.children[0].children.push(inner);
    let mut scene = node("Window", "root");
    scene.children.push(outer);
    let frame = frame(&scene);
    assert_eq!(
        Interaction::default()
            .stack_back_in_window(&scene, &frame)
            .unwrap()
            .path,
        [0]
    );
}

#[test]
fn an_open_dialog_prevents_back_from_reaching_background_stacks() {
    let mut scene = node("Window", "root");
    scene
        .children
        .push(stack("background", button("background-action")));
    let mut dialog = node("Dialog", "dialog");
    boolean(&mut dialog, "open", true);
    length(&mut dialog, "width", 300.0);
    length(&mut dialog, "height", 200.0);
    dialog.children.push(button("dialog-action"));
    scene.children.push(dialog);
    let frame = frame(&scene);
    let mut input = Interaction::default();
    input.reconcile(&scene, &frame);
    assert!(input.stack_back_in_window(&scene, &frame).is_none());
}
