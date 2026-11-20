use crate::{
    Node, ui_trace,
    update_profile::{self, UpdateSample},
};

const MAX_ACTIONS: usize = 48;
const MAX_SAMPLES: usize = 24;
const MAX_TEXT_BYTES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedAction {
    Click { id: String },
    Fill { id: String, text: String },
    Reload { generation: u64 },
}
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::{Rc, Weak},
    sync::mpsc::Sender,
    time::Instant,
};

#[doc(hidden)]
pub struct InspectionInput {
    pub action: RecordedAction,
    pub generation: u64,
    pub deadline: Instant,
    pub reply: Sender<Result<(), String>>,
}

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
    record: bool,
    record_text: bool,
    actions: Vec<RecordedAction>,
    truncated: bool,
    profile: bool,
    control: bool,
    input: Option<InspectionInput>,
    samples: VecDeque<(u64, u64, UpdateSample)>,
    diagnostics: Option<ui_trace::Buffer>,
    updated_requests: Vec<u64>,
    generation: u64,
    next_sample: u64,
}

impl WindowInspector {
    pub fn configure_observation(
        &self,
        record: bool,
        record_text: bool,
        profile: bool,
        control: bool,
    ) {
        let mut state = self.0.borrow_mut();
        state.record = record;
        state.record_text = record && record_text;
        state.profile = profile;
        state.control = control;
    }

    pub fn observing(&self) -> bool {
        let state = self.0.borrow();
        state.record || state.profile || state.control
    }

    pub fn profiling(&self) -> bool {
        self.0.borrow().profile
    }

    pub fn record_click(&self, scene: &Node, path: &[usize]) {
        if !self.0.borrow().record {
            return;
        }
        if let Some(id) = stable_id(scene, path, &["Button"]) {
            self.push_action(RecordedAction::Click { id });
        }
    }

    pub fn record_fill(&self, scene: &Node, path: &[usize]) {
        if !self.0.borrow().record_text {
            return;
        }
        let Some(node) = scene.at_path(path).filter(|node| !node.is_password()) else {
            return;
        };
        if let Some(id) = stable_id(scene, path, &["SearchField", "LineEdit", "TextEdit"])
            && let Some(text) = node.text("text")
        {
            if text.len() > MAX_TEXT_BYTES {
                self.0.borrow_mut().truncated = true;
            } else {
                self.push_action(RecordedAction::Fill {
                    id,
                    text: text.to_owned(),
                });
            }
        }
    }

    fn push_action(&self, action: RecordedAction) {
        let mut state = self.0.borrow_mut();
        if state.actions.len() < MAX_ACTIONS && !state.truncated {
            state.actions.push(action);
        } else {
            state.truncated = true;
        }
    }

    pub fn recording(&self) -> Option<(Vec<RecordedAction>, bool, bool)> {
        let state = self.0.borrow();
        state
            .record
            .then(|| (state.actions.clone(), state.truncated, state.record_text))
    }

    pub fn advance_generation(&self) {
        let mut state = self.0.borrow_mut();
        state.generation += 1;
        let generation = state.generation;
        if state.record {
            drop(state);
            self.push_action(RecordedAction::Reload { generation });
        }
    }

    pub fn push_sample(&self, sample: UpdateSample) {
        let mut state = self.0.borrow_mut();
        if state.samples.len() == MAX_SAMPLES {
            state.samples.pop_front();
        }
        state.next_sample += 1;
        let id = state.next_sample;
        let generation = state.generation;
        state.samples.push_back((id, generation, sample));
    }

    pub fn set_trace_buffer(&self, buffer: ui_trace::Buffer) {
        self.0.borrow_mut().diagnostics = Some(buffer);
    }

    pub fn trace_events(&self) -> Option<Vec<ui_trace::Fields>> {
        self.0
            .borrow()
            .diagnostics
            .as_ref()
            .map(ui_trace::Buffer::snapshot)
    }

    /// Associate a result write with the next batched native window update.
    pub fn trace_update_from(&self, request: u64) {
        let mut state = self.0.borrow_mut();
        if state.diagnostics.is_some()
            && state.updated_requests.len() < MAX_ACTIONS
            && !state.updated_requests.contains(&request)
        {
            state.updated_requests.push(request);
        }
    }

    pub fn measure_update(&self, update: impl FnOnce()) {
        let (generation, requests) = {
            let mut state = self.0.borrow_mut();
            (
                state.generation,
                std::mem::take(&mut state.updated_requests),
            )
        };
        let span = tracing::debug_span!(
            target: ui_trace::TARGET,
            "window_update",
            generation,
            // Several result writes can be coalesced in one native frame.
            requests = ?requests
        );
        if let Some(sample) = span.in_scope(|| update_profile::measure(update)) {
            self.push_sample(sample);
        }
    }

    pub fn samples(&self) -> Option<Vec<(u64, u64, UpdateSample)>> {
        let state = self.0.borrow();
        state
            .profile
            .then(|| state.samples.iter().cloned().collect())
    }

    pub fn queue_input(&self, input: InspectionInput) {
        let mut state = self.0.borrow_mut();
        let error = if !state.control {
            "Launch the live application with XRACK_CONTROL_UI=1"
        } else if input.generation != state.generation {
            "Source generation changed; refresh the target first"
        } else if state.input.is_some() {
            "An inspection action is already queued"
        } else {
            state.input = Some(input);
            return;
        };
        let _ = input.reply.send(Err(error.into()));
    }

    pub fn take_input(&self) -> Option<InspectionInput> {
        let mut state = self.0.borrow_mut();
        let input = state.input.take()?;
        if input.generation != state.generation || input.deadline <= Instant::now() {
            let _ = input.reply.send(Err(
                "The queued action has expired or the source has changed".into(),
            ));
            return None;
        }
        Some(input)
    }

    pub fn input_path(&self, scene: &Node, id: &str) -> Option<Vec<usize>> {
        named_path(scene, id)
    }

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

fn stable_id(scene: &Node, path: &[usize], kinds: &[&str]) -> Option<String> {
    let node = scene.at_path(path)?;
    let id = node.element_id().filter(|_| kinds.contains(&node.kind))?;
    (named_path(scene, id).as_deref() == Some(path)).then(|| id.to_owned())
}

fn named_path(scene: &Node, id: &str) -> Option<Vec<usize>> {
    fn find(node: &Node, id: &str, path: &mut Vec<usize>, found: &mut Option<Vec<usize>>) -> bool {
        if node.element_id() == Some(id) && found.replace(path.clone()).is_some() {
            return false;
        }
        for (index, child) in node.materialized_children() {
            path.push(index);
            if !find(child, id, path, found) {
                return false;
            }
            path.pop();
        }
        true
    }
    let mut found = None;
    if !find(scene, id, &mut Vec::new(), &mut found) {
        return None;
    }
    found.filter(|path| scene.row_key_at(path).is_none())
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
