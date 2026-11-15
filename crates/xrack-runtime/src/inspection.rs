use crate::Node;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

/// UI-thread state shared by a live-reload component and its native host.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct WindowInspector(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    attached: bool,
    picking: bool,
    selected: Option<String>,
    revision: u64,
    pick_revision: u64,
    scene: Weak<Node>,
    targets: Vec<(String, Vec<usize>)>,
}

impl WindowInspector {
    pub fn attach(&self, attached: bool) {
        let mut state = self.0.borrow_mut();
        state.attached = attached;
        state.picking = false;
        state.selected = None;
        state.scene = Weak::new();
        state.targets.clear();
        state.revision += 1;
    }

    pub fn attached(&self) -> bool {
        self.0.borrow().attached
    }

    pub fn picking(&self) -> bool {
        self.0.borrow().picking
    }

    pub fn set_picking(&self, picking: bool) {
        let mut state = self.0.borrow_mut();
        if state.attached && state.picking != picking {
            state.picking = picking;
            state.revision += 1;
        }
    }

    pub fn revision(&self) -> u64 {
        self.0.borrow().revision
    }

    pub fn selection(&self) -> (Option<String>, u64) {
        let state = self.0.borrow();
        (state.selected.clone(), state.pick_revision)
    }

    pub fn select(&self, key: &str) {
        let mut state = self.0.borrow_mut();
        if state.attached
            && state.selected.as_deref() != Some(key)
            && state.targets.iter().any(|(target, _)| target == key)
        {
            state.selected = Some(key.to_owned());
            state.revision += 1;
        }
    }

    pub fn matches_scene(&self, scene: &Rc<Node>) -> bool {
        Weak::ptr_eq(&self.0.borrow().scene, &Rc::downgrade(scene))
    }

    pub fn set_targets(&self, scene: &Rc<Node>, targets: Vec<(String, Vec<usize>)>) {
        let mut state = self.0.borrow_mut();
        if state.attached {
            state.scene = Rc::downgrade(scene);
            state.targets = targets;
        }
    }

    pub fn selected_path(&self) -> Option<Vec<usize>> {
        let state = self.0.borrow();
        let selected = state.selected.as_ref()?;
        state
            .targets
            .iter()
            .find_map(|(key, path)| (key == selected).then(|| path.clone()))
    }

    pub fn pick(&self, path: Option<&[usize]>) {
        let mut state = self.0.borrow_mut();
        if !state.attached {
            return;
        }
        state.picking = false;
        if let Some(key) = path.and_then(|path| {
            state
                .targets
                .iter()
                .filter(|(_, target)| path.starts_with(target))
                .max_by_key(|(_, target)| target.len())
                .map(|(key, _)| key.clone())
        }) {
            state.selected = Some(key);
            state.pick_revision += 1;
        }
        state.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_tracks_scene_replacement_and_picks_the_closest_target() {
        let inspector = WindowInspector::default();
        let initial = Rc::new(Node::new("Window", None));
        inspector.attach(true);
        inspector.set_targets(
            &initial,
            vec![("root".into(), vec![]), ("list".into(), vec![0])],
        );
        inspector.select("list");
        inspector.select("unknown");
        assert_eq!(inspector.selected_path(), Some(vec![0]));
        let next = Rc::new(Node::new("Window", None));
        assert!(!inspector.matches_scene(&next));
        inspector.set_targets(
            &next,
            vec![("root".into(), vec![]), ("list".into(), vec![1])],
        );
        assert_eq!(inspector.selected_path(), Some(vec![1]));
        inspector.set_picking(true);
        inspector.pick(Some(&[1, 3, 2]));
        assert_eq!(inspector.selection(), (Some("list".into()), 1));
        assert!(!inspector.picking());
        inspector.attach(false);
        inspector.pick(Some(&[1]));
        assert!(!inspector.attached());
        assert!(inspector.selected_path().is_none());
    }
}
