use crate::*;
use xrack_runtime::{
    GeometryHandle, GeometryMetric, Length, Orientation, Property, ScrollAction, ViewportHandle,
};

#[derive(Clone)]
pub(crate) enum Offset {
    Binding(Expr),
    Linked(Member),
}

pub(crate) fn offset_axis(name: &str) -> Option<Orientation> {
    match name {
        "content-x" => Some(Orientation::Horizontal),
        "content-y" => Some(Orientation::Vertical),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Metric {
    ContentX,
    ContentY,
    VisibleWidth,
    VisibleHeight,
    Dragging,
    Flicking,
    Moving,
    Rebounding,
    HorizontalOvershoot,
    VerticalOvershoot,
}

impl Metric {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "content-x" => Self::ContentX,
            "content-y" => Self::ContentY,
            "visible-width" => Self::VisibleWidth,
            "visible-height" => Self::VisibleHeight,
            "dragging" => Self::Dragging,
            "flicking" => Self::Flicking,
            "moving" => Self::Moving,
            "rebounding" => Self::Rebounding,
            "horizontal-overshoot" => Self::HorizontalOvershoot,
            "vertical-overshoot" => Self::VerticalOvershoot,
            _ => return None,
        })
    }

    pub(crate) fn read(self, handle: &ViewportHandle) -> Value {
        match self {
            Self::ContentX => handle.content_x().into(),
            Self::ContentY => handle.content_y().into(),
            Self::VisibleWidth => handle.visible_width().into(),
            Self::VisibleHeight => handle.visible_height().into(),
            Self::Dragging => handle.dragging().into(),
            Self::Flicking => handle.flicking().into(),
            Self::Moving => handle.moving().into(),
            Self::Rebounding => handle.rebounding().into(),
            Self::HorizontalOvershoot => handle.horizontal_overshoot().into(),
            Self::VerticalOvershoot => handle.vertical_overshoot().into(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Command {
    To,
    Index,
    Column,
    Cell,
    SourceIndex,
    SourceCell,
    CancelFlick,
    ReturnToBounds,
}

impl Command {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "scroll-to" => Self::To,
            "position-view-at-index" | "position-view-at-row" => Self::Index,
            "position-view-at-column" => Self::Column,
            "position-view-at-cell" => Self::Cell,
            "position-view-at-source-index" => Self::SourceIndex,
            "position-view-at-source-cell" => Self::SourceCell,
            "cancel-flick" => Self::CancelFlick,
            "return-to-bounds" => Self::ReturnToBounds,
            _ => return None,
        })
    }
}

pub(crate) struct Handles {
    geometry: BTreeMap<String, GeometryHandle>,
    viewports: BTreeMap<String, ViewportHandle>,
}

impl Handles {
    pub(crate) fn new(definition: &Definition, keys: &BTreeSet<&str>) -> Self {
        let geometry = definition
            .geometry_targets
            .iter()
            .filter(|key| keys.contains(key.as_str()))
            .map(|key| (key.clone(), GeometryHandle::default()))
            .collect();
        let viewports = definition
            .viewport_targets
            .iter()
            .filter(|key| keys.contains(key.as_str()))
            .filter_map(|key| {
                let element = definition.root.find_key(key)?;
                let notify = element.handlers.contains_key("scrolled")
                    || element.handlers.contains_key("flicked");
                let handle = match element.visual {
                    Visual::Builtin("Flickable") => ViewportHandle::for_flickable(notify),
                    _ if notify => ViewportHandle::with_scroll_notifications(),
                    _ => ViewportHandle::default(),
                };
                Some((key.clone(), handle))
            })
            .collect();
        Self {
            geometry,
            viewports,
        }
    }
}

impl Element {
    pub(crate) fn bind_viewport_offsets(&self, instance: &Instance) {
        if !self.offsets.is_empty() {
            let handle = &instance.0.layout.viewports[&self.key];
            for (axis, offset) in &self.offsets {
                let reader = Rc::downgrade(&instance.0);
                let writer = reader.clone();
                let binding = offset.clone();
                let target = offset.clone();
                let value = Rc::new(Property::<Result<Value>>::default());
                let written = value.clone();
                handle.bind_offset(
                    *axis,
                    move || {
                        let Some(instance) = reader.upgrade().map(Instance) else {
                            return Length::default();
                        };
                        let current = match &binding {
                            Offset::Linked(target) => instance.read(target),
                            Offset::Binding(expression) => {
                                value.get(|| instance.evaluate(expression, &[]))
                            }
                        };
                        match current {
                            Ok(Value::Length(length)) => length,
                            Ok(_) => Length::default(), // Verified by the dialect.
                            Err(error) => {
                                instance.0.error.set(Some(error));
                                Length::default()
                            }
                        }
                    },
                    move |length| match &target {
                        Offset::Linked(target) => {
                            if let Some(instance) = writer.upgrade().map(Instance)
                                && let Err(error) = instance.write(target, length.into())
                            {
                                instance.0.error.set(Some(error));
                            }
                        }
                        Offset::Binding(_) => written.set_if_changed(Ok(length.into())),
                    },
                );
            }
        }
        for child in self.children.iter().filter(|child| child.repeat.is_none()) {
            child.bind_viewport_offsets(instance);
        }
    }
}

impl Instance {
    pub(crate) fn geometry_read(&self, key: &str, metric: Option<GeometryMetric>) -> Result<Value> {
        if let Some(handle) = self.0.layout.geometry.get(key) {
            return Ok(match metric {
                Some(metric) => handle.read(metric).into(),
                None => handle.ready().into(),
            });
        }
        if let Some(owner) = self.row_owner() {
            return owner.geometry_read(key, metric);
        }
        Err(Error::Invalid(format!("unknown geometry target {key}")))
    }

    pub(crate) fn viewport(&self, key: &str) -> Result<ViewportHandle> {
        if let Some(handle) = self.0.layout.viewports.get(key) {
            return Ok(handle.clone());
        }
        if let Some(owner) = self.row_owner() {
            return owner.viewport(key);
        }
        Err(Error::Invalid(format!("unknown viewport target {key}")))
    }

    pub(crate) fn populate_layout_handles(&self, element: &Element, node: &mut Node) -> Result<()> {
        if let Some(handle) = self.0.layout.geometry.get(&element.key) {
            let field = match element.visual {
                Visual::Builtin(_) => &mut node.geometry_handle,
                Visual::Instance { .. } => &mut node.instance_geometry_handle,
            };
            *field = Some(handle.clone());
        }
        if let Some(handle) = self.0.layout.viewports.get(&element.key) {
            node.viewport_handle = Some(handle.clone());
        }
        if let Some(target) = &element.scroll_target {
            node.scroll_target = Some(self.viewport(target)?);
        }
        Ok(())
    }

    pub(crate) fn scroll_command(
        &self,
        target: &str,
        command: Command,
        args: &[Value],
    ) -> Result<()> {
        use Command::*;
        use Value::{Int, Length, ScrollAlignment};
        let action = match (command, args) {
            (CancelFlick, []) => ScrollAction::CancelFlick,
            (ReturnToBounds, []) => ScrollAction::ReturnToBounds,
            (To, [Length(x), Length(y)]) => ScrollAction::To { x: *x, y: *y },
            (Index, [Int(index), ScrollAlignment(alignment)]) => ScrollAction::ToIndex {
                index: *index,
                alignment: *alignment,
            },
            (Column, [Int(column), ScrollAlignment(alignment)]) => ScrollAction::ToColumn {
                column: *column,
                alignment: *alignment,
            },
            (
                Cell,
                [
                    Int(row),
                    Int(column),
                    ScrollAlignment(horizontal),
                    ScrollAlignment(vertical),
                ],
            ) => ScrollAction::ToCell {
                row: *row,
                column: *column,
                horizontal: *horizontal,
                vertical: *vertical,
            },
            (SourceIndex | SourceCell, [Int(index), ..]) => {
                let Ok(source_index) = usize::try_from(*index) else {
                    return Ok(());
                };
                let Some(key) = self.source_key(target, source_index)? else {
                    return Ok(());
                };
                match (command, &args[1..]) {
                    (SourceIndex, [ScrollAlignment(alignment)]) => ScrollAction::ToRowKey {
                        key,
                        source_index,
                        alignment: *alignment,
                    },
                    (
                        SourceCell,
                        [
                            Int(column),
                            ScrollAlignment(horizontal),
                            ScrollAlignment(vertical),
                        ],
                    ) => ScrollAction::ToCellAtRowKey {
                        key,
                        source_index,
                        column: *column,
                        horizontal: *horizontal,
                        vertical: *vertical,
                    },
                    _ => {
                        return Err(Error::Invalid(
                            "invalid source scroll command operands".into(),
                        ));
                    }
                }
            }
            _ => return Err(Error::Invalid("invalid scroll command operands".into())),
        };
        self.viewport(target)?.request(action);
        Ok(())
    }
}
