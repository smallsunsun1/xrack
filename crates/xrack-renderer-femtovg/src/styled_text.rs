use crate::{
    color,
    text::{TextSystem, fill_selected_text},
    theme::Palette,
};
use femtovg::{Canvas, Paint, Path, renderer::WGPURenderer};
use std::{collections::HashMap, ops::Range};
use xrack_layout::{LayoutNode, Point, Size, StyledTextLayout, TextMeasurer};
use xrack_runtime::{Node, TextStyle};

pub(crate) fn layout(
    system: &TextSystem,
    node: &Node,
    available: Size,
    selection_geometry: bool,
) -> Option<StyledTextLayout> {
    let document = node.styled_text("text")?;
    let mut paints = HashMap::new();
    let mut layout = StyledTextLayout::new(
        node,
        document,
        available,
        system.font_size(node),
        |text, style| {
            let paint = paints
                .entry(style)
                .or_insert_with(|| system.paint_styled(node, femtovg::Color::white(), style));
            system.advance_painted(text, paint)
        },
    );
    if selection_geometry && node.selectable_styled_text() {
        layout.shape_selection(document, |text, style| {
            let paint = paints
                .entry(style)
                .or_insert_with(|| system.paint_styled(node, femtovg::Color::white(), style));
            system.shape_visual_line(text, paint)
        });
    }
    Some(layout)
}

pub(crate) fn draw(
    canvas: &mut Canvas<WGPURenderer>,
    system: &TextSystem,
    node: &Node,
    layout: &LayoutNode,
    theme: Palette,
    focused: Option<usize>,
    selection: Range<usize>,
) -> Result<(), String> {
    let (Some(document), Some(plan)) = (node.styled_text("text"), &layout.styled_text) else {
        return Ok(());
    };
    let bounds = layout.bounds;
    let tint = node.color("color").unwrap_or(theme.text);
    let link_tint = node.color("link-color").unwrap_or(theme.focus);
    let highlight = Paint::color(color(theme.text_selection(node)));
    let selected_color = node.color("selected-text-color").map(color);
    let mut paints = HashMap::new();
    let base = system.paint_styled(node, color(tint), TextStyle::default());
    let baseline = system
        .context()
        .measure_font(&base)
        .map_or_else(|_| system.font_size(node), |metrics| metrics.ascender());
    for fragment in &plan.fragments {
        let spans =
            fragment
                .selection_geometry()
                .map_or_else(Vec::new, |(visual, range, prefix)| {
                    let start = selection.start.max(range.start);
                    let end = selection.end.min(range.end);
                    if start < end {
                        visual.selection_spans(
                            start - range.start + prefix..end - range.start + prefix,
                        )
                    } else {
                        Vec::new()
                    }
                });
        let x = bounds.x + fragment.bounds.x;
        let y = bounds.y + fragment.bounds.y;
        if !spans.is_empty() {
            let mut path = Path::new();
            for span in &spans {
                path.rect(
                    x + span.start,
                    y,
                    span.end - span.start,
                    fragment.bounds.height,
                );
            }
            canvas.fill_path(&path, &highlight);
        }
        let link = fragment.link().is_some();
        let mut style = fragment.style();
        let tint = if !layout.enabled {
            theme.disabled_text
        } else {
            style.color.unwrap_or(if link { link_tint } else { tint })
        };
        style.underline |= link;
        let paint = paints
            .entry((style, tint))
            .or_insert_with(|| system.paint_styled(node, color(tint), style));
        fill_selected_text(
            canvas,
            bounds,
            Point { x, y: y + baseline },
            &fragment.text(document),
            paint,
            &spans,
            selected_color,
        )
        .map_err(|error| format!("styled text drawing failed: {error}"))?;
    }
    if let Some(index) = focused {
        let mut outline = Path::new();
        for (_, rect) in layout
            .text_link_regions()
            .filter(|(link, _)| *link == index)
        {
            let rect = rect.inset(0.75);
            outline.rect(rect.x, rect.y, rect.width, rect.height);
        }
        let mut paint = Paint::color(color(theme.focus));
        paint.set_line_width(1.5);
        canvas.stroke_path(&outline, &paint);
    }
    Ok(())
}
