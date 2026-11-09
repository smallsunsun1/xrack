use femtovg::{Align, Baseline, Color, FontId, Paint, TextContext, TextDecoration, TextMetrics};
use fontdb::{Database, Family, Query, Style, Weight};
use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc, sync::Arc};
use xrack_layout::{LabelLayout, Rect, Size, TextAreaLayout, TextMeasurer};
use xrack_runtime::{EmbeddedFonts, Node, TextStyle, TextWrap};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FontRequest {
    family: String,
    weight: u16,
    italic: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct StyledFace {
    id: Option<FontId>,
    weight: u16,
    italic: bool,
}

struct FontCatalog {
    db: Database,
    primary: String,
    loaded: HashMap<fontdb::ID, Option<FontId>>,
    matches: HashMap<FontRequest, Option<FontId>>,
}

impl FontCatalog {
    fn new(
        db: Database,
        loaded: HashMap<fontdb::ID, Option<FontId>>,
        first: Option<FontId>,
    ) -> Self {
        let primary = db
            .faces()
            .find(|face| first.is_some() && loaded.get(&face.id) == Some(&first))
            .or_else(|| db.faces().next())
            .and_then(|face| face.families.first())
            .map(|(family, _)| family.clone())
            .unwrap_or_default();
        Self {
            db,
            primary,
            loaded,
            matches: HashMap::new(),
        }
    }

    fn resolve(
        &mut self,
        context: &TextContext,
        request: &FontRequest,
        fallback: bool,
    ) -> Option<FontId> {
        if let Some(font) = self.matches.get(request) {
            return *font;
        }
        let family = match request.family.as_str() {
            "" => self.primary.as_str(),
            "sans-serif" => self.db.family_name(&Family::SansSerif),
            "serif" => self.db.family_name(&Family::Serif),
            "monospace" => self
                .db
                .faces()
                .find(|face| face.monospaced)
                .and_then(|face| face.families.first())
                .map_or(self.db.family_name(&Family::Monospace), |(name, _)| name),
            name => self
                .db
                .faces()
                .flat_map(|face| &face.families)
                .find(|(family, _)| family.eq_ignore_ascii_case(name))
                .map_or(name, |(family, _)| family),
        };
        let families = [Family::Name(family), Family::Name(&self.primary)];
        let id = self.db.query(&Query {
            families: &families[..if fallback { 2 } else { 1 }],
            weight: Weight(request.weight),
            style: if request.italic {
                Style::Italic
            } else {
                Style::Normal
            },
            ..Default::default()
        });
        let font = id.and_then(|id| {
            *self
                .loaded
                .entry(id)
                .or_insert_with(|| load_face(&self.db, context, id))
        });
        // Bound memoization of requests supplied by dynamic application bindings.
        if self.matches.len() >= 128 {
            self.matches.clear();
        }
        self.matches.insert(request.clone(), font);
        font
    }
}

fn load_face(db: &Database, context: &TextContext, id: fontdb::ID) -> Option<FontId> {
    struct Binary(Arc<dyn AsRef<[u8]> + Send + Sync>);

    impl AsRef<[u8]> for Binary {
        fn as_ref(&self) -> &[u8] {
            self.0.as_ref().as_ref()
        }
    }

    let face = db.face(id)?;
    if let fontdb::Source::Binary(data) = &face.source {
        return context
            .add_shared_font_with_index(Binary(data.clone()), face.index)
            .ok();
    }
    db.with_face_data(id, |data, index| {
        context
            .add_shared_font_with_index(Arc::<[u8]>::from(data), index)
            .ok()
    })?
}

struct FallbackFace {
    id: Option<fontdb::ID>,
    data: Arc<[u8]>,
    index: u32,
}

impl FallbackFace {
    fn load(&self, context: &TextContext) -> Result<FontId, String> {
        context
            .add_shared_font_with_index(self.data.clone(), self.index)
            .map_err(|error| error.to_string())
    }
}

struct FontContext {
    identity: Rc<()>,
    context: TextContext,
    fallback: Vec<FontId>,
    catalog: RefCell<FontCatalog>,
    bundled: Option<RefCell<FontCatalog>>,
}

impl FontContext {
    fn with_bundle(&self, sources: &[FallbackFace], fonts: &EmbeddedFonts) -> Self {
        // FemtoVG tries its registered faces after the paint's fallback list.
        // A separate context keeps fonts from a previous scene out of that search.
        let context = TextContext::default();
        let mut loaded = HashMap::new();
        let fallback: Vec<_> = sources
            .iter()
            .filter_map(|source| {
                let font = source.load(&context).ok();
                if let Some(id) = source.id {
                    loaded.insert(id, font);
                }
                font
            })
            .collect();
        let system = self.catalog.borrow().db.clone();
        let catalog = RefCell::new(FontCatalog::new(system, loaded, fallback.first().copied()));
        let mut db = Database::new();
        for &source in fonts.sources() {
            db.load_font_source(fontdb::Source::Binary(Arc::new(source)));
        }
        let bundled = Some(RefCell::new(FontCatalog::new(db, HashMap::new(), None)));
        Self {
            identity: Rc::new(()),
            context,
            fallback,
            catalog,
            bundled,
        }
    }
}

struct EditLayout {
    text: String,
    size: f32,
    face: Option<StyledFace>,
    wrap: TextWrap,
    layout: TextAreaLayout,
}

/// One font registry and shaping cache shared by measurement and drawing.
pub struct TextSystem {
    base: FontContext,
    fallback_sources: Vec<FallbackFace>,
    bundled: Option<(&'static EmbeddedFonts, FontContext)>,
    active_fonts: Option<&'static EmbeddedFonts>,
    default_family: String,
    default_weight: u16,
    default_italic: bool,
    default_size: Option<f32>,
    edit_layout: RefCell<Option<EditLayout>>,
}

impl TextSystem {
    /// Use only the fonts embedded in the scene's UI library.
    pub fn bundled_only() -> Self {
        Self::new(
            Database::new(),
            TextContext::default(),
            Vec::new(),
            HashMap::new(),
            Vec::new(),
        )
    }

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
        let mut loaded = HashMap::new();
        let mut sources = Vec::new();
        // FemtoVG's fallback list holds at most eight font faces.
        for id in selected {
            if let Some(source) = db.with_face_data(id, |data, index| FallbackFace {
                id: Some(id),
                data: Arc::<[u8]>::from(data),
                index,
            }) {
                let font = source.load(&context).ok();
                loaded.insert(id, font);
                if let Some(font) = font {
                    sources.push(source);
                    fonts.push(font);
                }
            }
            if fonts.len() == 8 {
                break;
            }
        }
        Ok(Self::new(db, context, fonts, loaded, sources))
    }

    pub fn from_fonts(fonts: impl IntoIterator<Item = (Vec<u8>, u32)>) -> Result<Self, String> {
        let context = TextContext::default();
        let mut db = Database::new();
        let mut loaded = HashMap::new();
        let mut fallback = Vec::new();
        let mut sources = Vec::new();
        for (data, index) in fonts.into_iter().take(8) {
            let mut source = FallbackFace {
                id: None,
                data: data.into(),
                index,
            };
            let font = source.load(&context)?;
            let ids = db.load_font_source(fontdb::Source::Binary(Arc::new(source.data.clone())));
            source.id = ids
                .into_iter()
                .find(|&id| db.face(id).is_some_and(|face| face.index == index));
            if let Some(id) = source.id {
                loaded.insert(id, Some(font));
            }
            fallback.push(font);
            sources.push(source);
        }
        if fallback.is_empty() {
            return Err("at least one font is required".into());
        }
        Ok(Self::new(db, context, fallback, loaded, sources))
    }

    fn new(
        db: Database,
        context: TextContext,
        fallback: Vec<FontId>,
        loaded: HashMap<fontdb::ID, Option<FontId>>,
        fallback_sources: Vec<FallbackFace>,
    ) -> Self {
        let catalog = RefCell::new(FontCatalog::new(db, loaded, fallback.first().copied()));
        Self {
            base: FontContext {
                identity: Rc::new(()),
                context,
                fallback,
                catalog,
                bundled: None,
            },
            fallback_sources,
            bundled: None,
            active_fonts: None,
            default_family: String::new(),
            default_weight: 400,
            default_italic: false,
            default_size: None,
            edit_layout: RefCell::default(),
        }
    }

    pub fn set_window(&mut self, scene: &Node) {
        if scene.fonts != self.active_fonts {
            if let Some(fonts) = scene.fonts
                && self.bundled.as_ref().is_none_or(|(last, _)| *last != fonts)
            {
                self.bundled = Some((fonts, self.base.with_bundle(&self.fallback_sources, fonts)));
            }
            self.active_fonts = scene.fonts;
            self.edit_layout.get_mut().take();
        }
        self.default_family.clear();
        self.default_family
            .push_str(scene.text("default-font-family").unwrap_or("").trim());
        self.default_weight = weight(scene.int("default-font-weight"), 400);
        self.default_italic = scene.boolean("default-font-italic", false);
        self.default_size = scene.length("default-font-size").filter(|size| *size > 0.0);
    }

    fn font_context(&self) -> &FontContext {
        match (&self.bundled, self.active_fonts) {
            (Some((_, context)), Some(_)) => context,
            _ => &self.base,
        }
    }

    pub(crate) fn context(&self) -> &TextContext {
        &self.font_context().context
    }

    pub(crate) fn context_key(&self) -> &Rc<()> {
        &self.font_context().identity
    }

    fn face(&self, node: Option<&Node>, style: TextStyle) -> Option<StyledFace> {
        let family = node
            .and_then(|node| node.text("font-family"))
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .unwrap_or(&self.default_family);
        let weight = weight(
            node.and_then(|node| node.int("font-weight")),
            self.default_weight,
        );
        let italic = node.map_or(self.default_italic, |node| {
            node.boolean("font-italic", self.default_italic)
        });
        let family = if style.monospace { "monospace" } else { family };
        let weight = if style.bold { weight.max(700) } else { weight };
        let italic = italic || style.italic;
        let fonts = self.font_context();
        if family.is_empty() && weight == 400 && !italic && !fonts.fallback.is_empty() {
            return None;
        }
        let request = FontRequest {
            family: family.into(),
            weight,
            italic,
        };
        let id = fonts
            .bundled
            .as_ref()
            .filter(|_| !family.is_empty() || fonts.fallback.is_empty())
            .and_then(|catalog| {
                catalog
                    .borrow_mut()
                    .resolve(&fonts.context, &request, fonts.fallback.is_empty())
            })
            .or_else(|| {
                fonts
                    .catalog
                    .borrow_mut()
                    .resolve(&fonts.context, &request, true)
            });
        Some(StyledFace { id, weight, italic })
    }

    pub fn paint(&self, font_size: f32, color: Color, centered: bool) -> Paint {
        self.paint_face(
            self.face(None, TextStyle::default()),
            font_size,
            color,
            centered,
        )
    }

    pub(crate) fn paint_node(&self, node: &Node, color: Color, centered: bool) -> Paint {
        self.paint_node_at_size(node, self.font_size(node), color, centered)
    }

    pub(crate) fn paint_node_at_size(
        &self,
        node: &Node,
        size: f32,
        color: Color,
        centered: bool,
    ) -> Paint {
        self.paint_face(
            self.face(Some(node), TextStyle::default()),
            size,
            color,
            centered,
        )
    }

    pub(crate) fn paint_styled(&self, node: &Node, tint: Color, style: TextStyle) -> Paint {
        let mut paint = self.paint_face(
            self.face(Some(node), style),
            self.font_size(node),
            tint,
            false,
        );
        paint.set_text_baseline(Baseline::Alphabetic);
        paint.set_text_decoration(TextDecoration {
            underline: style.underline,
            strikethrough: style.strikethrough,
            ..Default::default()
        });
        paint
    }

    fn paint_face(
        &self,
        face: Option<StyledFace>,
        font_size: f32,
        color: Color,
        centered: bool,
    ) -> Paint {
        let mut paint = Paint::color(color);
        let fallback = &self.font_context().fallback;
        if let Some(face) = face.and_then(|face| face.id) {
            let mut fonts = [face; 8];
            let fallback = fallback
                .iter()
                .copied()
                .filter(|font| *font != face)
                .take(7);
            let mut len = 1;
            for font in fallback {
                fonts[len] = font;
                len += 1;
            }
            paint.set_font(&fonts[..len]);
        } else {
            paint.set_font(fallback);
        }
        if let Some(face) = face {
            paint.set_font_weight(f32::from(face.weight));
            paint.set_font_italic(face.italic);
        }
        paint.set_font_size(font_size);
        paint.set_text_baseline(Baseline::Top);
        paint.set_text_align(if centered { Align::Center } else { Align::Left });
        paint
    }

    pub fn shape(&self, text: &str, size: f32) -> Result<TextMetrics, String> {
        self.context()
            .measure_text(0.0, 0.0, text, &self.paint(size, Color::white(), false))
            .map_err(|e| e.to_string())
    }

    pub fn advance(&self, text: &str, size: f32) -> f32 {
        self.advance_painted(text, &self.paint(size, Color::white(), false))
    }

    pub fn advance_node(&self, node: &Node, text: &str) -> f32 {
        self.advance_painted(text, &self.paint_node(node, Color::white(), false))
    }

    pub(crate) fn advance_painted(&self, text: &str, paint: &Paint) -> f32 {
        if text.is_empty() {
            0.0
        } else {
            self.context()
                .measure_text(0.0, 0.0, text, paint)
                .map_or(0.0, |metrics| metrics.width())
        }
    }

    pub fn text_area_layout(&self, node: &Node, bounds: Rect, text: &str) -> TextAreaLayout {
        let area = bounds.inset(node.length("padding").unwrap_or(6.0).max(0.0));
        let size = self.font_size(node);
        let face = self.face(Some(node), TextStyle::default());
        let wrap = node.text_wrap();
        if let Some(last) = &*self.edit_layout.borrow()
            && last.layout.area.width == area.width
            && last.size == size
            && last.face == face
            && last.wrap == wrap
            && last.text == text
        {
            return last.layout.clone().with_viewport(area);
        }
        let paint = self.paint_face(face, size, Color::white(), false);
        let layout = TextAreaLayout::new(
            text,
            area,
            size * 1.4,
            wrap,
            |text| self.advance_painted(text, &paint),
            |text| {
                self.context()
                    .break_text(area.width + 0.01, text, &paint)
                    .unwrap_or(text.len())
            },
        );
        self.edit_layout.replace(Some(EditLayout {
            text: text.into(),
            size,
            face,
            wrap,
            layout: layout.clone(),
        }));
        layout
    }

    pub(crate) fn lines(&self, text: &str, paint: &Paint, width: f32) -> Vec<Range<usize>> {
        if text.is_empty() || width <= 0.0 {
            return vec![];
        }
        self.context()
            .break_text_vec(width, text, paint)
            .unwrap_or_else(|_| std::iter::once(0..text.len()).collect())
    }

    pub(crate) fn label_layout<'a>(&self, node: &'a Node, available: Size) -> LabelLayout<'a> {
        let size = self.font_size(node);
        let paint = self.paint_node_at_size(node, size, Color::white(), false);
        LabelLayout::with_font_size(node, available, size, |value| {
            self.advance_painted(value, &paint)
        })
    }

    fn measure_painted(&self, text: &str, paint: &Paint, font_size: f32, max_width: f32) -> Size {
        if max_width <= 0.0 {
            return Size::default();
        }
        let lines = self.lines(text, paint, max_width);
        let width = lines
            .iter()
            .filter_map(|range| {
                self.context()
                    .measure_text(0.0, 0.0, &text[range.clone()], paint)
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

impl TextMeasurer for TextSystem {
    fn begin_window(&mut self, scene: &Node) {
        self.set_window(scene);
    }

    fn default_font_size(&self) -> Option<f32> {
        self.default_size
    }

    fn measure_label(&mut self, node: &Node, max_width: f32) -> Size {
        let available = Size::new(max_width, f32::INFINITY);
        if let Some(text) = node.styled_text("text") {
            crate::styled_text::measure(self, node, text, available)
        } else {
            self.label_layout(node, available).size
        }
    }

    fn measure_node(&mut self, node: &Node, text: &str, max_width: f32) -> Size {
        let size = self.font_size(node);
        let paint = self.paint_node_at_size(node, size, Color::white(), false);
        self.measure_painted(text, &paint, size, max_width)
    }

    fn measure(&mut self, text: &str, size: f32, max_width: f32) -> Size {
        self.measure_painted(
            text,
            &self.paint(size, Color::white(), false),
            size,
            max_width,
        )
    }
}

fn weight(value: Option<i64>, fallback: u16) -> u16 {
    value
        .filter(|&weight| weight != 0)
        .map_or(fallback, |weight| weight.clamp(100, 900) as u16)
}
