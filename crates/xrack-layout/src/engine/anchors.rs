use crate::{Rect, Size, TextMeasurer};
use crate::{item::LayoutServices, linear::Axis};
use std::collections::HashMap;
use xrack_runtime::Node;

pub(crate) fn has_anchors(node: &Node) -> bool {
    node.properties
        .range("anchor-"..)
        .take_while(|(name, _)| name.starts_with("anchor-"))
        .any(|(name, _)| {
            matches!(
                *name,
                "anchor-fill"
                    | "anchor-center-in"
                    | "anchor-left"
                    | "anchor-right"
                    | "anchor-top"
                    | "anchor-baseline"
                    | "anchor-bottom"
                    | "anchor-horizontal-center"
                    | "anchor-vertical-center"
            )
        })
}

#[derive(Clone, Copy)]
enum Target {
    Parent,
    Sibling(usize),
}

#[derive(Clone, Copy)]
struct Line {
    target: Target,
    fraction: Option<f32>,
}

#[derive(Clone, Copy, Default)]
struct Constraints {
    start: Option<Line>,
    end: Option<Line>,
    center: Option<Line>,
    baseline: Option<Line>,
}

impl Axis {
    fn interval(self, rect: Rect) -> (f32, f32) {
        match self {
            Self::Horizontal => (rect.x, rect.width),
            Self::Vertical => (rect.y, rect.height),
        }
    }

    fn set_interval(self, rect: &mut Rect, origin: f32, size: f32) {
        match self {
            Self::Horizontal => (rect.x, rect.width) = (origin, size),
            Self::Vertical => (rect.y, rect.height) = (origin, size),
        }
    }

    fn edge_names(self) -> [&'static str; 3] {
        match self {
            Self::Horizontal => ["left", "right", "horizontal-center"],
            Self::Vertical => ["top", "bottom", "vertical-center"],
        }
    }

    fn property_names(self) -> [&'static str; 3] {
        match self {
            Self::Horizontal => ["anchor-left", "anchor-right", "anchor-horizontal-center"],
            Self::Vertical => ["anchor-top", "anchor-bottom", "anchor-vertical-center"],
        }
    }
}

fn constraints(
    axis: Axis,
    child: &Node,
    index: usize,
    names: &HashMap<&str, Option<usize>>,
) -> Constraints {
    if !has_anchors(child) || !crate::participates_in_flow(child) {
        return Constraints::default();
    }
    let target = |name: &str| {
        if name == "parent" {
            Some(Target::Parent)
        } else {
            let sibling = names.get(name).copied().flatten()?;
            (index != sibling).then_some(Target::Sibling(sibling))
        }
    };
    let read_line = |name| {
        let (name, edge) = child.text(name)?.rsplit_once('.')?;
        let edges = axis.edge_names();
        let fraction = match edge {
            edge if edge == edges[0] => Some(0.0),
            edge if edge == edges[1] => Some(1.0),
            edge if edge == edges[2] => Some(0.5),
            "baseline" if axis == Axis::Vertical => None,
            _ => return None,
        };
        Some(Line {
            target: target(name)?,
            fraction,
        })
    };
    let [start, end, center] = axis.property_names();
    let fill = child.text("anchor-fill").and_then(target);
    let start = read_line(start).or_else(|| {
        fill.map(|target| Line {
            target,
            fraction: Some(0.0),
        })
    });
    let end = read_line(end).or_else(|| {
        fill.map(|target| Line {
            target,
            fraction: Some(1.0),
        })
    });
    let baseline = (axis == Axis::Vertical && start.is_none() && end.is_none())
        .then(|| read_line("anchor-baseline"))
        .flatten();
    let center = if start.is_none() && end.is_none() && baseline.is_none() {
        read_line(center).or_else(|| {
            Some(Line {
                target: target(child.text("anchor-center-in")?)?,
                fraction: Some(0.5),
            })
        })
    } else {
        None
    };
    Constraints {
        start,
        end,
        center,
        baseline,
    }
}

pub(crate) fn children(parent: &Node, content: Rect, text: &mut impl TextMeasurer) -> Vec<Rect> {
    let mut rects: Vec<_> = parent
        .children
        .iter()
        .map(|child| crate::absolute_child(text, child, content))
        .collect();
    apply(&parent.children, content, &mut rects, text, |_| false, None);
    rects
}

/// Resolve anchors against the content box. Fixed regions can be referenced
/// by their siblings while retaining the bounds assigned by their container.
pub(crate) fn apply(
    children: &[Node],
    content: Rect,
    rects: &mut [Rect],
    text: &mut dyn LayoutServices,
    fixed: impl Fn(&Node) -> bool,
    mut fixed_heights: Option<&mut [bool]>,
) {
    if !children
        .iter()
        .any(|child| has_anchors(child) && !fixed(child))
    {
        return;
    }
    let mut names = HashMap::new();
    for (index, child) in children.iter().enumerate() {
        if crate::participates_in_flow(child)
            && let Some(id) = child.element_id()
        {
            names
                .entry(id)
                .and_modify(|existing| *existing = None)
                .or_insert(Some(index));
        }
    }
    let reference = Size::new(content.width, content.height);
    let widths: Vec<_> = rects.iter().map(|rect| rect.width).collect();
    resolve_axis(
        Axis::Horizontal,
        children,
        content,
        &names,
        rects,
        (&fixed, |_| {}),
        text,
    );
    for ((child, rect), width) in children.iter().zip(rects.iter_mut()).zip(widths) {
        if rect.width != width {
            let metrics = text.metrics(child, reference, Some(rect.width));
            rect.height = crate::absolute_height(child, metrics.y, content.height);
        }
    }
    resolve_axis(
        Axis::Vertical,
        children,
        content,
        &names,
        rects,
        (&fixed, |index| {
            if let Some(heights) = fixed_heights.as_deref_mut() {
                heights[index] = true;
            }
        }),
        text,
    );
}

fn resolve_axis(
    axis: Axis,
    children: &[Node],
    content: Rect,
    names: &HashMap<&str, Option<usize>>,
    rects: &mut [Rect],
    (fixed, mut on_resolved): (&impl Fn(&Node) -> bool, impl FnMut(usize)),
    text: &mut dyn LayoutServices,
) {
    let constraints: Vec<_> = children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            if fixed(child) {
                Constraints::default()
            } else {
                constraints(axis, child, index, names)
            }
        })
        .collect();
    let mut dependents = vec![Vec::new(); children.len()];
    let mut pending = vec![0; children.len()];
    let mut ready = Vec::with_capacity(children.len());
    for (index, lines) in constraints.iter().enumerate() {
        for line in [lines.start, lines.end, lines.center, lines.baseline]
            .into_iter()
            .flatten()
        {
            if let Target::Sibling(sibling) = line.target {
                dependents[sibling].push(index);
                pending[index] += 1;
            }
        }
        if pending[index] == 0 {
            ready.push(index);
        }
    }
    // Resolve each axis in dependency order. Cycles and their dependents keep
    // the regular absolute placement on that axis.
    while let Some(index) = ready.pop() {
        let lines = constraints[index];
        let child = &children[index];
        let mut coordinate = |line: Line| {
            let rect = match line.target {
                Target::Parent => content,
                Target::Sibling(sibling) => rects[sibling],
            };
            let (origin, extent) = axis.interval(rect);
            origin
                + match (line.fraction, line.target) {
                    (Some(fraction), _) => fraction * extent,
                    (None, Target::Parent) => 0.0,
                    (None, Target::Sibling(sibling)) => crate::item::baseline(
                        &children[sibling],
                        Size::new(rect.width, rect.height),
                        text,
                    ),
                }
        };
        let margin = child
            .length("anchor-margin")
            .filter(|margin| margin.is_finite())
            .unwrap_or(0.0);
        let start = lines.start.map(|line| coordinate(line) + margin);
        let end = lines.end.map(|line| coordinate(line) - margin);
        let center = lines.center.map(&mut coordinate);
        let baseline = lines.baseline.map(coordinate);
        let rect = rects[index];
        let reference = Size::new(content.width, content.height);
        let preferred = |text: &mut dyn LayoutServices| match axis {
            Axis::Horizontal => text.metrics(child, reference, None).x.preferred,
            Axis::Vertical => text.metrics(child, reference, Some(rect.width)).y.preferred,
        };

        let interval = match (start, end, center) {
            (Some(start), Some(end), _) => Some((start, (end - start).max(0.0))),
            (Some(start), None, _) => Some((start, preferred(text))),
            (None, Some(end), _) => {
                let size = preferred(text);
                Some((end - size, size))
            }
            (None, None, Some(center)) => {
                let size = preferred(text);
                Some((center - size * 0.5, size))
            }
            _ => baseline.map(|baseline| {
                let height = preferred(text);
                let offset = crate::item::baseline(child, Size::new(rect.width, height), text);
                (baseline - offset, height)
            }),
        };
        if let Some((origin, size)) = interval {
            axis.set_interval(&mut rects[index], origin, size);
            on_resolved(index);
        }
        for &dependent in &dependents[index] {
            pending[dependent] -= 1;
            if pending[dependent] == 0 {
                ready.push(dependent);
            }
        }
    }
}
