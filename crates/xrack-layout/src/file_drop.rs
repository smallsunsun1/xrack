use crate::{
    Interaction, LayoutNode, Point, Target, TextEditContext,
    drag::{Context, DragUpdate, send},
};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use xrack_runtime::{DragAction, DragEvent, Length, Node, Value};

/// Native file drags carry a batch and a window-local position.
#[derive(Debug)]
pub enum FileDragInput {
    Moved {
        sequence: u64,
        point: Point,
        paths: Arc<[String]>,
    },
    /// Promised files may expose extensions before they expose local paths.
    Offered {
        sequence: u64,
        point: Point,
        extensions: Arc<[Option<String>]>,
    },
    Dropped {
        sequence: u64,
        point: Point,
        paths: Arc<[String]>,
        target: Target,
    },
    /// An asynchronous drop has copied its complete batch into local storage.
    Copied {
        sequence: u64,
        point: Point,
        paths: Arc<[String]>,
        target: Target,
    },
    /// An accepted asynchronous drop could not prepare or copy its files.
    Failed {
        sequence: u64,
        point: Point,
        extensions: Arc<[Option<String>]>,
        target: Target,
        message: String,
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
    pub(crate) text_composing: bool,
    pub(crate) swiping: Option<Vec<usize>>,
}

/// The destination and editing state observed before a native provider starts loading.
#[doc(hidden)]
pub struct TextDrop {
    target: Target,
    point: Point,
    text: String,
    password: bool,
    focus: Option<Target>,
    editor: Option<TextEditContext>,
}

impl TextDrop {
    pub fn capture(
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        target: Target,
    ) -> Option<Self> {
        if input
            .file_drop_scope(layout)
            .text_target(scene, layout, point)?
            != target
        {
            return None;
        }
        let node = target.node_in(scene)?;
        Some(Self {
            text: input.editor_value(node).into_owned(),
            password: node.is_password(),
            focus: input.focused.clone(),
            editor: TextEditContext::capture(input, scene),
            target,
            point,
        })
    }

    pub fn destination(
        &self,
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        scope: &FileDropScope,
    ) -> Option<(&Target, Point)> {
        let node = self.target.node_in(scene)?;
        if input.focused != self.focus
            || self
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.is_current(input, scene))
            || input.editor_value(node) != self.text
            || node.is_password() != self.password
            || scope.text_target(scene, layout, self.point).as_ref() != Some(&self.target)
        {
            return None;
        }
        Some((&self.target, self.point))
    }
}

impl FileDropScope {
    pub(crate) fn scope_at(&self, layout: &LayoutNode, point: Point) -> &[usize] {
        self.popups
            .iter()
            .find(|path| {
                layout
                    .at_path(path)
                    .is_some_and(|item| item.bounds.contains(point))
            })
            .map(Vec::as_slice)
            .or(self.modal.as_deref())
            .unwrap_or_default()
    }

    pub fn target(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        paths: &[String],
    ) -> Option<Target> {
        self.hit_paths(scene, layout, point, paths)
            .map(|hit| hit.target)
    }

    pub fn target_for_extensions(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        extensions: &[Option<String>],
    ) -> Option<Target> {
        self.hit_extensions(scene, layout, point, extensions)
            .map(|hit| hit.target)
    }

    fn hit_paths(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        paths: &[String],
    ) -> Option<FileHit> {
        if paths.is_empty() || paths.iter().any(|path| !Path::new(path).is_absolute()) {
            return None;
        }
        self.hit(scene, layout, point, |filters| {
            paths.iter().all(|path| {
                accepts_extension(
                    filters,
                    Path::new(path).extension().and_then(|ext| ext.to_str()),
                )
            })
        })
    }

    fn hit_extensions(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        extensions: &[Option<String>],
    ) -> Option<FileHit> {
        if extensions.is_empty() {
            return None;
        }
        self.hit(scene, layout, point, |filters| {
            extensions.iter().all(|ext| {
                ext.as_deref()
                    .is_none_or(|ext| accepts_extension(filters, Some(ext)))
            })
        })
    }

    fn hit(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        extensions_match: impl Fn(&[String]) -> bool,
    ) -> Option<FileHit> {
        let context = Context {
            scene,
            layout,
            scope: self.scope_at(layout, point),
            modal: self.modal.as_deref(),
            transitions: &self.transitions,
            blocked: self.blocked,
        };
        let (target, local) = context.find(point, &|node, item| {
            let point = item.map_from_window(point)?;
            (node.kind == "DropArea"
                && node.boolean("accept-files", false)
                && !matches!(node.properties.get("drop-action"), Some(Value::DragAction(action)) if *action != DragAction::Copy)
                && extensions_match(node.strings("file-extensions")))
            .then_some(Point {
                x: point.x - item.bounds.x,
                y: point.y - item.bounds.y,
            })
        })?;
        Some(FileHit { target, local })
    }
}

fn accepts_extension(filters: &[String], extension: Option<&str>) -> bool {
    filters.is_empty()
        || extension.is_some_and(|extension| {
            filters
                .iter()
                .any(|filter| extension.eq_ignore_ascii_case(filter.trim_start_matches('.')))
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
    offer: Offer,
}

#[derive(Clone, Debug, PartialEq)]
enum Offer {
    Paths(Arc<[String]>),
    Extensions(Arc<[Option<String>]>),
}

impl Offer {
    fn hit(
        &self,
        scope: &FileDropScope,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<FileHit> {
        match self {
            Self::Paths(paths) => scope.hit_paths(scene, layout, point, paths),
            Self::Extensions(extensions) => scope.hit_extensions(scene, layout, point, extensions),
        }
    }
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
        let copied = matches!(input, FileDragInput::Copied { .. });
        match input {
            FileDragInput::Moved {
                sequence,
                point,
                paths,
            } if self.completed != Some(sequence) => {
                let offer = Offer::Paths(paths);
                let next = offer.hit(scope, scene, layout, point).map(|hit| Hover {
                    sequence,
                    point,
                    hit,
                    offer,
                });
                self.update_hover(next, &mut update);
            }
            FileDragInput::Offered {
                sequence,
                point,
                extensions,
            } if self.completed != Some(sequence) => {
                let offer = Offer::Extensions(extensions);
                let next = offer.hit(scope, scene, layout, point).map(|hit| Hover {
                    sequence,
                    point,
                    hit,
                    offer,
                });
                self.update_hover(next, &mut update);
            }
            FileDragInput::Dropped {
                sequence,
                point,
                paths,
                target,
            }
            | FileDragInput::Copied {
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
                    } else if !copied {
                        self.update_hover(None, &mut update);
                    }
                    if copied {
                        send(&mut update, &target, DragEvent::FilesEntered(paths.clone()));
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
            FileDragInput::Failed {
                sequence,
                point,
                extensions,
                target,
                message,
            } if self.completed != Some(sequence) => {
                self.completed = Some(sequence);
                if self
                    .hover
                    .as_ref()
                    .is_some_and(|hover| hover.sequence == sequence)
                {
                    self.update_hover(None, &mut update);
                }
                if scope
                    .target_for_extensions(scene, layout, point, &extensions)
                    .as_ref()
                    == Some(&target)
                {
                    send(&mut update, &target, DragEvent::FilesFailed(message));
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
            && hover
                .offer
                .hit(scope, scene, layout, hover.point)
                .map(|hit| hit.target)
                .as_ref()
                != Some(&hover.hit.target)
        {
            self.update_hover(None, &mut update);
        }
        update
    }

    pub fn over(&self, node: &Node, path: &[usize]) -> bool {
        self.hover.as_ref().is_some_and(|hover| {
            hover.hit.target.path == path && hover.hit.target.matches_node(node)
        })
    }

    fn update_hover(&mut self, next: Option<Hover>, update: &mut DragUpdate) {
        let same = self
            .hover
            .as_ref()
            .zip(next.as_ref())
            .is_some_and(|(old, new)| {
                old.sequence == new.sequence
                    && old.hit.target == new.hit.target
                    && old.offer == new.offer
            });
        if !same {
            if let Some(old) = &self.hover {
                send(update, &old.hit.target, DragEvent::Exited);
            }
            if let Some(new) = &next
                && let Offer::Paths(paths) = &new.offer
            {
                send(
                    update,
                    &new.hit.target,
                    DragEvent::FilesEntered(paths.clone()),
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
