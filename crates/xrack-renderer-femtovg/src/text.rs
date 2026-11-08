use femtovg::{Align, Baseline, Color, FontId, Paint, TextContext, TextMetrics};
use fontdb::{Database, Family, Query};
use std::{cell::RefCell, ops::Range, sync::Arc};
use xrack_layout::{LabelLayout, Rect, Size, TextAreaLayout, TextMeasurer, font_size};
use xrack_runtime::{Node, TextWrap};

struct EditLayout {
    text: String,
    size: f32,
    wrap: TextWrap,
    layout: TextAreaLayout,
}

/// One font registry and shaping cache shared by measurement and drawing.
pub struct TextSystem {
    pub(crate) context: TextContext,
    fonts: Vec<FontId>,
    edit_layout: RefCell<Option<EditLayout>>,
}

impl TextSystem {
    pub fn system() -> Result<Self, String> {
        let mut db = Database::new();
        db.load_system_fonts();
        let mut selected = Vec::new();
        if let Some(id) = db.query(&Query {
            families: &[Family::SansSerif],
            ..Default::default()
        }) {
            selected.push(id);
        }
        for family in [
            "Arial",
            "Segoe UI",
            "PingFang SC",
            "Microsoft YaHei",
            "Noto Sans CJK SC",
            "Noto Sans SC",
            "WenQuanYi Micro Hei",
            "Hiragino Sans",
            "Noto Sans Arabic",
            "Apple Color Emoji",
            "Segoe UI Emoji",
            "Noto Color Emoji",
        ] {
            if let Some(id) = db.query(&Query {
                families: &[Family::Name(family)],
                ..Default::default()
            }) && !selected.contains(&id)
            {
                selected.push(id);
            }
        }
        for face in db.faces() {
            if !selected.contains(&face.id) {
                selected.push(face.id);
            }
        }
        let context = TextContext::default();
        let mut fonts = Vec::new();
        // FemtoVG's fallback list holds at most eight font faces.
        for id in selected {
            if let Some(Ok(font)) = db.with_face_data(id, |data, index| {
                context.add_shared_font_with_index(Arc::<[u8]>::from(data), index)
            }) {
                fonts.push(font);
            }
            if fonts.len() == 8 {
                break;
            }
        }
        if fonts.is_empty() {
            return Err("no usable fonts found; install a system font or provide fonts to TextSystem::from_fonts".into());
        }
        Ok(Self {
            context,
            fonts,
            edit_layout: RefCell::default(),
        })
    }

    pub fn from_fonts(fonts: impl IntoIterator<Item = (Vec<u8>, u32)>) -> Result<Self, String> {
        let context = TextContext::default();
        let fonts = fonts
            .into_iter()
            .take(8)
            .map(|(data, index)| {
                context
                    .add_shared_font_with_index(data, index)
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if fonts.is_empty() {
            return Err("at least one font is required".into());
        }
        Ok(Self {
            context,
            fonts,
            edit_layout: RefCell::default(),
        })
    }

    pub fn paint(&self, font_size: f32, color: Color, centered: bool) -> Paint {
        let mut paint = Paint::color(color);
        paint.set_font(&self.fonts);
        paint.set_font_size(font_size);
        paint.set_text_baseline(Baseline::Top);
        paint.set_text_align(if centered { Align::Center } else { Align::Left });
        paint
    }

    pub fn shape(&self, text: &str, size: f32) -> Result<TextMetrics, String> {
        self.context
            .measure_text(0.0, 0.0, text, &self.paint(size, Color::white(), false))
            .map_err(|e| e.to_string())
    }

    pub fn advance(&self, text: &str, size: f32) -> f32 {
        if text.is_empty() {
            0.0
        } else {
            self.shape(text, size)
                .map_or(0.0, |metrics| metrics.width())
        }
    }

    pub fn text_area_layout(&self, node: &Node, bounds: Rect, text: &str) -> TextAreaLayout {
        let area = bounds.inset(node.length("padding").unwrap_or(6.0).max(0.0));
        let size = font_size(node);
        let wrap = node.text_wrap();
        if let Some(last) = &*self.edit_layout.borrow()
            && last.layout.area.width == area.width
            && last.size == size
            && last.wrap == wrap
            && last.text == text
        {
            return last.layout.clone().with_viewport(area);
        }
        let paint = self.paint(size, Color::white(), false);
        let layout = TextAreaLayout::new(
            text,
            area,
            size * 1.4,
            wrap,
            |text| self.advance(text, size),
            |text| {
                self.context
                    .break_text(area.width + 0.01, text, &paint)
                    .unwrap_or(text.len())
            },
        );
        self.edit_layout.replace(Some(EditLayout {
            text: text.into(),
            size,
            wrap,
            layout: layout.clone(),
        }));
        layout
    }

    pub(crate) fn lines(&self, text: &str, paint: &Paint, width: f32) -> Vec<Range<usize>> {
        if text.is_empty() || width <= 0.0 {
            return vec![];
        }
        self.context
            .break_text_vec(width, text, paint)
            .unwrap_or_else(|_| std::iter::once(0..text.len()).collect())
    }

    pub(crate) fn label_layout<'a>(&self, node: &'a Node, available: Size) -> LabelLayout<'a> {
        let size = font_size(node);
        LabelLayout::new(node, available, |value| self.advance(value, size))
    }
}

impl TextMeasurer for TextSystem {
    fn measure_label(&mut self, node: &Node, max_width: f32) -> Size {
        self.label_layout(node, Size::new(max_width, f32::INFINITY))
            .size
    }

    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> Size {
        if max_width <= 0.0 {
            return Size::default();
        }
        let paint = self.paint(font_size, Color::white(), false);
        let lines = self.lines(text, &paint, max_width);
        let width = lines
            .iter()
            .filter_map(|range| {
                self.context
                    .measure_text(0.0, 0.0, &text[range.clone()], &paint)
                    .ok()
            })
            .map(|m| m.width())
            .fold(0.0, f32::max);
        Size::new(
            width.min(max_width),
            lines.len().max(1) as f32 * font_size * 1.4,
        )
    }
}
