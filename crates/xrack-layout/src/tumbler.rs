use crate::{
    ControlInput, LayoutNode, LayoutSpace, Point, Rect, Size, Target, TextMeasurer, Transform2D,
    WindowSpace, input::valid, path_view::MeasuredPath, scroll_gesture::SCROLL_FRAME_INTERVAL,
};
use std::time::{Duration, Instant};
use xrack_runtime::{
    ControlAction, LayoutDirection, Node, Orientation, RowKey, Tumbler, ViewportHandle,
    ViewportMotion,
};

const DEFAULT_FLICK_DECELERATION: f64 = 2400.0;
const MIN_FLICK_VELOCITY: f32 = 220.0;
const MAX_FLICK_SECONDS: f32 = 0.55;

#[derive(Clone, Debug, PartialEq)]
pub struct TumblerRow {
    pub index: usize,
    pub key: RowKey,
    pub label: String,
    pub bounds: Rect,
    pub displacement: f32,
    pub(crate) progress: f32,
    pub(crate) content: Option<Box<(Node, LayoutNode)>>,
}

impl TumblerRow {
    pub fn content(&self) -> Option<(&Node, &LayoutNode)> {
        self.content.as_deref().map(|(node, layout)| (node, layout))
    }

    pub fn action(&self) -> ControlAction {
        ControlAction::PickTumbler {
            index: self.index,
            row: self.key,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TumblerGeometry {
    pub center: Rect,
    pub rows: Vec<TumblerRow>,
    pub selected: Option<usize>,
    pub(crate) visible: usize,
    path: Option<PathLayout>,
}

#[derive(Clone, Debug, PartialEq)]
struct PathLayout {
    commands: String,
    bounds: Rect,
    step: f32,
    axis: Orientation,
    mirrored: bool,
}

impl TumblerGeometry {
    pub(crate) fn new(
        wheel: &Tumbler<'_>,
        bounds: Rect,
        offset: f32,
        row_height: Option<f32>,
    ) -> Self {
        let visible = row_height.map_or(wheel.visible(), |height| {
            let fit = ((bounds.height / height) as usize).clamp(1, wheel.visible());
            // Keep a centered row with the same number of slots on either side.
            (fit - 1) | 1
        });
        let height = bounds.height / visible as f32;
        let center = Rect::new(
            bounds.x,
            bounds.y + (bounds.height - height) * 0.5,
            bounds.width,
            height,
        );
        let rows = if height > 0.0 && bounds.width > 0.0 {
            visible_options(wheel, visible, offset)
                .filter_map(|(index, displacement)| {
                    let bounds = Rect::new(
                        center.x,
                        center.y + displacement * height,
                        center.width,
                        height,
                    );
                    option_row(wheel, index, bounds, displacement, 0.5)
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            center,
            rows,
            selected: wheel.selected(),
            visible,
            path: None,
        }
    }

    pub(crate) fn along_path(
        node: &Node,
        wheel: &Tumbler<'_>,
        bounds: Rect,
        offset: f32,
        direction: LayoutDirection,
    ) -> Option<Self> {
        if !(bounds.width > 0.0 && bounds.height > 0.0) {
            return None;
        }
        let commands = node.text("path").unwrap_or("M 0 0.5 L 1 0.5");
        let width = node
            .length("item-width")
            .unwrap_or(96.0)
            .clamp(0.0, bounds.width);
        let height = node
            .length("item-height")
            .unwrap_or(72.0)
            .clamp(0.0, bounds.height);
        if !(width > 0.0 && height > 0.0) {
            return None;
        }
        let mirrored = direction == LayoutDirection::RightToLeft;
        let view_box = Rect::new(
            bounds.x + width * 0.5,
            bounds.y + height * 0.5,
            bounds.width - width,
            bounds.height - height,
        );
        let path = if let Some((_, segments)) = node.vector_path_geometry(commands) {
            MeasuredPath::from_segments(segments.iter().copied(), view_box, mirrored)?
        } else {
            MeasuredPath::new(commands, view_box, mirrored)?
        };
        let centered =
            |point: Point| Rect::new(point.x - width * 0.5, point.y - height * 0.5, width, height);
        let closed = path.closed();
        let mut rows: Vec<_> = visible_options(wheel, wheel.visible(), offset)
            .filter_map(|(index, displacement)| {
                let progress = 0.5 + displacement / wheel.visible() as f32;
                if !(0.0..=1.0).contains(&progress) || (closed && progress == 1.0) {
                    return None;
                }
                option_row(
                    wheel,
                    index,
                    centered(path.point_at(progress)),
                    displacement,
                    progress,
                )
            })
            .collect();
        // Paint farther options behind nearer ones, and resolve clicks in reverse paint order.
        rows.sort_by(|a, b| b.displacement.abs().total_cmp(&a.displacement.abs()));
        Some(Self {
            center: centered(path.point_at(0.5)),
            rows,
            selected: wheel.selected(),
            visible: wheel.visible(),
            path: Some(PathLayout {
                commands: commands.to_owned(),
                bounds: view_box,
                step: path.length() / wheel.visible() as f32,
                axis: match node.properties.get("orientation") {
                    Some(xrack_runtime::Value::Orientation(axis)) => *axis,
                    _ => Orientation::Horizontal,
                },
                mirrored,
            }),
        })
    }

    pub(crate) fn hit(&self, point: Point) -> Option<ControlAction> {
        let hit = |row: &&TumblerRow| row.bounds.contains(point);
        if self.path.is_some() {
            self.rows.iter().rev().find(hit)
        } else {
            self.rows.iter().find(hit)
        }
        .map(TumblerRow::action)
    }

    pub(crate) fn step_size(&self) -> f32 {
        self.path
            .as_ref()
            .map_or(self.center.height, |path| path.step)
    }

    pub(crate) fn navigation_axis(&self) -> (Orientation, bool) {
        self.path
            .as_ref()
            .map_or((Orientation::Vertical, false), |path| {
                (
                    path.axis,
                    path.mirrored && path.axis == Orientation::Horizontal,
                )
            })
    }
}

pub(crate) fn scaled_row_height(node: &Node, text: &impl TextMeasurer) -> Option<f32> {
    (text.text_scale() > 1.0 && node.combo_option_renderer.is_none())
        .then(|| (text.font_size(node) * 1.6 + 4.0).max(44.0))
}

pub(crate) fn preferred_size(node: &Node, text: &impl TextMeasurer) -> Size {
    let preferred = scaled_row_height(node, text).map_or(148.0, |height| {
        height * Tumbler::from_node(node).visible() as f32
    });
    Size::new(112.0, preferred)
}

fn visible_options<'a>(
    wheel: &'a Tumbler<'a>,
    visible: usize,
    offset: f32,
) -> impl Iterator<Item = (usize, f32)> + 'a {
    let middle = offset.round() as i64;
    let slots = if wheel.wraps() {
        (visible + 2).min(wheel.count())
    } else {
        visible + 2
    };
    let first = middle - (slots / 2) as i64;
    (first..first + slots as i64).filter_map(move |step| {
        let displacement = step as f32 - offset;
        if displacement.abs() >= (visible + 1) as f32 * 0.5 {
            return None;
        }
        let index = wheel.index_at(wheel.selected()? as i64 + step)?;
        Some((index, displacement))
    })
}

fn option_row(
    wheel: &Tumbler<'_>,
    index: usize,
    bounds: Rect,
    displacement: f32,
    progress: f32,
) -> Option<TumblerRow> {
    Some(TumblerRow {
        index,
        key: wheel.row_key(index)?,
        label: wheel.label(index)?,
        bounds,
        displacement,
        progress,
        content: None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Configuration {
    structure: (u64, u64),
    projection_revision: u64,
    selected: usize,
    visible: usize,
    wraps: bool,
}

impl Configuration {
    fn new(wheel: &Tumbler<'_>) -> Option<Self> {
        Some(Self {
            structure: wheel.structure_stamp(),
            projection_revision: wheel.projection_revision(),
            selected: wheel.selected()?,
            visible: wheel.visible(),
            wraps: wheel.wraps(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TumblerDrag {
    pub target: Target,
    pub(crate) picker_session: Option<i64>,
    config: Configuration,
    pub start: Point,
    step: f32,
    path: Option<PathLayout>,
    transform: Transform2D<LayoutSpace, WindowSpace>,
    motion: Option<ViewportHandle>,
    dragging: bool,
    pub offset: f32,
}

impl TumblerDrag {
    pub fn new(target: Target, node: &Node, item: &LayoutNode, start: Point) -> Option<Self> {
        let config = Configuration::new(&Tumbler::from_node(node))?;
        let geometry = item.tumbler.as_ref()?;
        let step = geometry.step_size();
        (step > 0.0).then_some(Self {
            target,
            picker_session: node.picker_session_id(),
            config,
            start,
            step,
            path: geometry.path.clone(),
            transform: item.world_transform(),
            motion: node.viewport_handle.clone(),
            dragging: false,
            offset: 0.0,
        })
    }

    pub fn offset_for(&self, wheel: &Tumbler<'_>, path: &[usize]) -> f32 {
        if self.target.path == path && Configuration::new(wheel) == Some(self.config) {
            self.offset
        } else {
            0.0
        }
    }

    fn resolve<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<Tumbler<'a>> {
        let item = layout.at_path(&self.target.path)?;
        let node = scene.at_path(&self.target.path)?;
        let geometry = item.tumbler.as_ref()?;
        if !valid(&self.target, scene, layout)
            || node.picker_session_id() != self.picker_session
            || geometry.step_size() != self.step
            || geometry.path != self.path
            || item.world_transform() != self.transform
            || node.viewport_handle != self.motion
        {
            return None;
        }
        let wheel = Tumbler::from_node(node);
        (Configuration::new(&wheel) == Some(self.config)).then_some(wheel)
    }

    fn project(&self, delta: Point) -> (f32, f32) {
        match &self.path {
            Some(path) if path.axis == Orientation::Horizontal => {
                (if path.mirrored { -delta.x } else { delta.x }, delta.y)
            }
            _ => (delta.y, delta.x),
        }
    }

    pub fn accepts_direction(&self, point: Point) -> bool {
        let (along, across) = self.project(Point {
            x: self.start.x - point.x,
            y: self.start.y - point.y,
        });
        along.abs() > across.abs()
    }

    fn bounded_offset(&self, wheel: &Tumbler<'_>, offset: f32) -> f32 {
        let limit = (wheel.count() - 1) as f32;
        let index = self.config.selected as f32;
        if wheel.wraps() {
            offset.clamp(-1_000_000.0, 1_000_000.0)
        } else {
            offset.clamp(-index, limit - index)
        }
    }

    pub fn is_current(&self, scene: &Node, layout: &LayoutNode) -> bool {
        self.resolve(scene, layout).is_some()
    }

    fn publish(&self, phase: ViewportMotion) {
        if let Some(motion) = &self.motion {
            motion.publish_motion(phase);
        }
    }

    pub fn cancel(&mut self) -> bool {
        self.publish(ViewportMotion::Idle);
        let had_preview = self.dragging || self.offset != 0.0;
        self.dragging = false;
        self.offset = 0.0;
        had_preview
    }

    pub fn update(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some(wheel) = self.resolve(scene, layout) else {
            return self.cancel();
        };
        let (distance, _) = self.project(Point {
            x: self.start.x - point.x,
            y: self.start.y - point.y,
        });
        let offset = self.bounded_offset(&wheel, distance / self.step);
        let moved = std::mem::replace(&mut self.offset, offset) != offset;
        if !std::mem::replace(&mut self.dragging, true) {
            self.publish(ViewportMotion::Dragging);
            return true;
        }
        moved
    }

    pub fn settle(
        self,
        scene: &Node,
        layout: &LayoutNode,
        velocity: Option<Point>,
        now: Instant,
    ) -> Result<TumblerFlick, Self> {
        let Some((target, duration)) = self.flick_destination(scene, layout, velocity) else {
            return Err(self);
        };
        self.publish(ViewportMotion::Flicking);
        Ok(TumblerFlick {
            from: self.offset,
            to: target,
            start: now,
            wake: now + SCROLL_FRAME_INTERVAL,
            duration,
            drag: self,
            interrupted: false,
        })
    }

    fn flick_destination(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        velocity: Option<Point>,
    ) -> Option<(f32, Duration)> {
        let wheel = self.resolve(scene, layout)?;
        let node = scene.at_path(&self.target.path)?;
        let deceleration = node
            .number("flick-deceleration")
            .unwrap_or(DEFAULT_FLICK_DECELERATION);
        if !deceleration.is_finite() || deceleration <= 0.0 {
            return None;
        }
        let delta = layout
            .at_path(&self.target.path)?
            .map_delta_from_window(velocity?)?;
        let speed = self.project(delta).0.clamp(-3200.0, 3200.0);
        if !self.offset.is_finite() || !speed.is_finite() || speed.abs() < MIN_FLICK_VELOCITY {
            return None;
        }
        let braking =
            (f64::from(speed.abs()) / deceleration).min(f64::from(MAX_FLICK_SECONDS)) as f32;
        let projected = self.offset + speed * braking * 0.5 / self.step;
        let target = self.bounded_offset(&wheel, projected).round();
        if target == self.offset.round() {
            return None;
        }
        let seconds = (2.0 * (target - self.offset).abs() * self.step / speed.abs())
            .clamp(0.08, MAX_FLICK_SECONDS);
        Some((target, Duration::from_secs_f32(seconds)))
    }

    pub fn finish(self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        self.publish(ViewportMotion::Idle);
        let wheel = self.resolve(scene, layout)?;
        let index = wheel.step(self.offset.round() as i64)?;
        (index != self.config.selected).then_some(ControlInput {
            action: wheel.select(index)?,
            path: self.target.path,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TumblerFlick {
    pub drag: TumblerDrag,
    pub wake: Instant,
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    interrupted: bool,
}

impl TumblerFlick {
    pub fn active(&self) -> bool {
        !self.interrupted
    }

    pub fn interrupt(&mut self) -> bool {
        if std::mem::replace(&mut self.interrupted, true) {
            return false;
        }
        self.drag.cancel();
        true
    }

    /// Sample the timed snap independently of the host's frame rate.
    pub fn advance(&mut self, now: Instant) -> bool {
        let t = (now.saturating_duration_since(self.start).as_secs_f32()
            / self.duration.as_secs_f32())
        .min(1.0);
        self.drag.offset = self.from + (self.to - self.from) * t * (2.0 - t);
        self.wake = (now + SCROLL_FRAME_INTERVAL).min(self.start + self.duration);
        t == 1.0
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TumblerWheel {
    target: Target,
    config: Configuration,
    remainder: f32,
}

pub enum TumblerScroll {
    Unhandled,
    Pending,
    Select(ControlInput),
}

pub(crate) fn scroll(
    accumulation: &mut Option<TumblerWheel>,
    target: &Target,
    wheel: &Tumbler<'_>,
    height: f32,
    delta: f32,
    lines: bool,
) -> TumblerScroll {
    let Some(config) = Configuration::new(wheel) else {
        return TumblerScroll::Unhandled;
    };
    if height <= 0.0
        || !delta.is_finite()
        || delta == 0.0
        || wheel.step(delta.signum() as i64) == Some(config.selected)
    {
        *accumulation = None;
        return TumblerScroll::Unhandled;
    }
    let mut rows = if lines {
        delta.signum() * delta.abs().round().max(1.0)
    } else {
        delta / height
    };
    if !lines
        && let Some(previous) = accumulation.as_ref()
        && previous.target == *target
        && previous.config == config
    {
        rows += previous.remainder;
    }
    rows = rows.clamp(-1_000_000.0, 1_000_000.0);
    let steps = rows.trunc() as i64;
    let index = wheel.step(steps).unwrap_or(config.selected);
    *accumulation = (!lines).then(|| TumblerWheel {
        target: target.clone(),
        config: Configuration {
            selected: index,
            ..config
        },
        remainder: rows.fract(),
    });
    if index != config.selected
        && let Some(action) = wheel.select(index)
    {
        TumblerScroll::Select(ControlInput {
            path: target.path.clone(),
            action,
        })
    } else {
        TumblerScroll::Pending
    }
}
