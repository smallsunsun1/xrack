//! A named-target driver for native UI scenarios. Pointer and focus
//! actions use the same Interaction state machine as the window backend.
use std::{rc::Rc, time::Instant};
use xrack_layout::{
    ChoiceKey, EditAction, EditSource, Interaction, LayoutNode, MenuKey, MenuPanel, MenuShortcut,
    NavigationKey, Point, Size, TextMeasurer, prepare_scene_with_input,
};
use xrack_runtime::{Component, Node};

pub struct UiDriver<C, T> {
    component: C,
    text: T,
    input: Interaction,
    scene: Rc<Node>,
    layout: LayoutNode,
    viewport: Size,
}

impl<C: Component, T: TextMeasurer> UiDriver<C, T> {
    pub fn new(component: C, mut text: T, viewport: Size) -> Self {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_scene_with_input(
            &component,
            component.render_shared(),
            viewport,
            &mut text,
            &input,
        );
        input.reconcile(&scene, &layout);
        Self {
            component,
            text,
            input,
            scene,
            layout,
            viewport,
        }
    }

    fn refresh(&mut self) {
        (self.scene, self.layout) = prepare_scene_with_input(
            &self.component,
            self.component.render_shared(),
            self.viewport,
            &mut self.text,
            &self.input,
        );
        self.input.reconcile(&self.scene, &self.layout);
    }

    pub fn scene(&self) -> &Node {
        &self.scene
    }
    pub fn component(&self) -> &C {
        &self.component
    }
    pub fn input(&self) -> &Interaction {
        &self.input
    }

    fn path(&self, id: &str) -> Vec<usize> {
        fn locate(node: &Node, id: &str, path: &mut Vec<usize>, found: &mut Option<Vec<usize>>) {
            if node.id == Some(id) {
                assert!(
                    found.replace(path.clone()).is_none(),
                    "ambiguous UI target {id:?}"
                );
            }
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                locate(child, id, path, found);
                path.pop();
            }
        }
        let mut found = None;
        locate(&self.scene, id, &mut Vec::new(), &mut found);
        found.unwrap_or_else(|| panic!("unknown UI target {id:?}"))
    }

    pub fn node(&self, id: &str) -> &Node {
        self.scene.at_path(&self.path(id)).unwrap()
    }

    pub fn focused(&self) -> Option<&'static str> {
        self.input.focused.as_ref().and_then(|target| target.id)
    }

    pub fn can_hit(&mut self, id: &str) -> bool {
        let path = self.path(id);
        self.move_to(&path);
        self.input
            .hovered
            .as_ref()
            .is_some_and(|target| target.path == path)
    }

    fn move_to(&mut self, path: &[usize]) {
        let item = self.layout.at_path(path).expect("target has layout");
        let hit = item.clip.intersect(item.bounds);
        assert!(
            hit.width > 0.0 && hit.height > 0.0,
            "target is outside the viewport"
        );
        self.input.pointer_move(
            &self.scene,
            &self.layout,
            Point {
                x: hit.x + hit.width * 0.5,
                y: hit.y + hit.height * 0.5,
            },
        );
    }

    pub fn click(&mut self, id: &str) {
        let path = self.path(id);
        self.move_to(&path);
        if self.input.menu_press(&self.scene, &self.layout) {
            self.release_menu();
            return;
        }
        self.input.pointer_press();
        let target = self
            .input
            .pointer_release_target()
            .unwrap_or_else(|| panic!("click did not reach {id:?}"));
        assert_eq!(
            target.path, path,
            "click hit a different control than {id:?}"
        );
        let action = self
            .input
            .dialog_button(&self.scene, &self.layout, &target)
            .or_else(|| {
                self.input
                    .select_group_option(&self.scene, &self.layout, &target)
            });
        if let Some(action) = action {
            assert!(
                self.component
                    .control_at(&self.scene, &action.path, action.action)
            );
        } else if self
            .input
            .open_menu_bar(&self.scene, &self.layout, target.clone())
        {
            // Opening the header changes only the interaction state.
        } else if self
            .input
            .open_choice_at_pointer(&self.scene, &self.layout, target.clone())
        {
            // Choice popups are input state and do not mutate the component on open.
        } else {
            self.component
                .dispatch_at(&self.scene, &target.path, "clicked");
        }
        self.refresh();
    }

    pub fn tab(&mut self) {
        if self.input.focus_next(&self.scene, &self.layout, false) {
            self.refresh();
        }
    }

    pub fn group_key(&mut self, key: NavigationKey) -> bool {
        let Some(action) = self.input.group_key(&self.scene, &self.layout, key) else {
            return false;
        };
        let changed = self
            .component
            .control_at(&self.scene, &action.path, action.action);
        self.refresh();
        changed
    }

    pub fn back(&mut self) -> bool {
        let Some(action) = self.input.stack_back(&self.scene, &self.layout) else {
            return false;
        };
        let moved = self
            .component
            .control_at(&self.scene, &action.path, action.action);
        self.refresh();
        moved
    }

    pub fn menus(&self) -> Vec<MenuPanel<'_>> {
        self.input.menu_panels(&self.scene, &self.layout)
    }

    pub fn context_menu(&mut self, id: &str) -> bool {
        let path = self.path(id);
        self.move_to(&path);
        self.input
            .open_context_menu(&self.scene, &self.layout, false)
    }

    pub fn keyboard_context_menu(&mut self) -> bool {
        self.input
            .open_context_menu(&self.scene, &self.layout, true)
    }

    pub fn menu_key(&mut self, key: MenuKey) {
        if let Some(action) = self.input.menu_key(&self.scene, &self.layout, key) {
            assert!(
                self.component
                    .control_at(&self.scene, &action.path, action.action)
            );
        }
        self.refresh();
    }

    pub fn menu_shortcut(&mut self, key: MenuShortcut) -> bool {
        let Some(action) = self.input.menu_shortcut(&self.scene, &self.layout, key) else {
            return false;
        };
        self.input.close_menu(&self.scene, &self.layout);
        assert!(
            self.component
                .control_at(&self.scene, &action.path, action.action)
        );
        self.refresh();
        true
    }

    pub fn click_menu_row(&mut self, label: &str) -> bool {
        let rect = {
            let panels = self.menus();
            let panel = panels.last().expect("menu panel is open");
            let slot = panel
                .rows
                .iter()
                .position(|row| row.label == label)
                .expect("row is visible");
            panel.row_bounds(slot)
        };
        self.input.pointer_move(
            &self.scene,
            &self.layout,
            Point {
                x: rect.x + rect.width * 0.5,
                y: rect.y + rect.height * 0.5,
            },
        );
        assert!(self.input.menu_press(&self.scene, &self.layout));
        self.release_menu()
    }

    fn release_menu(&mut self) -> bool {
        let activated = self
            .input
            .menu_release(&self.scene, &self.layout)
            .is_some_and(|action| {
                assert!(
                    self.component
                        .control_at(&self.scene, &action.path, action.action)
                );
                true
            });
        self.refresh();
        activated
    }

    pub fn choice_key(&mut self, key: ChoiceKey) {
        assert!(self.input.choice_open(), "choice popup is closed");
        if let Some(action) = self.input.choice_key(&self.scene, &self.layout, key) {
            assert!(
                self.component
                    .control_at(&self.scene, &action.path, action.action)
            );
        }
        self.refresh();
    }

    pub fn escape(&mut self) {
        let action = self
            .input
            .dialog_escape(&self.scene, &self.layout)
            .expect("dismissible dialog");
        assert!(
            self.component
                .control_at(&self.scene, &action.path, action.action)
        );
        self.refresh();
    }

    pub fn fill_focused(&mut self, text: &str) {
        let now = Instant::now();
        self.input.edit_text(
            &mut self.component,
            &self.scene,
            EditAction::SelectAll,
            EditSource::Keyboard,
            now,
        );
        assert!(self.input.edit_text(
            &mut self.component,
            &self.scene,
            EditAction::Insert(text),
            EditSource::Atomic,
            now
        ));
        self.refresh();
    }
}
