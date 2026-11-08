use crate::{TextSystem, Theme, color};
use femtovg::{Align, Canvas, Paint, Path, renderer::WGPURenderer};
use xrack_layout::{
    ChoiceMenu, MenuPanel, PageIndicatorGeometry, Rect, SpinGeometry, TableGeometry, font_size,
};
use xrack_runtime::{Color, Node, Orientation, TableHeader, combo_index};

fn fill(canvas: &mut Canvas<WGPURenderer>, rect: Rect, radius: f32, tint: Color) {
    let mut path = Path::new();
    path.rounded_rect(rect.x, rect.y, rect.width, rect.height, radius);
    canvas.fill_path(&path, &Paint::color(color(tint)));
}

pub(crate) fn page_indicator(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Theme,
    node: &Node,
    geometry: &PageIndicatorGeometry,
    enabled: bool,
    hovered: impl Fn(usize) -> bool,
) {
    let mut ordinary = Path::new();
    let mut selected = Path::new();
    let mut highlight = Path::new();
    for marker in geometry.markers() {
        let path = if marker.selected {
            &mut selected
        } else if enabled && hovered(marker.index) {
            &mut highlight
        } else {
            &mut ordinary
        };
        path.circle(marker.center.x, marker.center.y, marker.diameter * 0.5);
    }
    for (path, tint) in [
        (
            ordinary,
            if enabled {
                node.color("color").unwrap_or(Color(0x75859aff))
            } else {
                theme.disabled
            },
        ),
        (
            selected,
            if enabled {
                node.color("current-color").unwrap_or(theme.button)
            } else {
                theme.disabled_text
            },
        ),
        (highlight, theme.text),
    ] {
        if !path.is_empty() {
            canvas.fill_path(&path, &Paint::color(color(tint)));
        }
    }
}

pub(crate) fn split_handle(
    canvas: &mut Canvas<WGPURenderer>,
    node: &Node,
    bounds: Rect,
    theme: Theme,
    enabled: bool,
    highlighted: bool,
) {
    fill(
        canvas,
        bounds,
        0.0,
        if highlighted && enabled {
            Color(0x405582ff)
        } else {
            Color(0x25334bff)
        },
    );
    let tint = if highlighted && enabled {
        theme.focus
    } else {
        node.color("color").unwrap_or(Color(0x637ca4ff))
    };
    let width = bounds.width.min(2.0);
    let height = bounds.height.min(2.0);
    let strip = match node.orientation() {
        Orientation::Horizontal => Rect::new(
            bounds.x + (bounds.width - width) * 0.5,
            bounds.y,
            width,
            bounds.height,
        ),
        Orientation::Vertical => Rect::new(
            bounds.x,
            bounds.y + (bounds.height - height) * 0.5,
            bounds.width,
            height,
        ),
    };
    fill(canvas, strip, 0.0, tint);
}

pub(crate) fn menu_header(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    enabled: bool,
    highlighted: bool,
) -> Result<(), String> {
    if highlighted && enabled {
        fill(canvas, bounds.inset(2.0), 5.0, Color(0x405582ff));
    }
    let size = font_size(node);
    let tint = if enabled {
        theme.text
    } else {
        theme.disabled_text
    };
    canvas
        .fill_text(
            bounds.x + 12.0,
            bounds.y + (bounds.height - size * 1.4).max(0.0) * 0.5,
            node.text("title").unwrap_or(""),
            &text.paint(size, color(tint), false),
        )
        .map(|_| ())
        .map_err(|error| format!("menu header drawing failed: {error}"))
}

pub(crate) fn menu_panel(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Theme,
    panel: MenuPanel<'_>,
) -> Result<(), String> {
    let bounds = panel.bounds;
    fill(canvas, bounds, 6.0, Color(0x627596ff));
    fill(canvas, bounds.inset(1.0), 5.0, Color(0x25334bff));
    canvas.save();
    canvas.scissor(bounds.x, bounds.y, bounds.width, bounds.height);
    let result = (|| {
        for (slot, row) in panel.rows.iter().enumerate() {
            let rect = panel.row_bounds(slot);
            let center = rect.y + rect.height * 0.5;
            if row.separator {
                fill(
                    canvas,
                    Rect::new(rect.x + 8.0, center, (rect.width - 16.0).max(0.0), 1.0),
                    0.0,
                    Color(0x506483ff),
                );
                continue;
            }
            if panel.row_highlighted(slot) && row.enabled {
                fill(canvas, rect.inset(2.0), 4.0, Color(0x405582ff));
            }
            let tint = if row.enabled {
                theme.text
            } else {
                theme.disabled_text
            };
            let paint = text.paint(15.0, color(tint), false);
            let left = rect.x + 26.0;
            let right = rect.x + rect.width - 12.0;
            let y = rect.y + (rect.height - 21.0).max(0.0) * 0.5;
            let mut label_end = right - if row.submenu { 10.0 } else { 0.0 };
            if let Some(shortcut) = &row.shortcut {
                let width = text
                    .advance(shortcut, 15.0)
                    .min((right - left).max(0.0) * 0.65);
                if width > 0.0 {
                    let mut shortcut_paint = text.paint(15.0, color(tint), false);
                    shortcut_paint.set_text_align(Align::Right);
                    canvas.save();
                    canvas.intersect_scissor(right - width, rect.y, width, rect.height);
                    let result = canvas.fill_text(right, y, shortcut, &shortcut_paint);
                    canvas.restore();
                    result.map_err(|error| format!("menu shortcut drawing failed: {error}"))?;
                    label_end = right - width - 12.0;
                }
            }
            if label_end > left {
                canvas.save();
                canvas.intersect_scissor(left, rect.y, label_end - left, rect.height);
                let result = canvas.fill_text(left, y, row.label, &paint);
                canvas.restore();
                result.map_err(|error| format!("menu item drawing failed: {error}"))?;
            }
            let mut mark = Path::new();
            if row.checked == Some(true) {
                mark.move_to(rect.x + 9.0, center);
                mark.line_to(rect.x + 13.0, center + 4.0);
                mark.line_to(rect.x + 19.0, center - 4.0);
            } else if row.submenu {
                let x = rect.x + rect.width - 13.0;
                mark.move_to(x - 3.0, center - 4.0);
                mark.line_to(x + 1.0, center);
                mark.line_to(x - 3.0, center + 4.0);
            } else {
                continue;
            }
            let mut paint = Paint::color(color(tint));
            paint.set_line_width(1.6);
            canvas.stroke_path(&mark, &paint);
        }
        Ok(())
    })();
    canvas.restore();
    result
}

pub(crate) fn list_row(
    canvas: &mut Canvas<WGPURenderer>,
    bounds: Rect,
    selected: bool,
    hovered: bool,
    current: bool,
) {
    if selected {
        fill(canvas, bounds, 0.0, Color(0x768dff38));
        fill(
            canvas,
            Rect::new(bounds.x, bounds.y, 3.0, bounds.height),
            0.0,
            Color(0x9aadffff),
        );
    } else if hovered {
        fill(canvas, bounds, 0.0, Color(0xb3c3ff24));
    }
    if current {
        let mut outline = Path::new();
        let inset = bounds.inset(1.0);
        outline.rect(inset.x, inset.y, inset.width, inset.height);
        let mut paint = Paint::color(color(Color(0x9aadffff)));
        paint.set_line_width(1.0);
        canvas.stroke_path(&outline, &paint);
    }
}

pub(crate) fn tree_indicator(
    canvas: &mut Canvas<WGPURenderer>,
    bounds: Rect,
    expanded: bool,
    enabled: bool,
) {
    let x = bounds.x + bounds.width * 0.5;
    let y = bounds.y + bounds.height * 0.5;
    let extent = 5.0_f32.min(bounds.width * 0.5).min(bounds.height * 0.5);
    let mut arrow = Path::new();
    if expanded {
        arrow.move_to(x - extent, y - extent * 0.5);
        arrow.line_to(x + extent, y - extent * 0.5);
        arrow.line_to(x, y + extent * 0.5);
    } else {
        arrow.move_to(x - extent * 0.5, y - extent);
        arrow.line_to(x - extent * 0.5, y + extent);
        arrow.line_to(x + extent * 0.5, y);
    }
    arrow.close();
    let tint = if enabled {
        Color(0xc0d1eaff)
    } else {
        Color(0x687991ff)
    };
    canvas.fill_path(&arrow, &Paint::color(color(tint)));
}

pub(crate) fn table_header(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Theme,
    header: &TableHeader,
    geometry: &TableGeometry,
    clip: Rect,
    highlights: impl Fn(usize) -> (bool, bool, bool),
) -> Result<(), String> {
    fill(
        canvas,
        geometry.header.intersect(clip),
        0.0,
        Color(0x314363ff),
    );
    for (index, (column, bounds)) in header.columns.iter().zip(&geometry.columns).enumerate() {
        let visible = bounds.intersect(clip);
        if visible.width <= 0.0 || visible.height <= 0.0 {
            continue;
        }
        let (highlighted, resizing, selected) = highlights(index);
        if selected {
            fill(canvas, visible, 0.0, Color(0x455a89ff));
        }
        if highlighted {
            fill(
                canvas,
                visible,
                0.0,
                if selected {
                    Color(0x526899ff)
                } else {
                    Color(0x405582ff)
                },
            );
        }
        let sort = header.sort.filter(|sort| sort.column == index);
        let title_width = (bounds.width - if sort.is_some() { 32.0 } else { 16.0 }).max(0.0);
        let title =
            Rect::new(bounds.x + 8.0, bounds.y, title_width, bounds.height).intersect(visible);
        if title.width > 0.0 && title.height > 0.0 {
            canvas.save();
            canvas.intersect_scissor(title.x, title.y, title.width, title.height);
            let result = canvas.fill_text(
                bounds.x + 8.0,
                bounds.y + (bounds.height - 21.0).max(0.0) * 0.5,
                column.text("title").unwrap_or(""),
                &text.paint(15.0, color(theme.text), false),
            );
            canvas.restore();
            result.map_err(|error| format!("table header drawing failed: {error}"))?;
        }
        if let Some(sort) = sort
            && bounds.width >= 24.0
        {
            canvas.save();
            canvas.intersect_scissor(visible.x, visible.y, visible.width, visible.height);
            chevron(
                canvas,
                bounds.x + bounds.width - 12.0,
                bounds.y + bounds.height * 0.5,
                4.0,
                !sort.descending,
                theme.text,
            );
            canvas.restore();
        }
        let thickness = if resizing { 2.0 } else { 1.0 };
        let separator = Rect::new(
            bounds.x + bounds.width - thickness,
            bounds.y,
            thickness,
            bounds.height,
        );
        let separator = separator.intersect(visible);
        if separator.width > 0.0 {
            fill(
                canvas,
                separator,
                0.0,
                if resizing {
                    theme.focus
                } else {
                    Color(0x637ca4ff)
                },
            );
        }
    }
    Ok(())
}

fn chevron(canvas: &mut Canvas<WGPURenderer>, x: f32, y: f32, size: f32, up: bool, tint: Color) {
    let direction = if up { -1.0 } else { 1.0 };
    let mut path = Path::new();
    path.move_to(x - size, y - direction * size * 0.5);
    path.line_to(x, y + direction * size * 0.5);
    path.line_to(x + size, y - direction * size * 0.5);
    let mut paint = Paint::color(color(tint));
    paint.set_line_width(1.5);
    canvas.stroke_path(&path, &paint);
}

pub(crate) fn spin_buttons(
    canvas: &mut Canvas<WGPURenderer>,
    geometry: &SpinGeometry,
    theme: Theme,
    editable: bool,
) {
    let foreground = if editable {
        theme.text
    } else {
        theme.disabled_text
    };
    for (rect, up) in [(geometry.up, true), (geometry.down, false)] {
        fill(canvas, rect, 0.0, Color(0x384761ff));
        chevron(
            canvas,
            rect.x + rect.width * 0.5,
            rect.y + rect.height * 0.5,
            4.0,
            up,
            foreground,
        );
    }
}

pub(crate) fn combo(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    enabled: bool,
    open: bool,
) -> Result<(), String> {
    let foreground = if enabled {
        node.color("color").unwrap_or(theme.text)
    } else {
        theme.disabled_text
    };
    chevron(
        canvas,
        bounds.x + bounds.width - 14.0,
        bounds.y + bounds.height * 0.5,
        4.0,
        open,
        foreground,
    );
    let Some(index) = combo_index(node) else {
        return Ok(());
    };
    let size = font_size(node);
    canvas.save();
    canvas.intersect_scissor(
        bounds.x + 8.0,
        bounds.y,
        (bounds.width - 36.0).max(0.0),
        bounds.height,
    );
    let result = canvas.fill_text(
        bounds.x + 9.0,
        bounds.y + ((bounds.height - size * 1.4) * 0.5).max(0.0),
        &node.strings("model")[index],
        &text.paint(size, color(foreground), false),
    );
    canvas.restore();
    result
        .map(|_| ())
        .map_err(|err| format!("combobox drawing failed: {err}"))
}

pub(crate) fn menu(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Theme,
    menu: ChoiceMenu<'_>,
) -> Result<(), String> {
    fill(canvas, menu.bounds, 6.0, Color(0x27354dff));
    canvas.save();
    canvas.scissor(
        menu.bounds.x,
        menu.bounds.y,
        menu.bounds.width,
        menu.bounds.height,
    );
    let result = (|| {
        for (offset, label) in menu.rows.iter().enumerate() {
            let row = menu.row_bounds(offset);
            let index = menu.first + offset;
            if index == menu.highlighted {
                fill(canvas, row.inset(2.0), 4.0, Color(0x435e99ff));
            } else if menu.selected == Some(index) {
                fill(canvas, row.inset(2.0), 4.0, Color(0x35466dff));
            }
            canvas
                .fill_text(
                    row.x + 10.0,
                    row.y + ((row.height - 18.0) * 0.5).max(0.0),
                    label,
                    &text.paint(14.0, color(theme.text), false),
                )
                .map_err(|err| format!("combobox popup drawing failed: {err}"))?;
        }
        Ok(())
    })();
    canvas.restore();
    result
}

fn label(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    bounds: Rect,
    value: &str,
    size: f32,
    tint: Color,
) -> Result<(), String> {
    if bounds.width <= 0.0 || bounds.height <= 0.0 || value.is_empty() {
        return Ok(());
    }
    canvas.save();
    canvas.intersect_scissor(bounds.x, bounds.y, bounds.width, bounds.height);
    let result = canvas.fill_text(
        bounds.x,
        bounds.y + ((bounds.height - size * 1.4) * 0.5).max(0.0),
        value,
        &text.paint(size, color(tint), false),
    );
    canvas.restore();
    result
        .map(|_| ())
        .map_err(|error| format!("selection label drawing failed: {error}"))
}

pub(crate) fn radio(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    enabled: bool,
) -> Result<(), String> {
    let radius = 8.0_f32.min(bounds.height * 0.5);
    let x = bounds.x + 6.0 + radius;
    let y = bounds.y + bounds.height * 0.5;
    let foreground = if enabled {
        node.color("color").unwrap_or(theme.text)
    } else {
        theme.disabled_text
    };
    let mut circle = Path::new();
    circle.circle(x, y, radius);
    let mut outline = Paint::color(color(if enabled {
        theme.focus
    } else {
        theme.disabled_text
    }));
    outline.set_line_width(1.4);
    canvas.stroke_path(&circle, &outline);
    if node.boolean("checked", false) {
        let mut dot = Path::new();
        dot.circle(x, y, radius * 0.55);
        canvas.fill_path(
            &dot,
            &Paint::color(color(if enabled {
                theme.button
            } else {
                theme.disabled_text
            })),
        );
    }
    label(
        canvas,
        text,
        Rect::new(
            bounds.x + 28.0,
            bounds.y,
            (bounds.width - 28.0).max(0.0),
            bounds.height,
        ),
        node.text("text").unwrap_or(""),
        font_size(node),
        foreground,
    )
}

pub(crate) fn group_title(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Theme,
    node: &Node,
    bounds: Rect,
) -> Result<(), String> {
    let left = node
        .length("padding-left")
        .or_else(|| node.length("padding"))
        .unwrap_or(0.0)
        .max(0.0);
    let top = node
        .length("padding-top")
        .or_else(|| node.length("padding"))
        .unwrap_or(0.0)
        .max(0.0);
    let size = font_size(node);
    label(
        canvas,
        text,
        Rect::new(
            bounds.x + left,
            bounds.y + top,
            (bounds.width - left).max(0.0),
            size * 1.4,
        ),
        node.text("title").unwrap_or(""),
        size,
        theme.text,
    )
}

pub(crate) struct TabAppearance {
    pub selected: bool,
    pub enabled: bool,
    pub focused: bool,
}

pub(crate) fn tab(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Theme,
    bounds: Rect,
    title: &str,
    appearance: TabAppearance,
) -> Result<(), String> {
    let base = if !appearance.enabled {
        theme.disabled
    } else if appearance.selected {
        Color(0x435e99ff)
    } else {
        Color(0x27354dff)
    };
    let surface = bounds.inset(2.0);
    fill(canvas, surface, 6.0, base);
    if appearance.focused {
        let mut path = Path::new();
        path.rounded_rect(surface.x, surface.y, surface.width, surface.height, 6.0);
        let mut paint = Paint::color(color(theme.focus));
        paint.set_line_width(1.5);
        canvas.stroke_path(&path, &paint);
    }
    label(
        canvas,
        text,
        Rect::new(
            bounds.x + 10.0,
            bounds.y,
            (bounds.width - 20.0).max(0.0),
            bounds.height,
        ),
        title,
        14.0,
        if appearance.enabled {
            theme.text
        } else {
            theme.disabled_text
        },
    )
}
