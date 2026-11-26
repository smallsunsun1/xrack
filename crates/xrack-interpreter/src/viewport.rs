use crate::*;
use xrack_runtime::{
    GeometryHandle, GeometryMetric, Length, Orientation, Property, ScrollAction, ScrollBarRange,
    ScrollBarStep, ScrollCommandMethod, ViewportHandle,
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
    EffectiveScrollBarWidth,
    EffectiveScrollBarHeight,
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
            "effective-scroll-bar-width" => Self::EffectiveScrollBarWidth,
            "effective-scroll-bar-height" => Self::EffectiveScrollBarHeight,
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
            Self::EffectiveScrollBarWidth => handle.effective_scroll_bar_width().into(),
            Self::EffectiveScrollBarHeight => handle.effective_scroll_bar_height().into(),
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
            .map(|key| {
                let handle = if definition.scene_view_targets.contains(key) {
                    GeometryHandle::for_scene_view()
                } else {
                    GeometryHandle::default()
                };
                (key.clone(), handle)
            })
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
                    Visual::Builtin("Flickable" | "Tumbler" | "PathView") => {
                        ViewportHandle::for_flickable(notify)
                    }
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
                        Offset::Binding(_) => {}
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
        self.geometry_value(key, |handle| match metric {
            Some(metric) => handle.read(metric).into(),
            None => handle.ready().into(),
        })
    }

    pub(crate) fn observed_scroll_metric(&self, key: &str, metric: &str) -> Result<Value> {
        self.geometry_value(key, |handle| match metric {
            "visual-position" => handle.scroll_visual_position().into(),
            "visual-size" => handle.scroll_visual_size().into(),
            "active" => handle.scroll_active().into(),
            "pressed" => handle.scroll_pressed().into(),
            "hovered" => handle.scroll_hovered().into(),
            _ => unreachable!("verified scroll metric"),
        })
    }

    pub(crate) fn scene_view_query(
        &self,
        key: &str,
        method: &str,
        inputs: &[Value],
    ) -> Result<Value> {
        let view = self.geometry_value(key, GeometryHandle::scene_view)?;
        Ok(match (method, inputs) {
            ("project", [Value::Point3D(point)]) => view.project(*point).into(),
            (
                "unproject",
                [
                    Value::Point2D(point),
                    Value::Float(depth),
                    Value::Point3D(fallback),
                ],
            ) => view.unproject(*point, *depth, *fallback).into(),
            ("pick", [Value::Point2D(point)]) => view.pick(*point).into(),
            ("hit", [Value::Point2D(point)]) => view.hit(*point).into(),
            ("hit-all", [Value::Point2D(point)]) => Value::Hit3DList(view.hit_all(*point)),
            ("hit-row", [Value::Point2D(point), Value::Int(row)]) => {
                view.hit_row(*point, *row).into()
            }
            ("hit-subset", [Value::Point2D(point), Value::IntList(rows)]) => {
                Value::Hit3DList(view.hit_subset(*point, rows))
            }
            ("hit-subset", [Value::Point2D(point), Value::IntModel(rows)]) => {
                Value::Hit3DList(view.hit_subset(*point, &rows.snapshot()))
            }
            (
                "hit-instance",
                [
                    Value::Point2D(point),
                    Value::Int(row),
                    Value::Int(node),
                    Value::Int(index),
                ],
            ) => view.hit_instance(*point, *row, *node, *index).into(),
            ("closest-point", [Value::Point3D(origin), Value::Float(radius)]) => {
                view.closest_point(*origin, *radius).into()
            }
            (
                "closest-point-row",
                [
                    Value::Point3D(origin),
                    Value::Float(radius),
                    Value::Int(row),
                ],
            ) => view.closest_point_row(*origin, *radius, *row).into(),
            (
                method,
                [
                    Value::Point3D(origin),
                    Value::Vector3D(direction),
                    Value::Float(max),
                    selection @ ..,
                ],
            ) => match (method, selection) {
                ("ray-hit", []) => view.ray_hit(*origin, *direction, *max).into(),
                ("ray-hit-all", []) => {
                    Value::Hit3DList(view.ray_hit_all(*origin, *direction, *max))
                }
                ("ray-hit-row", [Value::Int(row)]) => {
                    view.ray_hit_row(*origin, *direction, *max, *row).into()
                }
                ("ray-hit-subset", [Value::IntList(rows)]) => {
                    Value::Hit3DList(view.ray_hit_subset(*origin, *direction, *max, rows))
                }
                ("ray-hit-subset", [Value::IntModel(rows)]) => Value::Hit3DList(
                    view.ray_hit_subset(*origin, *direction, *max, &rows.snapshot()),
                ),
                ("ray-hit-instance", [Value::Int(row), Value::Int(node), Value::Int(index)]) => {
                    view.ray_hit_instance(*origin, *direction, *max, *row, *node, *index)
                        .into()
                }
                _ => {
                    return Err(Error::Invalid(format!(
                        "incorrect SceneView3D query {method}"
                    )));
                }
            },
            ("pick-in-rect", [Value::Point2D(start), Value::Point2D(end)]) => {
                Value::IntModel(view.pick_in_rect(*start, *end))
            }
            _ => {
                return Err(Error::Invalid(format!(
                    "incorrect SceneView3D query {method}"
                )));
            }
        })
    }

    fn geometry_value<T>(
        &self,
        key: &str,
        read: impl FnOnce(&xrack_runtime::GeometryHandle) -> T,
    ) -> Result<T> {
        if let Some(handle) = self.0.layout.geometry.get(key) {
            return Ok(read(handle));
        }
        if let Some(owner) = self.row_owner() {
            return owner.geometry_value(key, read);
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

    pub(crate) fn position_child(
        &self,
        view: &str,
        target: &str,
        horizontal: xrack_runtime::ScrollAlignment,
        vertical: xrack_runtime::ScrollAlignment,
    ) -> Result<()> {
        let target = self.geometry_value(target, GeometryHandle::scroll_target_id)?;
        self.viewport(view)?.request(ScrollAction::ToChild {
            target,
            horizontal,
            vertical,
        });
        Ok(())
    }

    pub(crate) fn scroll_command(
        &self,
        target: &str,
        method: ScrollCommandMethod,
        operands: &[Value],
    ) -> Result<()> {
        if let Some(direction) = method.bar_step() {
            return self.scroll_bar_step(target, direction);
        }
        let command = method
            .command(operands)
            .ok_or_else(|| Error::Invalid("invalid scroll command operands".into()))?;
        if let Some(action) = command.try_resolve(|index| self.source_key(target, index))? {
            self.viewport(target)?.request(action);
        }
        Ok(())
    }

    fn scroll_bar_step(&self, key: &str, direction: ScrollBarStep) -> Result<()> {
        let bar = self
            .definition()
            .root
            .find_key(key)
            .ok_or_else(|| Error::Invalid(format!("unknown scroll bar {key}")))?;
        let binding = |name| -> Result<Option<f64>> {
            Ok(match self.binding_value(bar, name)? {
                Some(Value::Float(value)) => Some(value),
                _ => None,
            })
        };
        if let Some(target) = &bar.scroll_target {
            let axis = match self.binding_value(bar, "orientation")? {
                Some(Value::Orientation(axis)) => axis,
                _ => Orientation::Vertical,
            };
            self.viewport(target)?.request(ScrollAction::StepBar {
                axis,
                step_size: binding("step-size")?,
                direction,
            });
        } else {
            let position = match self.control_value(bar, "position")? {
                Some(Value::Float(value)) => value,
                _ => 0.0,
            };
            let size = binding("size")?.unwrap_or(ScrollBarRange::DEFAULT_SIZE);
            let step = binding("step-size")?.unwrap_or(ScrollBarRange::DEFAULT_STEP);
            if let Some(value) = ScrollBarRange::new(position, size, step).apply(direction.action())
            {
                self.set_control_value(bar, "position", value.into())?;
            }
        }
        Ok(())
    }
}
