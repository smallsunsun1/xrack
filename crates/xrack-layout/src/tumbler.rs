use crate::{ControlInput, LayoutNode, Point, Rect, Target, input::valid};
use xrack_runtime::{ControlAction, Node, RowKey, Tumbler};

#[derive(Clone, Debug, PartialEq)]
pub struct TumblerRow {
    pub index: usize,
    pub key: RowKey,
    pub label: String,
    pub bounds: Rect,
    pub displacement: f32,
}

impl TumblerRow {
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
}

impl TumblerGeometry {
    pub(crate) fn new(wheel: &Tumbler<'_>, bounds: Rect, offset: f32) -> Self {
        let height = bounds.height / wheel.visible() as f32;
        let center = Rect::new(
            bounds.x,
            bounds.y + (bounds.height - height) * 0.5,
            bounds.width,
            height,
        );
        let selected = wheel.selected();
        let mut rows = Vec::new();
        if let Some(selected) = selected
            && height > 0.0
            && bounds.width > 0.0
        {
            let middle = offset.round() as i64;
            let slots = if wheel.wraps() {
                (wheel.visible() + 2).min(wheel.count())
            } else {
                wheel.visible() + 2
            };
            let first = middle - (slots / 2) as i64;
            for step in first..first + slots as i64 {
                let displacement = step as f32 - offset;
                let bounds = Rect::new(
                    center.x,
                    center.y + displacement * height,
                    center.width,
                    height,
                );
                if displacement.abs() >= (wheel.visible() + 1) as f32 * 0.5 {
                    continue;
                }
                let Some(index) = wheel.index_at(selected as i64 + step) else {
                    continue;
                };
                if let Some((key, label)) = wheel.row_key(index).zip(wheel.label(index)) {
                    rows.push(TumblerRow {
                        index,
                        key,
                        label,
                        bounds,
                        displacement,
                    });
                }
            }
        }
        Self {
            center,
            rows,
            selected,
        }
    }

    pub(crate) fn hit(&self, point: Point) -> Option<ControlAction> {
        self.rows
            .iter()
            .find(|row| row.bounds.contains(point))
            .map(TumblerRow::action)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Configuration {
    structure: (u64, u64),
    selected: usize,
    visible: usize,
    wraps: bool,
}

impl Configuration {
    fn new(wheel: &Tumbler<'_>) -> Option<Self> {
        Some(Self {
            structure: wheel.structure_stamp(),
            selected: wheel.selected()?,
            visible: wheel.visible(),
            wraps: wheel.wraps(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TumblerDrag {
    pub target: Target,
    config: Configuration,
    pub start: Point,
    height: f32,
    pub offset: f32,
}

impl TumblerDrag {
    pub fn new(target: Target, node: &Node, item: &LayoutNode, start: Point) -> Option<Self> {
        let config = Configuration::new(&Tumbler::from_node(node))?;
        let height = item.tumbler.as_ref()?.center.height;
        (height > 0.0).then_some(Self {
            target,
            config,
            start,
            height,
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
        if !valid(&self.target, scene, layout)
            || layout
                .at_path(&self.target.path)?
                .tumbler
                .as_ref()?
                .center
                .height
                != self.height
        {
            return None;
        }
        let wheel = Tumbler::from_node(scene.at_path(&self.target.path)?);
        (Configuration::new(&wheel) == Some(self.config)).then_some(wheel)
    }

    pub fn update(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some(wheel) = self.resolve(scene, layout) else {
            return std::mem::replace(&mut self.offset, 0.0) != 0.0;
        };
        let distance = (self.start.y - point.y) / self.height;
        let limit = (wheel.count() - 1) as f32;
        let index = self.config.selected as f32;
        let offset = if wheel.wraps() {
            distance.clamp(-1_000_000.0, 1_000_000.0)
        } else {
            distance.clamp(-index, limit - index)
        };
        std::mem::replace(&mut self.offset, offset) != offset
    }

    pub fn finish(self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let wheel = self.resolve(scene, layout)?;
        let index = wheel.step(self.offset.round() as i64)?;
        (index != self.config.selected).then_some(ControlInput {
            action: wheel.select(index)?,
            path: self.target.path,
        })
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
    if index == config.selected {
        TumblerScroll::Pending
    } else {
        TumblerScroll::Select(ControlInput {
            path: target.path.clone(),
            action: wheel.select(index).expect("selected wheel row"),
        })
    }
}
