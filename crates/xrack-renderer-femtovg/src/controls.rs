use crate::{
    TextSystem, color,
    image::Images,
    theme::{Palette, PaletteRole as Role},
};
use femtovg::{Align, Canvas, Paint, Path, renderer::WGPURenderer};
use xrack_layout::{
    CalendarGeometry, ChoiceMenu, ColorPlaneGeometry, ComboGeometry, LayoutNode, MenuPanel,
    PageIndicatorGeometry, Rect, SpinGeometry, TableGeometry, TextMeasurer,
};
use xrack_runtime::{Color, Node, Orientation, RowKey, TableHeader, combo_editable, combo_text};

fn fill(canvas: &mut Canvas<WGPURenderer>, rect: Rect, radius: f32, tint: Color) {
    let mut path = Path::new();
    path.rounded_rect(rect.x, rect.y, rect.width, rect.height, radius);
    canvas.fill_path(&path, &Paint::color(color(tint)));
}

pub(crate) fn color_preview(canvas: &mut Canvas<WGPURenderer>, bounds: Rect, tint: Color) {
    fill(canvas, bounds, 0.0, Color(0xe0e4ecff));
    let tile = Rect::new(0.0, 0.0, bounds.width / 8.0, bounds.height / 2.0);
    for row in 0..2 {
        for column in (row..8).step_by(2) {
            let rect = Rect::new(
                bounds.x + column as f32 * tile.width,
                bounds.y + row as f32 * tile.height,
                tile.width,
                tile.height,
            );
            fill(canvas, rect, 0.0, Color(0x8791a3ff));
        }
    }
    fill(canvas, bounds, 0.0, tint);
}

pub(crate) fn color_plane(canvas: &mut Canvas<WGPURenderer>, node: &Node, bounds: Rect) {
    let geometry = ColorPlaneGeometry::new(bounds);
    let field = geometry.field;
    if field.width <= 0.0 || field.height <= 0.0 {
        return;
    }
    let white = color(Color(0xffffffff));
    let black = color(Color(0x000000ff));
    let hue = color(node.color("hue-color").unwrap_or(Color(0xff0000ff)));
    let mut path = Path::new();
    path.rect(field.x, field.y, field.width, field.height);
    canvas.fill_path(
        &path,
        &Paint::linear_gradient(field.x, field.y, field.x + field.width, field.y, white, hue),
    );
    canvas.fill_path(
        &path,
        &Paint::linear_gradient(
            field.x,
            field.y,
            field.x,
            field.y + field.height,
            color(Color(0x00000000)),
            black,
        ),
    );

    let marker = geometry.point(
        node.number("saturation").unwrap_or_default(),
        node.number("brightness").unwrap_or_default(),
    );
    let mut outline = Path::new();
    outline.circle(marker.x, marker.y, 4.0);
    for (tint, width) in [(black, 4.0), (white, 2.0)] {
        let mut paint = Paint::color(tint);
        paint.set_line_width(width);
        canvas.stroke_path(&outline, &paint);
    }
}

pub(crate) fn calendar(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    node: &Node,
    geometry: &CalendarGeometry,
    enabled: bool,
) -> Result<(), String> {
    let draw = |canvas: &mut Canvas<WGPURenderer>, rect: Rect, label: &str, tint: Color| {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return Ok(());
        }
        let size = text.font_size(node).min((rect.height * 0.56).max(1.0));
        let paint = text.paint_node_at_size(node, size, color(tint), true);
        canvas.save();
        canvas.intersect_scissor(rect.x, rect.y, rect.width, rect.height);
        let result = canvas.fill_text(
            rect.x + rect.width * 0.5,
            rect.y + ((rect.height - size * 1.4) * 0.5).max(0.0),
            label,
            &paint,
        );
        canvas.restore();
        result
            .map(|_| ())
            .map_err(|error| format!("calendar label drawing failed: {error}"))
    };
    if geometry.has_header() {
        let month = geometry.state.month;
        draw(
            canvas,
            geometry.title(),
            &format!("{:04}-{:02}", month.year, month.month),
            theme.text,
        )?;
        for ((rect, action), label) in geometry.navigation().into_iter().zip(["‹", "›"]) {
            let tint = if enabled && action.is_some() {
                theme.text
            } else {
                theme.disabled_text
            };
            draw(canvas, rect, label, tint)?;
        }
    }
    for (rect, label) in geometry.weekdays(node) {
        draw(canvas, rect, label, theme.disabled_text)?;
    }
    for week in geometry.weeks() {
        draw(
            canvas,
            week.bounds,
            &week.number.to_string(),
            theme.disabled_text,
        )?;
    }
    if node.calendar_cells().is_some() {
        return Ok(());
    }
    for day in geometry.days() {
        if day.selected {
            let inset = 2.0_f32
                .min(day.bounds.width * 0.2)
                .min(day.bounds.height * 0.2);
            fill(
                canvas,
                day.bounds.inset(inset),
                6.0,
                if enabled {
                    theme.button
                } else {
                    theme.disabled
                },
            );
        } else if day.today && day.enabled && enabled {
            let rect = day.bounds.inset(
                3.0_f32
                    .min(day.bounds.width * 0.2)
                    .min(day.bounds.height * 0.2),
            );
            let mut outline = Path::new();
            outline.rounded_rect(rect.x, rect.y, rect.width, rect.height, 6.0);
            let mut paint = Paint::color(color(theme.focus));
            paint.set_line_width(1.5);
            canvas.stroke_path(&outline, &paint);
        }
        let tint = if enabled && day.enabled && (day.in_month || day.selected) {
            if day.selected {
                theme.on_accent(theme.text)
            } else {
                theme.text
            }
        } else {
            theme.disabled_text
        };
        draw(canvas, day.bounds, &day.date.day.to_string(), tint)?;
    }
    Ok(())
}

pub(crate) fn page_indicator(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Palette,
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
                node.color("color").unwrap_or(theme.role(Role::Border))
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

pub(crate) fn tumbler(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    node: &Node,
    wheel: &xrack_layout::TumblerGeometry,
    enabled: bool,
) -> Result<(), String> {
    if wheel.rows.is_empty() {
        return Ok(());
    }
    fill(
        canvas,
        wheel.center.inset(2.0),
        6.0,
        if enabled {
            theme.button
        } else {
            theme.disabled
        },
    );
    for row in &wheel.rows {
        let size = text
            .font_size(node)
            .min((row.bounds.height * 0.64).max(1.0));
        let mut tint = color(if enabled {
            let foreground = if row.displacement.abs() < 0.5 {
                theme.on_accent(theme.text)
            } else {
                theme.text
            };
            node.color("color").unwrap_or(foreground)
        } else {
            theme.disabled_text
        });
        tint.a *= (1.0 - 0.22 * row.displacement.abs()).max(0.38);
        let paint = text.paint_node_at_size(node, size, tint, true);
        canvas.save();
        canvas.intersect_scissor(
            row.bounds.x + 4.0,
            row.bounds.y,
            (row.bounds.width - 8.0).max(0.0),
            row.bounds.height,
        );
        let result = canvas.fill_text(
            row.bounds.x + row.bounds.width * 0.5,
            row.bounds.y + ((row.bounds.height - size * 1.4) * 0.5).max(0.0),
            &row.label,
            &paint,
        );
        canvas.restore();
        result.map_err(|error| format!("tumbler label drawing failed: {error}"))?;
    }
    Ok(())
}

pub(crate) fn split_handle(
    canvas: &mut Canvas<WGPURenderer>,
    node: &Node,
    bounds: Rect,
    theme: Palette,
    enabled: bool,
    highlighted: bool,
) {
    fill(
        canvas,
        bounds,
        0.0,
        if highlighted && enabled {
            theme.role(Role::HoverSurface)
        } else {
            theme.role(Role::InsetSurface)
        },
    );
    let tint = node.color("color").unwrap_or_else(|| {
        if highlighted && enabled {
            theme.focus
        } else {
            theme.role(Role::Divider)
        }
    });
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
    theme: Palette,
    node: &Node,
    bounds: Rect,
    enabled: bool,
    highlighted: bool,
) -> Result<(), String> {
    if highlighted && enabled {
        fill(
            canvas,
            bounds.inset(2.0),
            5.0,
            theme.role(Role::HoverSurface),
        );
    }
    let size = text.font_size(node);
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
            &text.paint_node_at_size(node, size, color(tint), false),
        )
        .map(|_| ())
        .map_err(|error| format!("menu header drawing failed: {error}"))
}

pub(crate) fn menu_panel(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    panel: MenuPanel<'_>,
    images: &mut Images,
    scale: f32,
) -> Result<(), String> {
    let bounds = panel.bounds;
    fill(canvas, bounds, 6.0, theme.role(Role::RaisedBorder));
    fill(
        canvas,
        bounds.inset(1.0),
        5.0,
        theme.role(Role::InsetSurface),
    );
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
                    theme.role(Role::StrongDivider),
                );
                continue;
            }
            if panel.row_highlighted(slot) && row.enabled {
                fill(canvas, rect.inset(2.0), 4.0, theme.role(Role::HoverSurface));
            }
            let tint = if row.enabled {
                theme.text
            } else {
                theme.disabled_text
            };
            let size = row
                .node
                .font_size()
                .or_else(|| text.default_font_size())
                .unwrap_or(15.0)
                .clamp(1.0, (rect.height / 1.4).max(1.0));
            let paint = text.paint_node_at_size(row.node, size, color(tint), false);
            if let Some(image) = row.icon {
                images.draw_icon(canvas, image, panel.icon_bounds(slot), scale, row.enabled)?;
            }
            let left = rect.x + panel.label_offset();
            let right = rect.x + rect.width - 12.0;
            let y = rect.y + (rect.height - size * 1.4).max(0.0) * 0.5;
            let mut label_end = right - if row.submenu { 10.0 } else { 0.0 };
            if let Some(shortcut) = &row.shortcut {
                let width = text
                    .advance_painted(shortcut, &paint)
                    .min((right - left).max(0.0) * 0.65);
                if width > 0.0 {
                    let mut shortcut_paint = paint.clone();
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
    theme: Palette,
    bounds: Rect,
    selected: bool,
    hovered: bool,
    current: bool,
) {
    if selected {
        fill(canvas, bounds, 0.0, theme.role(Role::RowSelection));
        fill(
            canvas,
            Rect::new(bounds.x, bounds.y, 3.0, bounds.height),
            0.0,
            theme.role(Role::SelectionOutline),
        );
    } else if hovered {
        fill(canvas, bounds, 0.0, theme.role(Role::RowHover));
    }
    if current {
        let mut outline = Path::new();
        let inset = bounds.inset(1.0);
        outline.rect(inset.x, inset.y, inset.width, inset.height);
        let mut paint = Paint::color(color(theme.role(Role::SelectionOutline)));
        paint.set_line_width(1.0);
        canvas.stroke_path(&outline, &paint);
    }
}

pub(crate) fn tree_indicator(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Palette,
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
        theme.role(Role::Disclosure)
    } else {
        theme.role(Role::DisabledDisclosure)
    };
    canvas.fill_path(&arrow, &Paint::color(color(tint)));
}

pub(crate) fn table_header(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    header: &TableHeader,
    geometry: &TableGeometry,
    clip: Rect,
    highlights: impl Fn(usize) -> (bool, bool, bool),
) -> Result<(), String> {
    fill(
        canvas,
        geometry.header.intersect(clip),
        0.0,
        theme.role(Role::TableHeader),
    );
    for (index, (column, bounds)) in header.columns.iter().zip(&geometry.columns).enumerate() {
        let visible = bounds.intersect(geometry.column_clip(index, clip));
        if visible.width <= 0.0 || visible.height <= 0.0 {
            continue;
        }
        let (highlighted, resizing, selected) = highlights(index);
        if selected {
            fill(canvas, visible, 0.0, theme.role(Role::SelectedColumn));
        }
        if highlighted {
            fill(
                canvas,
                visible,
                0.0,
                if selected {
                    theme.role(Role::ActiveColumn)
                } else {
                    theme.role(Role::HoverSurface)
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
            let size = text
                .default_font_size()
                .unwrap_or(15.0)
                .clamp(1.0, (bounds.height / 1.4).max(1.0));
            let result = canvas.fill_text(
                bounds.x + 8.0,
                bounds.y + (bounds.height - size * 1.4).max(0.0) * 0.5,
                column.text("title").unwrap_or(""),
                &text.paint(size, color(theme.text), false),
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
                    theme.role(Role::Divider)
                },
            );
        }
    }
    Ok(())
}

pub(crate) fn table_row_headers(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    node: &Node,
    layout: &LayoutNode,
    highlights: impl Fn(usize, Option<RowKey>) -> (bool, bool),
) -> Result<(), String> {
    let Some(table) = &layout.table else {
        return Ok(());
    };
    let gutter = table.row_header;
    let corner = Rect::new(gutter.x, table.header.y, gutter.width, table.header.height);
    let background = theme.role(Role::TableHeader);
    fill(canvas, corner.intersect(layout.clip), 0.0, background);
    let clip = gutter.intersect(layout.clip);
    fill(canvas, clip, 0.0, background);
    for (slot, (row, item)) in node.children.iter().zip(&layout.children).enumerate() {
        let Some(label) = row.text("row-header-text") else {
            continue;
        };
        let bounds = table.row_header_cell(item.bounds);
        let visible = bounds.intersect(clip);
        if !item.visible || visible.width <= 0.0 || visible.height <= 0.0 {
            continue;
        }
        let (hovered, resizing) = highlights(slot, row.row_key);
        let selected = row
            .row_key
            .zip(node.table_selection.as_deref())
            .is_some_and(|(key, selection)| selection.row_header_selected(key));
        if hovered || selected {
            let role = match (hovered, selected) {
                (true, true) => Role::ActiveColumn,
                (false, true) => Role::SelectedColumn,
                _ => Role::HoverSurface,
            };
            fill(canvas, visible, 0.0, theme.role(role));
        }
        let size = text
            .default_font_size()
            .unwrap_or(15.0)
            .clamp(1.0, (bounds.height / 1.4).max(1.0));
        let label_clip = Rect::new(
            bounds.x + 6.0,
            bounds.y,
            (bounds.width - 12.0).max(0.0),
            bounds.height,
        )
        .intersect(visible);
        if label_clip.width > 0.0 {
            canvas.save();
            canvas.intersect_scissor(
                label_clip.x,
                label_clip.y,
                label_clip.width,
                label_clip.height,
            );
            let tint = if item.enabled {
                theme.text
            } else {
                theme.disabled_text
            };
            let result = canvas.fill_text(
                bounds.x + 6.0,
                bounds.y + (bounds.height - size * 1.4).max(0.0) * 0.5,
                label,
                &text.paint(size, color(tint), false),
            );
            canvas.restore();
            result.map_err(|error| format!("table row header drawing failed: {error}"))?;
        }
        let thickness = if resizing { 2.0 } else { 1.0 };
        let line = Rect::new(
            bounds.x,
            bounds.y + bounds.height - thickness,
            bounds.width,
            thickness,
        )
        .intersect(visible);
        fill(
            canvas,
            line,
            0.0,
            if resizing {
                theme.focus
            } else {
                theme.role(Role::Divider)
            },
        );
    }
    let separator = Rect::new(
        gutter.x + gutter.width - 1.0,
        table.header.y,
        1.0,
        table.header.height + gutter.height,
    );
    fill(
        canvas,
        separator.intersect(layout.clip),
        0.0,
        theme.role(Role::Divider),
    );
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
    theme: Palette,
    editable: bool,
) {
    let foreground = if editable {
        theme.text
    } else {
        theme.disabled_text
    };
    for (rect, up) in [(geometry.up, true), (geometry.down, false)] {
        fill(canvas, rect, 0.0, theme.role(Role::Control));
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
    theme: Palette,
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
    let trigger = ComboGeometry::new(bounds).trigger;
    chevron(
        canvas,
        trigger.x + trigger.width * 0.5,
        bounds.y + bounds.height * 0.5,
        4.0,
        open,
        foreground,
    );
    let label = combo_text(node);
    if label.is_empty() || combo_editable(node) {
        return Ok(());
    }
    let size = text.font_size(node);
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
        label,
        &text.paint_node_at_size(node, size, color(foreground), false),
    );
    canvas.restore();
    result
        .map(|_| ())
        .map_err(|err| format!("combobox drawing failed: {err}"))
}

pub(crate) fn menu(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    menu: ChoiceMenu<'_>,
) -> Result<(), String> {
    fill(canvas, menu.bounds, 6.0, theme.role(Role::Surface));
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
            if Some(index) == menu.highlighted {
                fill(
                    canvas,
                    row.inset(2.0),
                    4.0,
                    theme.role(Role::SelectedSurface),
                );
            } else if menu.selected == Some(index) {
                fill(
                    canvas,
                    row.inset(2.0),
                    4.0,
                    theme.role(Role::InactiveSelection),
                );
            }
            let size = menu
                .control
                .font_size()
                .or_else(|| text.default_font_size())
                .unwrap_or(14.0)
                .clamp(1.0, (row.height / 1.4).max(1.0));
            let foreground = if menu.row_enabled(offset) {
                theme.text
            } else {
                theme.disabled_text
            };
            canvas
                .fill_text(
                    row.x + 10.0,
                    row.y + ((row.height - size * 1.4) * 0.5).max(0.0),
                    label,
                    &text.paint_node_at_size(menu.control, size, color(foreground), false),
                )
                .map_err(|err| format!("combobox popup drawing failed: {err}"))?;
        }
        Ok(())
    })();
    canvas.restore();
    result
}

pub(crate) fn label(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    node: &Node,
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
        &text.paint_node_at_size(node, size, color(tint), false),
    );
    canvas.restore();
    result
        .map(|_| ())
        .map_err(|error| format!("selection label drawing failed: {error}"))
}

pub(crate) fn search_icons(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Palette,
    node: &Node,
    geometry: &xrack_layout::SearchGeometry,
    enabled: bool,
) {
    let mut paint = Paint::color(color(if enabled {
        theme.text
    } else {
        theme.disabled_text
    }));
    paint.set_line_width(1.5);
    for (rect, action) in geometry.actions(node) {
        let x = rect.x + rect.width * 0.5;
        let y = rect.y + rect.height * 0.5;
        let radius = 5.0_f32.min(rect.width * 0.2).min(rect.height * 0.2);
        if radius <= 0.0 {
            continue;
        }
        let mut icon = Path::new();
        if action == xrack_runtime::ControlAction::ClearSearch {
            icon.move_to(x - radius, y - radius);
            icon.line_to(x + radius, y + radius);
            icon.move_to(x - radius, y + radius);
            icon.line_to(x + radius, y - radius);
        } else {
            let lens = radius * 0.75;
            icon.circle(x - lens * 0.35, y - lens * 0.35, lens);
            icon.move_to(x + lens * 0.35, y + lens * 0.35);
            icon.line_to(x + radius, y + radius);
        }
        canvas.stroke_path(&icon, &paint);
    }
}

pub(crate) fn radio(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    node: &Node,
    bounds: Rect,
    enabled: bool,
) -> Result<(), String> {
    let radius = 8.0_f32.min(bounds.height * 0.5);
    let item_delegate = node.is_item_delegate();
    let x = if item_delegate {
        bounds.x + (bounds.width - 12.0 - radius).max(radius)
    } else {
        bounds.x + 6.0 + radius
    };
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
    let label_bounds = if item_delegate {
        Rect::new(
            bounds.x + 12.0,
            bounds.y,
            (x - radius - bounds.x - 22.0).max(0.0),
            bounds.height,
        )
    } else {
        Rect::new(
            bounds.x + 28.0,
            bounds.y,
            (bounds.width - 28.0).max(0.0),
            bounds.height,
        )
    };
    label(
        canvas,
        text,
        node,
        label_bounds,
        node.text("text").unwrap_or(""),
        text.font_size(node),
        foreground,
    )
}

pub(crate) fn group_title(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
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
    let size = text.font_size(node);
    label(
        canvas,
        text,
        node,
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
    theme: Palette,
    bounds: Rect,
    title: &str,
    node: &Node,
    appearance: TabAppearance,
) -> Result<(), String> {
    let base = if !appearance.enabled {
        theme.disabled
    } else if appearance.selected {
        theme.role(Role::SelectedSurface)
    } else {
        theme.role(Role::Surface)
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
        node,
        Rect::new(
            bounds.x + 10.0,
            bounds.y,
            (bounds.width - 20.0).max(0.0),
            bounds.height,
        ),
        title,
        text.default_font_size()
            .unwrap_or(xrack_layout::TAB_LABEL_SIZE)
            .clamp(1.0, (bounds.height / 1.4).max(1.0)),
        if appearance.enabled {
            theme.text
        } else {
            theme.disabled_text
        },
    )
}
