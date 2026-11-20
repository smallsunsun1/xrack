use crate::*;
use xrack_layout::{TextMeasurer, layout};
use xrack_runtime::{DialogRole, Length, Value};

struct Dialogs {
    root: Node,
    rejected: Vec<String>,
}

impl Component for Dialogs {
    fn render(&self) -> Node {
        self.root.clone()
    }

    fn dispatch(&mut self, _: &str, _: &str) -> bool {
        false
    }

    fn control_at(&mut self, scene: &Node, path: &[usize], action: ControlAction) -> bool {
        match action {
            ControlAction::Dialog(DialogRole::Reject) => self
                .rejected
                .push(scene.at_path(path).unwrap().id.unwrap().to_owned()),
            ControlAction::Popup(false) => {}
            _ => return false,
        }
        let mut node = &mut self.root;
        for &slot in path {
            node = &mut node.children[slot];
        }
        node.properties.insert("open", false.into());
        true
    }
}

struct Measure;

impl TextMeasurer for Measure {
    fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
        Size::new(90.0, 20.0)
    }
}

fn prepare(app: &mut Application<Dialogs>) {
    app.scene = app.component.render_shared();
    let frame = Rc::new(layout(&app.scene, Size::new(400.0, 700.0), &mut Measure));
    app.input.reconcile(&app.scene, &frame);
    app.layout = Some(frame);
}

fn back(app: &mut Application<Dialogs>, state: ElementState, repeat: bool) {
    assert!(app.android_back_key(&KeyEvent {
        logical_key: Key::Named(NamedKey::BrowserBack),
        unmodified_key: Key::Named(NamedKey::BrowserBack),
        text: None,
        state,
        repeat,
    }));
    prepare(app);
}

fn dialog(id: &'static str, child: Node) -> Node {
    let mut dialog = Node::new("Dialog", Some(id));
    dialog.properties.insert("open", true.into());
    dialog.properties.insert("width", Length(320.0).into());
    dialog.properties.insert("height", Length(250.0).into());
    dialog.children.push(child);
    dialog
}

#[test]
fn back_closes_the_choice_then_one_dialog_per_physical_gesture() {
    let mut choice = Node::new("ComboBox", Some("choice"));
    choice.properties.insert("width", Length(180.0).into());
    choice.properties.insert("height", Length(44.0).into());
    choice.properties.insert(
        "model",
        Value::StringList(vec!["First".into(), "Second".into()]),
    );
    let mut root = Node::new("Window", None);
    root.children.push(dialog("outer", dialog("inner", choice)));
    let mut app = Application::new(
        Dialogs {
            root,
            rejected: Vec::new(),
        },
        LocalExecutor::new(|| {}),
    );
    prepare(&mut app);
    let frame = app.layout.as_ref().unwrap();
    let target = app
        .input
        .accessibility_target(&app.scene, frame, &[0, 0, 0])
        .unwrap();
    assert!(app.input.open_choice_at_pointer(&app.scene, frame, target));
    assert!(app.input.choice_open());

    back(&mut app, ElementState::Pressed, false);
    assert!(!app.input.choice_open());
    assert!(app.component.rejected.is_empty());
    for state in [ElementState::Pressed, ElementState::Released] {
        back(&mut app, state, true);
        assert!(app.component.rejected.is_empty());
    }

    back(&mut app, ElementState::Pressed, false);
    assert_eq!(app.component.rejected, ["inner"]);
    back(&mut app, ElementState::Released, false);
    back(&mut app, ElementState::Pressed, true);
    assert_eq!(app.component.rejected, ["inner"]);

    app.component.root.children[0]
        .properties
        .insert("close-on-escape", false.into());
    prepare(&mut app);
    back(&mut app, ElementState::Pressed, false);
    assert_eq!(app.component.rejected, ["inner"]);
    app.component.root.children[0]
        .properties
        .insert("close-on-escape", true.into());
    prepare(&mut app);
    back(&mut app, ElementState::Pressed, false);
    assert_eq!(app.component.rejected, ["inner", "outer"]);
}

#[test]
fn a_nonmodal_popup_can_block_back_without_falling_through() {
    let mut popup = Node::new("PopupWindow", Some("popup"));
    for (name, value) in [("open", true), ("modal", false), ("close-on-escape", false)] {
        popup.properties.insert(name, value.into());
    }
    popup.properties.insert("width", Length(200.0).into());
    popup.properties.insert("height", Length(160.0).into());
    let mut root = Node::new("Window", None);
    root.children.push(popup);
    let mut app = Application::new(
        Dialogs {
            root,
            rejected: Vec::new(),
        },
        LocalExecutor::new(|| {}),
    );
    prepare(&mut app);
    assert!(app.layout.as_ref().unwrap().dialog_paths().is_empty());
    assert_eq!(app.layout.as_ref().unwrap().overlay_paths(), [vec![0]]);
    back(&mut app, ElementState::Pressed, false);
    assert!(app.component.root.children[0].boolean("open", false));
    app.component.root.children[0]
        .properties
        .insert("close-on-escape", true.into());
    prepare(&mut app);
    back(&mut app, ElementState::Pressed, false);
    assert!(!app.component.root.children[0].boolean("open", true));
    assert!(app.component.rejected.is_empty());
}
