use crate::{Theme, color};
use femtovg::{Canvas, LineCap, Paint, Path, Solidity, renderer::WGPURenderer};
use xrack_layout::{DialGeometry, Point, Rect, ScrollBarGeometry, SliderGeometry};
use xrack_runtime::{Color, IndicatorState, Node, Orientation, SliderInterval, SliderRange};

fn capsule(canvas: &mut Canvas<WGPURenderer>, bounds: Rect, tint: Color) {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }
    let mut path = Path::new();
    path.rounded_rect(
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        bounds.width.min(bounds.height) * 0.5,
    );
    canvas.fill_path(&path, &Paint::color(color(tint)));
}

pub(crate) fn scroll_bar(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Theme,
    geometry: ScrollBarGeometry,
    enabled: bool,
    highlighted: bool,
) {
    let thickness = geometry.track.width.min(geometry.track.height);
    let inset = (thickness * 0.15).min(1.5);
    capsule(canvas, geometry.track.inset(inset), Color(0x202c42d9));
    let tint = if !enabled || !geometry.scrollable() {
        theme.disabled_text
    } else if highlighted {
        theme.focus
    } else {
        Color(0x9bb4e3d9)
    };
    capsule(canvas, geometry.thumb.inset(inset), tint);
}

fn track(from: Point, to: Point, width: f32, orientation: Orientation) -> Rect {
    match orientation {
        Orientation::Horizontal => Rect::new(
            from.x,
            from.y - width * 0.5,
            (to.x - from.x).max(0.0),
            width,
        ),
        Orientation::Vertical => {
            Rect::new(from.x - width * 0.5, to.y, width, (from.y - to.y).max(0.0))
        }
    }
}

pub(crate) fn slider(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    enabled: bool,
    pressed: bool,
) {
    let orientation = node.orientation();
    let geometry = SliderGeometry::new(bounds, orientation);
    let start = geometry.position(0.0);
    let end = geometry.position(1.0);
    let thumb = geometry.position(SliderRange::from_node(node).fraction());
    let thickness = (geometry.radius * 2.0).min(6.0);
    let fill = if enabled {
        theme.button
    } else {
        theme.disabled
    };
    capsule(
        canvas,
        track(start, end, thickness, orientation),
        Color(0x35415bff),
    );
    capsule(canvas, track(start, thumb, thickness, orientation), fill);
    knob(canvas, theme, thumb, geometry.radius, enabled, pressed);
}

fn knob(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Theme,
    thumb: Point,
    radius: f32,
    enabled: bool,
    active: bool,
) {
    let mut knob = Path::new();
    knob.circle(thumb.x, thumb.y, radius);
    let knob_color = if !enabled {
        theme.disabled_text
    } else if active {
        theme.focus
    } else {
        theme.text
    };
    canvas.fill_path(&knob, &Paint::color(color(knob_color)));
}

pub(crate) fn interval(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    enabled: bool,
    active: [bool; 2],
) {
    let orientation = node.orientation();
    let geometry = SliderGeometry::new(bounds, orientation);
    let thumbs = SliderInterval::from_node(node)
        .fractions()
        .map(|t| geometry.position(t));
    let thickness = (geometry.radius * 2.0).min(6.0);
    capsule(
        canvas,
        track(
            geometry.position(0.0),
            geometry.position(1.0),
            thickness,
            orientation,
        ),
        Color(0x35415bff),
    );
    capsule(
        canvas,
        track(thumbs[0], thumbs[1], thickness, orientation),
        if enabled {
            theme.button
        } else {
            theme.disabled
        },
    );
    let order = if active[0] { [1, 0] } else { [0, 1] };
    for index in order {
        knob(
            canvas,
            theme,
            thumbs[index],
            geometry.radius,
            enabled,
            active[index],
        );
    }
}

pub(crate) fn dial(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    enabled: bool,
    active: bool,
) {
    let geometry = DialGeometry::new(bounds, node);
    if geometry.track_radius <= 0.0 {
        return;
    }
    let center = geometry.center;
    let mut face = Path::new();
    face.circle(center.x, center.y, geometry.face_radius - 1.0);
    canvas.fill_path(&face, &Paint::color(color(Color(0x27354dff))));
    let fraction = SliderRange::from_node(node).fraction();
    for (end, tint) in [
        (1.0, Color(0x586582ff)),
        (
            fraction,
            if enabled {
                theme.button
            } else {
                theme.disabled
            },
        ),
    ] {
        if end <= 0.0 {
            continue;
        }
        let mut path = Path::new();
        let angle = |position| (geometry.angle(position) - std::f64::consts::FRAC_PI_2) as f32;
        path.arc(
            center.x,
            center.y,
            geometry.track_radius,
            angle(0.0),
            angle(end),
            Solidity::Hole,
        );
        let mut paint = Paint::color(color(tint));
        paint.set_line_width((geometry.thumb_radius * 0.75).max(1.0));
        paint.set_line_cap(LineCap::Round);
        canvas.stroke_path(&path, &paint);
    }
    knob(
        canvas,
        theme,
        geometry.position(fraction),
        geometry.thumb_radius,
        enabled,
        active,
    );
}

pub(crate) fn indicator(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    state: IndicatorState,
    phase: Option<f32>,
    enabled: bool,
) -> bool {
    let (progress, motion) = match state {
        IndicatorState::Hidden => return false,
        IndicatorState::Determinate(progress) => (progress as f32, None),
        IndicatorState::Indeterminate => (0.27, phase),
    };
    let foreground = if enabled {
        node.color("color").unwrap_or(theme.button)
    } else {
        theme.disabled
    };
    let track = node
        .color("track-color")
        .unwrap_or(if node.kind == "BusyIndicator" {
            Color(0)
        } else {
            Color(0x35415bff)
        });
    if node.kind == "ProgressIndicator" {
        capsule(canvas, bounds, track);
        let width = bounds.width * progress;
        let position = if state == IndicatorState::Indeterminate {
            motion.map_or(0.5, |phase| 1.0 - (2.0 * phase - 1.0).abs())
        } else {
            0.0
        };
        capsule(
            canvas,
            Rect::new(
                bounds.x + (bounds.width - width) * position,
                bounds.y,
                width,
                bounds.height,
            ),
            foreground,
        );
    } else {
        let diameter = bounds.width.min(bounds.height);
        let stroke = node
            .length("stroke-width")
            .unwrap_or(4.0)
            .clamp(0.0, diameter * 0.5);
        if stroke <= 0.0 {
            return false;
        }
        let radius = (diameter - stroke) * 0.5;
        let x = bounds.x + bounds.width * 0.5;
        let y = bounds.y + bounds.height * 0.5;
        let arc = |canvas: &mut Canvas<WGPURenderer>, start: f32, sweep: f32, tint: Color| {
            let mut path = Path::new();
            if sweep >= 1.0 {
                path.circle(x, y, radius);
            } else {
                let start = (start * std::f32::consts::TAU) - std::f32::consts::FRAC_PI_2;
                path.arc(
                    x,
                    y,
                    radius,
                    start,
                    start + sweep * std::f32::consts::TAU,
                    Solidity::Hole,
                );
            }
            let mut paint = Paint::color(color(tint));
            paint.set_line_width(stroke);
            paint.set_line_cap(LineCap::Round);
            canvas.stroke_path(&path, &paint);
        };
        if track.0 & 0xff != 0 {
            arc(canvas, 0.0, 1.0, track);
        }
        if progress > 0.0 {
            arc(canvas, motion.unwrap_or(0.0), progress, foreground);
        }
    }
    motion.is_some() && foreground.0 & 0xff != 0
}
