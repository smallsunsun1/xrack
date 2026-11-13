use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Duration,
};
use xrack_renderer_femtovg::{
    Renderer,
    preview::{ClipboardRequest, PreviewScene},
};
use xrack_runtime::{
    Component, NativeDialogPresentation, NativeDialogResult, SecondaryWindowRegistration,
};

#[derive(Debug, Serialize)]
pub(crate) struct Description {
    id: Option<u64>,
    title: String,
}

struct Child {
    registration: SecondaryWindowRegistration,
    scene: PreviewScene,
}

pub(crate) struct Windows {
    root: PreviewScene,
    children: BTreeMap<u64, Child>,
    selected: Option<u64>,
    dialog: Option<(Option<u64>, u64)>,
    clipboard: Option<(Option<u64>, u64)>,
}

impl Windows {
    pub(crate) fn new(component: Box<dyn Component>) -> Self {
        let root = PreviewScene::new(component);
        Self {
            root,
            children: BTreeMap::new(),
            selected: None,
            dialog: None,
            clipboard: None,
        }
    }

    pub(crate) fn closed(&self) -> bool {
        self.root.window_closed()
    }

    pub(crate) fn selection(&self) -> Option<u64> {
        self.selected
    }

    pub(crate) fn select(&mut self, id: Option<u64>) -> bool {
        if id == self.selected || id.is_some_and(|id| !self.children.contains_key(&id)) {
            return false;
        }
        self.selected_mut().set_window_active(false);
        self.selected = id;
        self.selected_mut().set_window_active(true);
        true
    }

    fn window(&mut self, id: Option<u64>) -> Option<&mut PreviewScene> {
        match id {
            Some(id) => self.children.get_mut(&id).map(|child| &mut child.scene),
            None => Some(&mut self.root),
        }
    }

    pub(crate) fn selected_mut(&mut self) -> &mut PreviewScene {
        self.window(self.selected)
            .expect("live preview window selection")
    }

    pub(crate) fn selected(&self) -> &PreviewScene {
        self.selected
            .map_or(&self.root, |id| &self.children[&id].scene)
    }

    pub(crate) fn render(
        &mut self,
        renderer: &mut Renderer,
        scale: f32,
    ) -> Result<Vec<u8>, String> {
        // Closing a child can invalidate its parent's scene and registration list.
        for _ in 0..8 {
            if !self.advance_windows(renderer) {
                break;
            }
        }
        if self
            .selected
            .is_some_and(|id| !self.children.contains_key(&id))
        {
            self.selected = None;
            if !self.closed() {
                self.root.set_window_active(true);
                self.root.advance(renderer);
            }
        }
        if self.closed() {
            return Ok(Vec::new());
        }
        Ok(self
            .selected_mut()
            .draw_prepared_scaled(renderer, scale)?
            .to_vec())
    }

    fn advance_windows(&mut self, renderer: &mut Renderer) -> bool {
        let mut live = BTreeSet::new();
        let mut next = VecDeque::from([None]);
        let mut closed = false;
        while let Some(id) = next.pop_front() {
            let scene = self.window(id).expect("mounted preview window");
            scene.advance(renderer);
            if scene.window_closed() {
                let Some(id) = id else { break };
                live.remove(&id);
                self.children[&id].registration.closed();
                closed = true;
                continue;
            }
            if let Some(windows) = scene.secondary_windows() {
                for registration in windows.iter() {
                    let child_id = registration.id();
                    if !live.insert(child_id) {
                        continue;
                    }
                    let child = self.children.entry(child_id);
                    if let std::collections::btree_map::Entry::Vacant(entry) = child {
                        let Some(component) = registration.mount() else {
                            live.remove(&child_id);
                            continue;
                        };
                        let mut scene = PreviewScene::new(component);
                        scene.set_window_active(false);
                        entry.insert(Child {
                            registration: registration.clone(),
                            scene,
                        });
                    }
                    next.push_back(Some(child_id));
                }
            }
        }
        self.children.retain(|id, _| live.contains(id));
        self.dialog = self
            .dialog
            .filter(|(id, _)| id.is_none_or(|id| live.contains(&id)));
        self.clipboard = self
            .clipboard
            .filter(|(id, _)| id.is_none_or(|id| live.contains(&id)));
        closed
    }

    pub(crate) fn descriptions(&self) -> Vec<Description> {
        std::iter::once((None, &self.root))
            .chain(
                self.children
                    .iter()
                    .map(|(&id, child)| (Some(id), &child.scene)),
            )
            .map(|(id, scene)| Description {
                id,
                title: scene
                    .title()
                    .unwrap_or(if id.is_some() {
                        "Auxiliary window"
                    } else {
                        "Main window"
                    })
                    .into(),
            })
            .collect()
    }

    pub(crate) fn next_frame_delay(&self) -> Option<Duration> {
        std::iter::once(&self.root)
            .chain(self.children.values().map(|child| &child.scene))
            .filter_map(PreviewScene::next_frame_delay)
            .min()
    }

    pub(crate) fn begin_native_dialog(&mut self) -> Option<NativeDialogPresentation> {
        if self.dialog.is_some() || self.closed() {
            return None;
        }
        let (owner, request) = self
            .root
            .begin_native_dialog()
            .map(|request| (None, request))
            .or_else(|| {
                self.children.iter_mut().find_map(|(&id, child)| {
                    child
                        .scene
                        .begin_native_dialog()
                        .map(|request| (Some(id), request))
                })
            })?;
        self.dialog = Some((owner, request.id));
        Some(request)
    }

    pub(crate) fn finish_native_dialog(&mut self, id: u64, result: NativeDialogResult) -> bool {
        let Some((owner, _)) = self.dialog.filter(|(_, request)| *request == id) else {
            return false;
        };
        let Some(scene) = self.window(owner).filter(|scene| !scene.window_closed()) else {
            self.dialog = None;
            return false;
        };
        let completed = scene.finish_native_dialog(id, result);
        if completed {
            self.dialog = None;
        }
        completed
    }

    pub(crate) fn take_clipboard_request(&mut self) -> Option<ClipboardRequest> {
        let request = self.selected_mut().take_clipboard_request()?;
        self.clipboard = Some((self.selected, request.id));
        Some(request)
    }

    pub(crate) fn finish_clipboard(&mut self, id: u64, success: bool, text: &str) -> bool {
        let Some((owner, _)) = self.clipboard.take_if(|(_, request)| *request == id) else {
            return false;
        };
        self.window(owner)
            .is_some_and(|scene| scene.finish_clipboard(id, success, text))
    }
}
