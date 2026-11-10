use crate::{
    LayoutNode, Point, Target,
    drag::{Context, DragUpdate, send},
};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use xrack_runtime::{DragAction, DragEvent, Length, Node, Value};

/// Native file drags carry a whole pasteboard batch and a window-local position.
#[derive(Debug)]
pub enum FileDragInput {
    Moved {
        sequence: u64,
        point: Point,
        paths: Arc<[String]>,
    },
    Dropped {
        sequence: u64,
        point: Point,
        paths: Arc<[String]>,
        target: Target,
    },
    Left {
        sequence: u64,
    },
}

/// The interaction gates needed for a synchronous native drag acceptance query.
#[derive(Default)]
pub struct FileDropScope {
    pub(crate) modal: Option<Vec<usize>>,
    pub(crate) popups: Vec<Vec<usize>>,
    pub(crate) transitions: BTreeSet<Vec<usize>>,
    pub(crate) blocked: bool,
}

impl FileDropScope {
    pub fn target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        paths: &[String],
    ) -> Option<Target> {
        self.hit(scene, layout, point, paths).map(|hit| hit.target)
    }

    fn hit(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        paths: &[String],
    ) -> Option<FileHit> {
        if paths.is_empty() || paths.iter().any(|path| !Path::new(path).is_absolute()) {
            return None;
        }
        let scope = self
            .popups
            .iter()
            .find(|path| {
                layout
                    .at_path(path)
                    .is_some_and(|item| item.bounds.contains(point))
            })
            .map(Vec::as_slice)
            .or(self.modal.as_deref())
            .unwrap_or_default();
        let context = Context {
            scene,
            layout,
            scope,
            modal: self.modal.as_deref(),
            transitions: &self.transitions,
            blocked: self.blocked,
        };
        let (target, local) = context.find(point, &|node, item| {
            let point = item.map_from_window(point)?;
            accepts(node, paths).then_some(Point {
                x: point.x - item.bounds.x,
                y: point.y - item.bounds.y,
            })
        })?;
        Some(FileHit { target, local })
    }
}

fn accepts(node: &Node, paths: &[String]) -> bool {
    if node.kind != "DropArea"
        || !node.boolean("accept-files", false)
        || matches!(node.properties.get("drop-action"), Some(Value::DragAction(action)) if *action != DragAction::Copy)
    {
        return false;
    }
    let filters = node.strings("file-extensions");
    filters.is_empty()
        || paths.iter().all(|path| {
            Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    filters.iter().any(|filter| {
                        extension.eq_ignore_ascii_case(filter.trim_start_matches('.'))
                    })
                })
        })
}

#[derive(Clone, Debug)]
struct FileHit {
    target: Target,
    local: Point,
}

#[derive(Clone, Debug)]
struct Hover {
    sequence: u64,
    point: Point,
    hit: FileHit,
    paths: Arc<[String]>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FileDrag {
    hover: Option<Hover>,
    completed: Option<u64>,
}

impl FileDrag {
    pub fn has_hover(&self) -> bool {
        self.hover.is_some()
    }

    pub fn update(
        &mut self,
        scope: &FileDropScope,
        scene: &Node,
        layout: &LayoutNode,
        input: FileDragInput,
    ) -> DragUpdate {
        let mut update = DragUpdate::default();
        match input {
            FileDragInput::Moved {
                sequence,
                point,
                paths,
            } if self.completed != Some(sequence) => {
                let next = scope.hit(scene, layout, point, &paths).map(|hit| Hover {
                    sequence,
                    point,
                    hit,
                    paths,
                });
                self.update_hover(next, &mut update);
            }
            FileDragInput::Dropped {
                sequence,
                point,
                paths,
                target,
            } if self.completed != Some(sequence) => {
                self.completed = Some(sequence);
                if scope.target(scene, layout, point, &paths).as_ref() == Some(&target) {
                    if self.hover.as_ref().is_some_and(|hover| {
                        hover.sequence == sequence && hover.hit.target == target
                    }) {
                        self.hover = None;
                    } else {
                        self.update_hover(None, &mut update);
                    }
                    send(&mut update, &target, DragEvent::FilesDropped(paths));
                } else if self
                    .hover
                    .as_ref()
                    .is_some_and(|hover| hover.sequence == sequence)
                {
                    self.update_hover(None, &mut update);
                }
            }
            FileDragInput::Left { sequence }
                if self
                    .hover
                    .as_ref()
                    .is_some_and(|hover| hover.sequence == sequence) =>
            {
                self.update_hover(None, &mut update);
            }
            _ => {}
        }
        update
    }

    pub fn reconcile(
        &mut self,
        scope: &FileDropScope,
        scene: &Node,
        layout: &LayoutNode,
    ) -> DragUpdate {
        let mut update = DragUpdate::default();
        if let Some(hover) = &self.hover
            && scope
                .target(scene, layout, hover.point, &hover.paths)
                .as_ref()
                != Some(&hover.hit.target)
        {
            self.update_hover(None, &mut update);
        }
        update
    }

    pub fn over(&self, node: &Node, path: &[usize]) -> bool {
        self.hover
            .as_ref()
            .is_some_and(|hover| hover.hit.target.path == path && hover.hit.target.id == node.id)
    }

    fn update_hover(&mut self, next: Option<Hover>, update: &mut DragUpdate) {
        let same = self
            .hover
            .as_ref()
            .zip(next.as_ref())
            .is_some_and(|(old, new)| {
                old.sequence == new.sequence
                    && old.hit.target == new.hit.target
                    && (Arc::ptr_eq(&old.paths, &new.paths) || old.paths == new.paths)
            });
        if !same {
            if let Some(old) = &self.hover {
                send(update, &old.hit.target, DragEvent::Exited);
            }
            if let Some(new) = &next {
                send(
                    update,
                    &new.hit.target,
                    DragEvent::FilesEntered(new.paths.clone()),
                );
            }
        }
        if let Some(new) = &next
            && (!same
                || self
                    .hover
                    .as_ref()
                    .is_none_or(|old| old.hit.local != new.hit.local))
        {
            send(
                update,
                &new.hit.target,
                DragEvent::Moved {
                    x: Length(new.hit.local.x.into()),
                    y: Length(new.hit.local.y.into()),
                },
            );
        }
        self.hover = next;
    }
}
