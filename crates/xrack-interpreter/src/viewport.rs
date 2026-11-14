use crate::*;
use xrack_runtime::{
    GeometryHandle, GeometryMetric, Length, Orientation, Property, ScrollCommandMethod,
    ViewportHandle,
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
                let value = Rc::new(Property::<Length>::default());
                let written = value.clone();
                handle.bind_offset(
                    *axis,
                    move || {
                        let Some(instance) = reader.upgrade().map(Instance::from_state) else {
                            return Length::default();
                        };
                        let read_length = |value| match value {
                            Value::Length(length) => length,
                            _ => Length::default(), // Verified by the dialect.
                        };
                        let current = match &binding {
                            Offset::Linked(target) => instance.read(target).map(read_length),
                            Offset::Binding(expression) => value
                                .try_get(|| instance.evaluate(expression, &[]).map(read_length)),
                        };
                        instance.capture_result(current).unwrap_or_default()
                    },
                    move |length| match &target {
                        Offset::Linked(target) => {
                            if let Some(instance) = writer.upgrade().map(Instance::from_state) {
                                instance.capture_result(instance.write(target, length.into()));
                            }
                        }
                        Offset::Binding(_) => written.set_if_changed_by(length, |left, right| {
                            xrack_runtime::same_float_bits(&left.0, &right.0)
                        }),
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
            match element.visual {
                Visual::Builtin(_) => node.geometry_handle = Some(handle.clone()),
                Visual::Instance { .. } => node.set_instance_geometry_handle(handle.clone()),
            }
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
        method: ScrollCommandMethod,
        operands: &[Value],
    ) -> Result<()> {
        let command = method
            .command(operands)
            .ok_or_else(|| Error::Invalid("invalid scroll command operands".into()))?;
        if let Some(action) = command.try_resolve(|index| self.source_key(target, index))? {
            self.viewport(target)?.request(action);
        }
        Ok(())
    }
}
