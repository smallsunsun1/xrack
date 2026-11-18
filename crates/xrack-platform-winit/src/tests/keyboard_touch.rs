use super::*;
use xrack_layout::{TextMeasurer, layout_with_input};
use xrack_runtime::Length;

#[derive(Default)]
struct View {
    clicks: usize,
}

impl Component for View {
    fn render(&self) -> Node {
        let mut root = Node::new("Window", None);
        let mut button = Node::new("Button", Some("button"));
        button.properties.insert("width", Length(120.0).into());
        button.properties.insert("height", Length(72.0).into());
        root.children.push(button);
        root
    }

    fn dispatch(&mut self, id: &str, event: &str) -> bool {
        if id == "button" && event == "clicked" {
            self.clicks += 1;
            true
        } else {
            false
        }
    }
}

struct Measure;

impl TextMeasurer for Measure {
    fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
        Size::new(12.0, 16.0)
    }
}

#[test]
fn key_releases_do_not_cancel_an_in_progress_touch() {
    for logical_key in [Key::Named(NamedKey::Control), Key::Character("v".into())] {
        let mut app = Application::new(View::default(), LocalExecutor::new(|| {}));
        app.layout = Some(Rc::new(layout_with_input(
            &app.scene,
            Size::new(180.0, 120.0),
            &mut Measure,
            &app.input,
        )));
        let device = DeviceId::dummy();
        let point = Point { x: 30.0, y: 40.0 };
        let now = Instant::now();
        app.update_touch(device, 1, TouchPhase::Started, point, now);
        // Android may deliver the preceding shortcut's key-up after this down.
        app.default_key_input(
            &KeyEvent {
                unmodified_key: logical_key.clone(),
                logical_key,
                text: None,
                state: ElementState::Released,
                repeat: false,
            },
            false,
        );
        app.update_touch(
            device,
            1,
            TouchPhase::Ended,
            point,
            now + Duration::from_millis(20),
        );
        assert_eq!(app.component.clicks, 1);
    }
}
