use crate::{
    TextSystem, brush_paint, color,
    image::Images,
    theme::{Palette, PaletteRole as Role},
};
use femtovg::{Align, Canvas, Paint, Path, renderer::WGPURenderer};
use xrack_layout::{
    CalendarGeometry, ChoiceMenu, ColorPlaneGeometry, ComboGeometry, LayoutNode, MenuPanel,
    PageIndicatorGeometry, Rect, SpinGeometry, TableGeometry, TextMeasurer,
};
use xrack_runtime::{
    Color, LayoutDirection, Node, Orientation, RowKey, TableHeader, combo_editable, combo_text,
};

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

fn picker_label_size(
    text: &TextSystem,
    node: &Node,
    rect: Rect,
    label: &str,
    height_ratio: f32,
) -> f32 {
    let mut size = text
        .font_size(node)
        .min((rect.height * height_ratio).max(1.0));
    if text.text_scale() > 1.0 {
        let width = (rect.width - 4.0).max(1.0);
        let paint = text.paint_node_at_size(node, size, femtovg::Color::white(), false);
        let advance = text.advance_painted(label, &paint);
        if advance > width {
            size = (size * width / advance).max(1.0);
        }
    }
    size
}

fn draw_picker_label(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    node: &Node,
    rect: Rect,
    label: &str,
    tint: femtovg::Color,
    size: f32,
) -> Result<(), String> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Ok(());
    }
    let paint = text.paint_node_at_size(node, size, tint, true);
    canvas.save();
    canvas.intersect_scissor(rect.x, rect.y, rect.width, rect.height);
    let result = text.fill_text(
        canvas,
        rect.x + rect.width * 0.5,
        rect.y + ((rect.height - size * 1.4) * 0.5).max(0.0),
        label,
        &paint,
    );
    canvas.restore();
    result
        .map(|_| ())
        .map_err(|error| format!("picker label drawing failed: {error}"))
}

pub(crate) fn calendar(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    node: &Node,
    geometry: &CalendarGeometry,
    enabled: bool,
) -> Result<(), String> {
    let draw = |canvas: &mut Canvas<WGPURenderer>, rect, label: &str, tint| {
        let size = picker_label_size(text, node, rect, label, 0.56);
        draw_picker_label(canvas, text, node, rect, label, color(tint), size)
    };
    if geometry.has_header() {
        let month = geometry.state.month;
        draw(
            canvas,
            geometry.title(),
            &format!("{:04}-{:02}", month.year, month.month),
            theme.text,
        )?;
        let arrows = if geometry.layout_direction == LayoutDirection::RightToLeft {
            ["›", "‹"]
        } else {
            ["‹", "›"]
        };
        for ((rect, action), label) in geometry.navigation().into_iter().zip(arrows) {
            let tint = if enabled && action.is_some() {
                theme.text
            } else {
                theme.disabled_text
            };
            draw(canvas, rect, label, tint)?;
        }
    }
    let weekday_size = (text.text_scale() > 1.0).then(|| {
        geometry
            .weekdays(node)
            .map(|(rect, label)| picker_label_size(text, node, rect, label, 0.56))
            .fold(f32::INFINITY, f32::min)
    });
    for (rect, label) in geometry.weekdays(node) {
        let size = weekday_size.unwrap_or_else(|| picker_label_size(text, node, rect, label, 0.56));
        draw_picker_label(
            canvas,
            text,
            node,
            rect,
            label,
            color(theme.disabled_text),
            size,
        )?;
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
        let endpoint = day.highlight.start || day.highlight.end;
        if day.highlight.in_range && enabled {
            let bounds = day.bounds;
            let inset = 3.0_f32.min(bounds.height * 0.25);
            fill(
                canvas,
                Rect::new(
                    bounds.x,
                    bounds.y + inset,
                    bounds.width,
                    bounds.height - 2.0 * inset,
                ),
                0.0,
                theme.text_selection(node),
            );
        }
        if day.selected || endpoint {
            let inset = 2.0_f32
                .min(day.bounds.width * 0.2)
                .min(day.bounds.height * 0.2);
            fill(
                canvas,
                day.bounds.inset(inset),
                6.0,
                if enabled && (day.enabled || day.selected) {
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
        let tint = if enabled && day.enabled && (day.in_month || day.selected || endpoint) {
            if day.selected || endpoint {
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

pub(crate) fn tumbler_background(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Palette,
    wheel: &xrack_layout::TumblerGeometry,
    enabled: bool,
) {
    if wheel.rows.is_empty() {
        return;
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
}

pub(crate) fn tumbler_row(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    node: &Node,
    row: &xrack_layout::TumblerRow,
    enabled: bool,
) -> Result<(), String> {
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
    let bounds = Rect::new(
        row.bounds.x + 4.0,
        row.bounds.y,
        (row.bounds.width - 8.0).max(0.0),
        row.bounds.height,
    );
    let size = picker_label_size(text, node, bounds, &row.label, 0.64);
    draw_picker_label(canvas, text, node, bounds, &row.label, tint, size)
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
    layout: &LayoutNode,
    highlighted: bool,
) -> Result<(), String> {
    let bounds = layout.bounds;
    let enabled = layout.enabled;
    if highlighted && enabled {
        fill(
            canvas,
            bounds.inset(2.0),
            5.0,
            theme.role(Role::HoverSurface),
        );
    }
    let tint = if enabled {
        theme.text
    } else {
        theme.disabled_text
    };
    row_label(
        canvas,
        text,
        node,
        Rect::new(
            bounds.x + 12.0,
            bounds.y,
            (bounds.width - 24.0).max(0.0),
            bounds.height,
        ),
        node.text("title").unwrap_or(""),
        tint,
        layout.layout_direction,
    )
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
    let rtl = panel.layout_direction == LayoutDirection::RightToLeft;
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
            let size = row.font_size.min((rect.height / 1.4).max(1.0));
            let paint = text.paint_node_at_size(row.node, size, color(tint), false);
            if let Some(image) = &row.icon {
                images.draw_icon(
                    canvas,
                    image,
                    panel.icon_bounds(slot),
                    scale,
                    row.enabled,
                    None,
                )?;
            }
            let label_start = panel.label_offset();
            let trailing = rect.width - 12.0;
            let mut label_end = trailing - if row.submenu { 10.0 } else { 0.0 };
            if let Some(shortcut) = &row.shortcut {
                let width = text.advance_painted(shortcut, &paint);
                let label_width = text.advance_painted(row.label, &paint);
                if width > 0.0 && label_width + width + 12.0 <= label_end - label_start {
                    let area = panel.row_content_bounds(slot, trailing - width, width);
                    let mut shortcut_paint = paint.clone();
                    let x = if rtl {
                        area.x
                    } else {
                        shortcut_paint.set_text_align(Align::Right);
                        area.x + area.width
                    };
                    draw_label(canvas, text, area, shortcut, size, x, &shortcut_paint)?;
                    label_end = trailing - width - 12.0;
                }
            }
            let area = panel.row_content_bounds(slot, label_start, label_end - label_start);
            let mut label_paint = paint;
            let x = if rtl {
                label_paint.set_text_align(Align::Right);
                area.x + area.width
            } else {
                area.x
            };
            let label = xrack_layout::elide_to_width(row.label, area.width, |value| {
                text.advance_painted(value, &label_paint)
            });
            draw_label(canvas, text, area, &label, size, x, &label_paint)?;
            let mut mark = Path::new();
            if row.checked == Some(true) {
                let start = panel.row_content_bounds(slot, 9.0, 10.0).x;
                mark.move_to(start, center);
                mark.line_to(start + 4.0, center + 4.0);
                mark.line_to(start + 10.0, center - 4.0);
            } else if row.submenu {
                let x = panel.row_content_bounds(slot, rect.width - 13.0, 0.0).x;
                let direction = if rtl { -1.0 } else { 1.0 };
                mark.move_to(x - 3.0 * direction, center - 4.0);
                mark.line_to(x + direction, center);
                mark.line_to(x - 3.0 * direction, center + 4.0);
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

pub(crate) fn list_section_header(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    node: &Node,
    header: &xrack_layout::ListSectionHeader,
    direction: LayoutDirection,
) -> Result<(), String> {
    fill(canvas, header.bounds, 0.0, theme.role(Role::InsetSurface));
    let mut label = header.bounds;
    let inset = 10.0_f32.min(label.width * 0.5);
    label.x += inset;
    label.width -= inset * 2.0;
    leading_label(
        canvas,
        text,
        node,
        label,
        &header.label,
        theme.text,
        direction,
    )
}

pub(crate) fn list_row(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Palette,
    bounds: Rect,
    direction: LayoutDirection,
    selected: bool,
    hovered: bool,
    current: bool,
) {
    if selected {
        fill(canvas, bounds, 0.0, theme.role(Role::RowSelection));
        let width = bounds.width.min(3.0);
        let x = if direction == LayoutDirection::RightToLeft {
            bounds.x + bounds.width - width
        } else {
            bounds.x
        };
        fill(
            canvas,
            Rect::new(x, bounds.y, width, bounds.height),
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
    direction: LayoutDirection,
    expanded: bool,
    enabled: bool,
    text_scale: f32,
) {
    let x = bounds.x + bounds.width * 0.5;
    let y = bounds.y + bounds.height * 0.5;
    let extent = (5.0 * text_scale.max(1.0))
        .min(bounds.width * 0.5)
        .min(bounds.height * 0.5);
    let mut arrow = Path::new();
    if expanded {
        arrow.move_to(x - extent, y - extent * 0.5);
        arrow.line_to(x + extent, y - extent * 0.5);
        arrow.line_to(x, y + extent * 0.5);
    } else {
        let dx = if direction == LayoutDirection::RightToLeft {
            -extent * 0.5
        } else {
            extent * 0.5
        };
        arrow.move_to(x - dx, y - extent);
        arrow.line_to(x - dx, y + extent);
        arrow.line_to(x + dx, y);
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
        let title = geometry.content_bounds(*bounds, 8.0, title_width);
        let origin = if geometry.mirrored() {
            title.x + title.width
        } else {
            title.x
        };
        let title = title.intersect(visible);
        if title.width > 0.0 && title.height > 0.0 {
            canvas.save();
            canvas.intersect_scissor(title.x, title.y, title.width, title.height);
            let size = xrack_layout::table_heading_font_size(text)
                .clamp(1.0, (bounds.height / 1.4).max(1.0));
            let mut paint = text.paint(size, color(theme.text), false);
            if geometry.mirrored() {
                paint.set_text_align(Align::Right);
            }
            let caption = column.text("title").unwrap_or("");
            let caption = if text.text_scale() > 1.0 {
                xrack_layout::elide_to_width(caption, title_width, |value| {
                    text.advance_painted(value, &paint)
                })
            } else {
                std::borrow::Cow::Borrowed(caption)
            };
            let result = text.fill_text(
                canvas,
                origin,
                bounds.y + (bounds.height - size * 1.4).max(0.0) * 0.5,
                &caption,
                &paint,
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
                geometry.content_bounds(*bounds, bounds.width - 12.0, 0.0).x,
                bounds.y + bounds.height * 0.5,
                4.0,
                !sort.descending,
                theme.text,
            );
            canvas.restore();
        }
        let thickness = if resizing { 2.0 } else { 1.0 };
        let separator = geometry.content_bounds(*bounds, bounds.width - thickness, thickness);
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
        let mut size =
            xrack_layout::table_heading_font_size(text).clamp(1.0, (bounds.height / 1.4).max(1.0));
        let label_width = (bounds.width - 12.0).max(0.0);
        let label_clip =
            Rect::new(bounds.x + 6.0, bounds.y, label_width, bounds.height).intersect(visible);
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
            let mut paint = text.paint(size, color(tint), false);
            if text.text_scale() > 1.0 {
                let advance = text.advance_painted(label, &paint);
                if advance > label_width {
                    size = (size * label_width / advance).max(1.0);
                    paint.set_font_size(size);
                }
            }
            let x = if table.mirrored() {
                paint.set_text_align(Align::Right);
                bounds.x + bounds.width - 6.0
            } else {
                bounds.x + 6.0
            };
            let result = text.fill_text(
                canvas,
                x,
                bounds.y + (bounds.height - size * 1.4).max(0.0) * 0.5,
                label,
                &paint,
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
        table.content_bounds(gutter, gutter.width - 1.0, 1.0).x,
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
    node: &Node,
    theme: Palette,
    editable: bool,
) {
    let foreground = if editable {
        theme.text
    } else {
        theme.disabled_text
    };
    for (rect, up, kind) in [
        (geometry.up, true, "SpinUpIndicator"),
        (geometry.down, false, "SpinDownIndicator"),
    ] {
        if node.control_part(kind).is_some() {
            continue;
        }
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
    layout: &LayoutNode,
    open: bool,
) -> Result<ComboGeometry, String> {
    let foreground = if layout.enabled {
        node.color("color").unwrap_or(theme.text)
    } else {
        theme.disabled_text
    };
    let geometry = ComboGeometry::for_control(node, layout.bounds, layout.layout_direction);
    if node.control_part("ComboIndicator").is_none() {
        let trigger = geometry.trigger;
        chevron(
            canvas,
            trigger.x + trigger.width * 0.5,
            trigger.y + trigger.height * 0.5,
            4.0,
            open,
            foreground,
        );
    }
    if !combo_editable(node) && node.combo_content().is_none() {
        let label = geometry.label_bounds(layout.layout_direction);
        let rtl = layout.layout_direction == LayoutDirection::RightToLeft;
        let size = text.font_size(node);
        let mut paint = text.paint_node_at_size(node, size, color(foreground), false);
        let x = if rtl {
            paint.set_text_align(Align::Right);
            label.x + label.width - 1.0
        } else {
            label.x + 1.0
        };
        draw_label(canvas, text, label, combo_text(node), size, x, &paint)?;
    }
    Ok(geometry)
}

fn choice_radius(menu: &ChoiceMenu<'_>) -> f32 {
    menu.popup_style()
        .and_then(|style| style.length("border-radius"))
        .unwrap_or(6.0)
        .clamp(0.0, menu.bounds.width.min(menu.bounds.height) * 0.5)
}

pub(crate) fn choice_surface(
    canvas: &mut Canvas<WGPURenderer>,
    menu: &ChoiceMenu<'_>,
    theme: Palette,
) -> Path {
    let bounds = menu.bounds;
    let mut path = Path::new();
    path.rounded_rect(
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        choice_radius(menu),
    );
    let paint = menu
        .popup_style()
        .and_then(|style| brush_paint(style, "background", bounds, 0.0))
        .unwrap_or_else(|| Paint::color(color(theme.role(Role::Surface))));
    canvas.fill_path(&path, &paint);
    path
}

pub(crate) fn choice_border(
    canvas: &mut Canvas<WGPURenderer>,
    menu: &ChoiceMenu<'_>,
    theme: Palette,
) {
    let Some(style) = menu.popup_style() else {
        return;
    };
    let width = style
        .length("border-width")
        .unwrap_or(0.0)
        .clamp(0.0, menu.bounds.width.min(menu.bounds.height) * 0.5);
    if width == 0.0 {
        return;
    }
    let bounds = menu.bounds.inset(width * 0.5);
    let mut path = Path::new();
    path.rounded_rect(
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        (choice_radius(menu) - width * 0.5).max(0.0),
    );
    let mut paint = brush_paint(style, "border-color", menu.bounds, 0.0)
        .unwrap_or_else(|| Paint::color(color(theme.role(Role::SelectionOutline))));
    paint.set_line_width(width);
    canvas.stroke_path(&path, &paint);
}

pub(crate) fn choice_indicator(canvas: &mut Canvas<WGPURenderer>, bounds: Rect, theme: Palette) {
    fill(canvas, bounds, bounds.width * 0.5, theme.disabled_text);
}

pub(crate) fn choice_row(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    menu: &ChoiceMenu<'_>,
    offset: usize,
) -> Result<(), String> {
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
    let size = menu.font_size.min((row.height / 1.4).max(1.0));
    let foreground = if menu.row_enabled(offset) {
        theme.text
    } else {
        theme.disabled_text
    };
    let mut paint = text.paint_node_at_size(menu.control, size, color(foreground), false);
    let pad = (row.width * 0.5).min(10.0);
    let label_bounds = Rect::new(row.x + pad, row.y, row.width - 2.0 * pad, row.height);
    let x = if menu.layout_direction == LayoutDirection::RightToLeft {
        paint.set_text_align(Align::Right);
        label_bounds.x + label_bounds.width
    } else {
        label_bounds.x
    };
    let label = xrack_layout::elide_to_width(&menu.rows[offset], label_bounds.width, |value| {
        text.advance_painted(value, &paint)
    });
    draw_label(canvas, text, label_bounds, &label, size, x, &paint)
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
    let paint = text.paint_node_at_size(node, size, color(tint), false);
    draw_label(canvas, text, bounds, value, size, bounds.x, &paint)
}

pub(crate) fn leading_label(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    node: &Node,
    bounds: Rect,
    value: &str,
    tint: Color,
    direction: LayoutDirection,
) -> Result<(), String> {
    let size = text.font_size(node);
    let mut paint = text.paint_node_at_size(node, size, color(tint), false);
    let x = if direction == LayoutDirection::RightToLeft {
        paint.set_text_align(Align::Right);
        bounds.x + bounds.width
    } else {
        bounds.x
    };
    draw_label(canvas, text, bounds, value, size, x, &paint)
}

pub(crate) fn row_label(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    node: &Node,
    bounds: Rect,
    value: &str,
    tint: Color,
    direction: LayoutDirection,
) -> Result<(), String> {
    let size = text.row_label_font_size(node, bounds.height);
    let mut paint = text.paint_node_at_size(node, size, color(tint), false);
    let x = if direction == LayoutDirection::RightToLeft {
        paint.set_text_align(Align::Right);
        bounds.x + bounds.width
    } else {
        bounds.x
    };
    let value = if text.text_scale() > 1.0 {
        xrack_layout::elide_to_width(value, bounds.width, |label| {
            text.advance_painted(label, &paint)
        })
    } else {
        std::borrow::Cow::Borrowed(value)
    };
    draw_label(canvas, text, bounds, &value, size, x, &paint)
}

fn draw_label(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    bounds: Rect,
    value: &str,
    size: f32,
    x: f32,
    paint: &Paint,
) -> Result<(), String> {
    if bounds.width <= 0.0 || bounds.height <= 0.0 || value.is_empty() {
        return Ok(());
    }
    canvas.save();
    canvas.intersect_scissor(bounds.x, bounds.y, bounds.width, bounds.height);
    let result = text.fill_text(
        canvas,
        x,
        bounds.y + ((bounds.height - size * 1.4) * 0.5).max(0.0),
        value,
        paint,
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

pub(crate) fn radio_indicator(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Palette,
    node: &Node,
    bounds: Rect,
    enabled: bool,
) {
    let radius = bounds.width.min(bounds.height) * 0.5;
    let x = bounds.x + bounds.width * 0.5;
    let y = bounds.y + bounds.height * 0.5;
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
}

pub(crate) fn draw_title(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    node: &Node,
    bounds: Rect,
    tint: Color,
    size: f32,
    lines: &[xrack_layout::PlannedLabelLine],
) -> Result<(), String> {
    let title = node.text("title").unwrap_or_default();
    let paint = text.paint_node_at_size(node, size, color(tint), false);
    for line in lines {
        text.fill_text(
            canvas,
            bounds.x + line.origin.x,
            bounds.y + line.origin.y,
            &line.text(title),
            &paint,
        )
        .map_err(|error| format!("title drawing failed: {error}"))?;
    }
    Ok(())
}

pub(crate) fn expander_header(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    theme: Palette,
    node: &Node,
    header: Rect,
    layout: &LayoutNode,
) -> Result<(), String> {
    let rtl = layout.layout_direction == LayoutDirection::RightToLeft;
    let direction = if rtl { -1.0 } else { 1.0 };
    let x = if rtl {
        header.x + header.width - 10.0
    } else {
        header.x + 10.0
    };
    let y = header.y + header.height * 0.5;
    let foreground = if layout.enabled {
        node.color("color").unwrap_or(theme.text)
    } else {
        theme.disabled_text
    };
    let mut chevron = Path::new();
    if node.expander_collapsed() {
        chevron.move_to(x - 2.0 * direction, y - 4.0);
        chevron.line_to(x + 2.0 * direction, y);
        chevron.line_to(x - 2.0 * direction, y + 4.0);
    } else {
        chevron.move_to(x - 4.0, y - 2.0);
        chevron.line_to(x, y + 2.0);
        chevron.line_to(x + 4.0, y - 2.0);
    }
    let mut paint = Paint::color(color(foreground));
    paint.set_line_width(1.5);
    canvas.stroke_path(&chevron, &paint);
    if let Some((size, lines)) = layout.group_title() {
        return draw_title(canvas, text, node, header, foreground, size, lines);
    }
    let mut paint = text.paint_node(node, color(foreground), false);
    let size = text.font_size(node);
    if rtl {
        paint.set_text_align(Align::Right);
    }
    text.fill_text(
        canvas,
        x + direction * 13.0,
        header.y + ((header.height - size * 1.4) * 0.5).max(0.0),
        node.text("title").unwrap_or(""),
        &paint,
    )
    .map(|_| ())
    .map_err(|error| format!("expander header drawing failed: {error}"))
}

pub(crate) fn group_title(
    canvas: &mut Canvas<WGPURenderer>,
    text: &TextSystem,
    node: &Node,
    bounds: Rect,
    tint: Color,
    direction: LayoutDirection,
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
    let right = node
        .length("padding-right")
        .or_else(|| node.length("padding"))
        .unwrap_or(0.0)
        .max(0.0);
    let size = text.font_size(node);
    leading_label(
        canvas,
        text,
        node,
        Rect::new(
            bounds.x + left,
            bounds.y + top,
            (bounds.width - left - right).max(0.0),
            (bounds.height - top).clamp(0.0, size * 1.4),
        ),
        node.text("title").unwrap_or(""),
        tint,
        direction,
    )
}

pub(crate) struct TabAppearance {
    pub selected: bool,
    pub enabled: bool,
    pub focused: bool,
    pub close: Option<Rect>,
    pub close_hovered: bool,
    pub direction: LayoutDirection,
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
    let content = if let Some(close) = appearance.close {
        if appearance.enabled && appearance.close_hovered {
            fill(canvas, close, 4.0, theme.role(Role::SelectedSurface));
        }
        let center = (close.x + close.width / 2.0, close.y + close.height / 2.0);
        let radius = (close.width * 0.19).min(5.0);
        let mut glyph = Path::new();
        glyph.move_to(center.0 - radius, center.1 - radius);
        glyph.line_to(center.0 + radius, center.1 + radius);
        glyph.move_to(center.0 - radius, center.1 + radius);
        glyph.line_to(center.0 + radius, center.1 - radius);
        let tint = if appearance.enabled {
            theme.text
        } else {
            theme.disabled_text
        };
        let mut paint = Paint::color(color(tint));
        paint.set_line_width(1.5);
        canvas.stroke_path(&glyph, &paint);
        xrack_layout::tab_content_rect(bounds, close)
    } else {
        bounds
    };
    if node.kind == "TabButton" && node.control_part("ButtonContent").is_some() {
        return Ok(());
    }
    let label_bounds = Rect::new(
        content.x + 10.0,
        content.y,
        (content.width - 20.0).max(0.0),
        content.height,
    );
    let size = xrack_layout::tab_font_size(node, text, bounds.height);
    let tint = if appearance.enabled {
        theme.text
    } else {
        theme.disabled_text
    };
    if text.text_scale() <= 1.0 {
        return label(canvas, text, node, label_bounds, title, size, tint);
    }
    let mut paint = text.paint_node_at_size(node, size, color(tint), false);
    let x = if appearance.direction == LayoutDirection::RightToLeft {
        paint.set_text_align(Align::Right);
        label_bounds.x + label_bounds.width
    } else {
        label_bounds.x
    };
    let title = xrack_layout::elide_to_width(title, label_bounds.width, |value| {
        text.advance_painted(value, &paint)
    });
    draw_label(canvas, text, label_bounds, &title, size, x, &paint)
}
