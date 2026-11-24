use femtovg::{
    Align, Baseline, Canvas, Color, ErrorKind, FillRule, FontId, Paint, Path, TextContext,
    TextDecoration, TextMetrics, renderer::WGPURenderer,
};
use fontdb::{Database, Family, Query, Style, Weight};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, VecDeque},
    hash::BuildHasher,
    ops::Range,
    rc::Rc,
    sync::Arc,
};
use xrack_layout::{
    LabelLayout, LabelLine, LabelStyle, Point, Rect, Size, StyledTextLayout, TextAlignment,
    TextAreaLayout, TextMeasurer, VisualTextLine, text_editor_viewport,
};
use xrack_runtime::{EmbeddedFonts, LayoutDirection, Node, TextStyle, TextWrap};

#[cfg(target_os = "android")]
use crate::{AndroidGlyphBitmap, android_emoji::Emoji};
#[cfg(target_os = "ios")]
use crate::{color_glyphs::ColorGlyphs, ios_font::NativeFont};

pub(crate) fn fill_selected_text(
    canvas: &mut Canvas<WGPURenderer>,
    text_system: &TextSystem,
    clip: Rect,
    origin: Point,
    text: &str,
    paint: &Paint,
    spans: &[Range<f32>],
    selected_color: Option<Color>,
) -> Result<(), ErrorKind> {
    fill_with_selection(
        canvas,
        clip,
        origin.x,
        paint,
        spans,
        selected_color,
        |canvas, paint| {
            text_system
                .fill_text(canvas, origin.x, origin.y, text, paint)
                .map(|_| ())
        },
    )
}

pub(crate) fn fill_with_selection(
    canvas: &mut Canvas<WGPURenderer>,
    clip: Rect,
    origin_x: f32,
    paint: &Paint,
    spans: &[Range<f32>],
    selected_color: Option<Color>,
    mut draw: impl FnMut(&mut Canvas<WGPURenderer>, &Paint) -> Result<(), ErrorKind>,
) -> Result<(), ErrorKind> {
    let Some(selected_color) = selected_color.filter(|_| !spans.is_empty()) else {
        return draw(canvas, paint);
    };

    // Draw the complete shaped line through disjoint clips. Splitting the
    // string here would reshape ligatures and could reorder bidi fragments.
    let mut normal = Path::new();
    let mut selected = Path::new();
    let right = clip.x + clip.width;
    let mut left = clip.x;
    for span in spans {
        let start = (origin_x + span.start).max(left);
        let end = (origin_x + span.end).min(right);
        if end <= start {
            continue;
        }
        if start > left {
            normal.rect(left, clip.y, start - left, clip.height);
        }
        selected.rect(start, clip.y, end - start, clip.height);
        left = end;
    }
    if selected.is_empty() {
        return draw(canvas, paint);
    }
    if left < right {
        normal.rect(left, clip.y, right - left, clip.height);
    }
    let mut fill = |path: &Path, paint: &Paint| {
        canvas.save();
        canvas.clip_path(path, FillRule::NonZero);
        let result = draw(canvas, paint);
        canvas.restore();
        result
    };
    if !normal.is_empty() {
        fill(&normal, paint)?;
    }
    if selected_color.a > 0.0 {
        fill(&selected, &paint.clone().with_color(selected_color))?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FontRequest {
    family: String,
    weight: u16,
    italic: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct StyledFace {
    id: Option<FontId>,
    weight: u16,
    italic: bool,
}

#[derive(Clone, Copy)]
pub(super) struct LabelFont {
    size: f32,
    face: Option<StyledFace>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct LabelSettings {
    available: [u32; 2],
    style: LabelStyle,
    font_size: u32,
    face: Option<StyledFace>,
    registered_fonts: [usize; 2],
}

struct CachedLabel {
    text: String,
    settings: LabelSettings,
    label: Rc<PreparedLabel>,
    bytes: usize,
}

pub(crate) struct PreparedLabel {
    pub(crate) layout: LabelLayout<'static>,
    paint: Paint,
    glyphs: RefCell<Option<(f32, crate::cell_glyphs::Run)>>,
}

impl PreparedLabel {
    const MAX_CACHED_TEXT_BYTES: usize = 64;

    pub(crate) fn paint(&self, tint: Color) -> Paint {
        self.paint.clone().with_color(tint)
    }

    pub(super) fn visit_glyphs(
        &self,
        context: &TextContext,
        origin: Point,
        scale: f32,
        mut visit: impl FnMut(FontId, u16, f32, f32) -> bool,
    ) -> Option<bool> {
        let [line] = self.layout.lines.as_slice() else {
            return None;
        };
        let text = &line.text;
        if text.len() > Self::MAX_CACHED_TEXT_BYTES || !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let mut cached = self.glyphs.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|(measured_scale, _)| *measured_scale != scale)
        {
            let paint = self
                .paint
                .clone()
                .with_font_size(self.paint.font_size() * scale);
            let metrics = context.measure_text(0.0, 0.0, text, &paint).ok()?;
            if metrics.glyphs.len() > text.len() {
                return None;
            }
            *cached = Some((scale, crate::cell_glyphs::Run::from_metrics(&metrics)));
        }
        let (_, run) = cached.as_ref()?;
        Some(
            crate::cell_glyphs::placed_glyphs(
                run.glyphs.iter().copied(),
                run.baseline,
                [origin.x, origin.y],
                scale,
            )
            .all(|(font, glyph, x, y)| visit(font, glyph, x, y)),
        )
    }
}

#[derive(Default)]
struct LabelCache {
    entries: HashMap<u64, CachedLabel>,
    order: VecDeque<u64>,
    bytes: usize,
}

impl LabelCache {
    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }

    fn get<'a>(
        &mut self,
        text: &'a str,
        settings: LabelSettings,
        build: impl FnOnce() -> (LabelLayout<'a>, Paint),
    ) -> Rc<PreparedLabel> {
        const MAX_ENTRIES: usize = 2048;
        const MAX_BYTES: usize = 2 * 1024 * 1024;

        let key = self.entries.hasher().hash_one((text, settings));
        // Compare the full inputs on lookup, including collisions of the hash.
        if let Some(entry) = self.entries.get(&key)
            && entry.text == text
            && entry.settings == settings
        {
            return entry.label.clone();
        }
        let (plan, paint) = build();
        let layout = LabelLayout {
            size: plan.size,
            lines: plan
                .lines
                .into_iter()
                .map(|line| LabelLine {
                    text: Cow::Owned(line.text.into_owned()),
                    origin: line.origin,
                })
                .collect(),
        };
        let bytes = size_of::<CachedLabel>()
            .saturating_add(size_of::<PreparedLabel>())
            .saturating_add(text.len())
            .saturating_add(layout.lines.capacity() * size_of::<LabelLine<'static>>())
            .saturating_add(
                layout
                    .lines
                    .iter()
                    .map(|line| match &line.text {
                        Cow::Owned(text) => text.capacity(),
                        Cow::Borrowed(_) => 0,
                    })
                    .sum::<usize>(),
            );
        let glyph_bytes = match layout.lines.as_slice() {
            // Reserve a bounded glyph allocation in the same cache budget.
            [line] if line.text.len() <= PreparedLabel::MAX_CACHED_TEXT_BYTES => {
                line.text.len() * size_of::<crate::cell_glyphs::Glyph>()
            }
            _ => 0,
        };
        let bytes = bytes.saturating_add(glyph_bytes);
        let label = Rc::new(PreparedLabel {
            layout,
            paint,
            glyphs: RefCell::new(None),
        });
        if bytes > MAX_BYTES {
            return label;
        }
        if let Some(entry) = self.entries.remove(&key) {
            self.bytes -= entry.bytes;
            self.order.retain(|old| *old != key);
        }
        while self.entries.len() >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES {
            let Some(old) = self
                .order
                .pop_front()
                .and_then(|key| self.entries.remove(&key))
            else {
                return label;
            };
            self.bytes -= old.bytes;
        }
        self.entries.insert(
            key,
            CachedLabel {
                text: text.to_owned(),
                settings,
                label: label.clone(),
                bytes,
            },
        );
        self.order.push_back(key);
        self.bytes += bytes;
        label
    }
}

struct FontCatalog {
    db: Database,
    primary: String,
    loaded: HashMap<fontdb::ID, Option<FontId>>,
    // Glyph bounds use the same bytes as the registered font.
    outline_faces: HashMap<FontId, FontSource>,
    matches: HashMap<FontRequest, Option<FontId>>,
}

#[derive(Clone, Copy)]
pub(crate) struct OutlineGlyph {
    pub(crate) bounds: Option<ttf_parser::Rect>,
    pub(crate) units_per_em: u16,
}

#[derive(Clone, Copy)]
pub(super) struct GlyphExtents {
    pub(super) min_bearing: [f32; 2],
    pub(super) max_bearing: [f32; 2],
    pub(super) size: [f32; 2],
}

pub(super) fn atlas_size_range(size: f32, scale: f32) -> [f32; 2] {
    // A previous paint in the same FemtoVG size bin may have produced
    // slightly different rounded bearings and texel extents.
    let size_bin = (size * scale * 10.0).trunc();
    [
        (size_bin / 10.0 - 0.0001).max(0.0),
        (size_bin + 1.0) / 10.0 + 0.0001,
    ]
}

impl OutlineGlyph {
    pub(super) fn atlas_extents(self, size_range: [f32; 2]) -> Option<GlyphExtents> {
        let ink = self.bounds?;
        let [small, large] = size_range.map(|size| size / f32::from(self.units_per_em));
        let bearing = [f32::from(ink.x_min), -f32::from(ink.y_max)];
        Some(GlyphExtents {
            min_bearing: bearing.map(|value| (value * small).min(value * large).round()),
            max_bearing: bearing.map(|value| (value * small).max(value * large).round()),
            size: [ink.width(), ink.height()].map(|extent| (f32::from(extent) * large).ceil()),
        })
    }
}

impl GlyphExtents {
    pub(super) fn padded_edges(self) -> [f32; 4] {
        [
            self.min_bearing[0] - 1.0,
            self.min_bearing[1] - 1.0,
            self.max_bearing[0] + self.size[0] + 1.0,
            self.max_bearing[1] + self.size[1] + 1.0,
        ]
    }
}

impl FontCatalog {
    fn outline_glyph(&self, font: FontId, glyph: u16) -> Option<OutlineGlyph> {
        let source = self.outline_faces.get(&font)?;
        let face = ttf_parser::Face::parse(source.bytes.as_ref(), source.index).ok()?;
        let glyph = ttf_parser::GlyphId(glyph);
        if face.is_variable()
            || face.is_color_glyph(glyph)
            || face.glyph_svg_image(glyph).is_some()
            || face.glyph_raster_image(glyph, u16::MAX).is_some()
        {
            return None;
        }
        Some(OutlineGlyph {
            bounds: face.glyph_bounding_box(glyph),
            units_per_em: face.units_per_em(),
        })
    }

    fn new(
        db: Database,
        loaded: HashMap<fontdb::ID, Option<FontId>>,
        first: Option<FontId>,
        fallback: &[FallbackFace],
    ) -> Self {
        let primary = db
            .faces()
            .find(|face| first.is_some() && loaded.get(&face.id) == Some(&first))
            .or_else(|| db.faces().next())
            .and_then(|face| face.families.first())
            .map(|(family, _)| family.clone())
            .unwrap_or_default();
        let outline_faces = fallback
            .iter()
            .filter_map(|fallback| {
                let font = loaded.get(&fallback.id?).copied().flatten()?;
                Some((font, fallback.source.clone()))
            })
            .collect();
        Self {
            db,
            primary,
            loaded,
            outline_faces,
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
            *self.loaded.entry(id).or_insert_with(|| {
                let source = FontSource::from_database(&self.db, id)?;
                let font = source.load(context).ok()?;
                self.outline_faces.insert(font, source);
                Some(font)
            })
        });
        // Bound memoization of requests supplied by dynamic application bindings.
        if self.matches.len() >= 128 {
            self.matches.clear();
        }
        self.matches.insert(request.clone(), font);
        font
    }
}

#[derive(Clone)]
struct FontBytes(Arc<dyn AsRef<[u8]> + Send + Sync>);

impl AsRef<[u8]> for FontBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref().as_ref()
    }
}

#[derive(Clone)]
struct FontSource {
    bytes: FontBytes,
    index: u32,
}

impl FontSource {
    fn from_database(db: &Database, id: fontdb::ID) -> Option<Self> {
        let face = db.face(id)?;
        let bytes = if let fontdb::Source::Binary(data) = &face.source {
            FontBytes(data.clone())
        } else {
            db.with_face_data(id, |data, _| FontBytes(Arc::new(Arc::<[u8]>::from(data))))?
        };
        Some(Self {
            bytes,
            index: face.index,
        })
    }

    fn load(&self, context: &TextContext) -> Result<FontId, String> {
        context
            .add_shared_font_with_index(self.bytes.clone(), self.index)
            .map_err(|error| error.to_string())
    }
}

struct FallbackFace {
    id: Option<fontdb::ID>,
    source: FontSource,
}

#[derive(Clone, PartialEq)]
enum SceneFonts {
    Embedded(&'static EmbeddedFonts),
    Loaded(xrack_runtime::LoadedFonts),
}

impl SceneFonts {
    fn from_scene(scene: &Node) -> Option<Self> {
        scene
            .fonts
            .map(Self::Embedded)
            .or_else(|| scene.loaded_fonts.clone().map(Self::Loaded))
    }

    fn load(&self, database: &mut Database) {
        match self {
            Self::Embedded(fonts) => {
                for &source in fonts.sources() {
                    database.load_font_source(fontdb::Source::Binary(Arc::new(source)));
                }
            }
            Self::Loaded(fonts) => {
                for source in fonts.sources() {
                    database.load_font_source(fontdb::Source::Binary(source.clone()));
                }
            }
        }
    }
}

#[cfg(target_os = "ios")]
struct NativeFallback {
    regular: (FontId, Rc<NativeFont>),
    bold: Option<(FontId, Rc<NativeFont>)>,
    bold_fonts: Vec<FontId>,
}

struct FontContext {
    identity: Rc<()>,
    families: Vec<String>,
    context: TextContext,
    fallback: Vec<FontId>,
    catalog: RefCell<FontCatalog>,
    bundled: Option<RefCell<FontCatalog>>,
    #[cfg(target_os = "android")]
    emoji_font: Option<(FontId, std::path::PathBuf, u32)>,
    #[cfg(target_os = "ios")]
    native_fallback: Option<NativeFallback>,
    #[cfg(target_os = "ios")]
    native_emoji: Option<(FontId, Rc<NativeFont>)>,
}

#[cfg(target_os = "android")]
fn android_emoji_font(
    db: &Database,
    loaded: &HashMap<fontdb::ID, Option<FontId>>,
) -> Option<(FontId, std::path::PathBuf, u32)> {
    let id = db.query(&Query {
        families: &[Family::Name("Noto Color Emoji")],
        ..Default::default()
    })?;
    let font = loaded.get(&id).copied().flatten()?;
    let face = db.face(id)?;
    let fontdb::Source::File(path) = &face.source else {
        return None;
    };
    Some((font, path.clone(), face.index))
}

impl FontContext {
    fn with_bundle(&self, sources: &[FallbackFace], fonts: &SceneFonts) -> Self {
        // FemtoVG tries its registered faces after the paint's fallback list.
        // A separate context keeps fonts from a previous scene out of that search.
        let context = TextContext::default();
        let mut loaded = HashMap::new();
        let fallback: Vec<_> = sources
            .iter()
            .filter_map(|source| {
                let font = source.source.load(&context).ok();
                if let Some(id) = source.id {
                    loaded.insert(id, font);
                }
                font
            })
            .collect();
        let system = self.catalog.borrow().db.clone();
        let mut db = Database::new();
        fonts.load(&mut db);
        let families = font_family_names(&system, Some(&db));
        #[cfg(target_os = "android")]
        let emoji_font = android_emoji_font(&system, &loaded);
        let catalog = RefCell::new(FontCatalog::new(
            system,
            loaded,
            fallback.first().copied(),
            sources,
        ));
        let bundled = Some(RefCell::new(FontCatalog::new(
            db,
            HashMap::new(),
            None,
            &[],
        )));
        let result = Self {
            identity: Rc::new(()),
            families,
            context,
            fallback,
            catalog,
            bundled,
            #[cfg(target_os = "android")]
            emoji_font,
            #[cfg(target_os = "ios")]
            native_fallback: None,
            #[cfg(target_os = "ios")]
            native_emoji: None,
        };
        #[cfg(target_os = "ios")]
        let result = if let Some(native) = &self.native_fallback {
            result.with_native_fallback(
                native.regular.1.clone(),
                native.bold.as_ref().map(|(_, font)| font.clone()),
            )
        } else {
            result
        };
        #[cfg(target_os = "ios")]
        let result = if let Some((_, emoji)) = &self.native_emoji {
            result.with_native_emoji(emoji.clone())
        } else {
            result
        };
        result
    }

    #[cfg(target_os = "ios")]
    fn with_native_fallback(
        mut self,
        regular: Rc<NativeFont>,
        bold: Option<Rc<NativeFont>>,
    ) -> Self {
        let Ok(regular_id) = self
            .context
            .add_shared_font_with_index(regular.data.clone(), 0)
        else {
            return self;
        };
        let position = self.fallback.len().min(1);
        self.fallback.insert(position, regular_id);
        self.fallback.truncate(8);
        let bold = bold.and_then(|font| {
            let id = self
                .context
                .add_shared_font_with_index(font.data.clone(), 0)
                .ok()?;
            Some((id, font))
        });
        let mut bold_fonts = self.fallback.clone();
        if let Some((id, _)) = &bold {
            bold_fonts[position] = *id;
        }
        self.native_fallback = Some(NativeFallback {
            regular: (regular_id, regular),
            bold,
            bold_fonts,
        });
        self
    }

    #[cfg(target_os = "ios")]
    fn with_native_emoji(mut self, font: Rc<NativeFont>) -> Self {
        let Ok(id) = self
            .context
            .add_shared_font_with_index(font.data.clone(), 0)
        else {
            return self;
        };
        let position = if self.native_fallback.is_some() { 2 } else { 1 };
        let insert = |fonts: &mut Vec<FontId>| {
            fonts.insert(position.min(fonts.len()), id);
            fonts.truncate(8);
        };
        insert(&mut self.fallback);
        if let Some(native) = &mut self.native_fallback {
            insert(&mut native.bold_fonts);
        }
        let catalog = self.catalog.get_mut();
        for face in catalog
            .db
            .faces()
            .filter(|face| face.post_script_name == "AppleColorEmoji")
        {
            catalog.loaded.insert(face.id, Some(id));
        }
        self.native_emoji = Some((id, font));
        self
    }
}

struct EditLayout {
    text: String,
    size: f32,
    face: Option<StyledFace>,
    wrap: TextWrap,
    alignment: TextAlignment,
    layout: TextAreaLayout,
}

struct EditLine {
    text: String,
    size: f32,
    face: Option<StyledFace>,
    geometry: Rc<VisualTextLine>,
}

fn font_family_names(system: &Database, bundled: Option<&Database>) -> Vec<String> {
    let mut families: Vec<_> = system
        .faces()
        .chain(bundled.into_iter().flat_map(Database::faces))
        .filter_map(|face| face.families.first())
        .map(|(family, _)| family.clone())
        .collect();
    families.sort_by_cached_key(|family| (family.to_ascii_lowercase(), family.clone()));
    families.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    families
}

mod cell_shaping;

pub(super) type CellFonts = [Option<FontId>; 8];

pub(super) struct CellPaint {
    pub(super) paint: Paint,
    pub(super) fonts: CellFonts,
}

/// One font registry and shaping cache shared by measurement and drawing.
pub struct TextSystem {
    base: FontContext,
    fallback_sources: Vec<FallbackFace>,
    bundled: Option<(SceneFonts, FontContext)>,
    active_fonts: Option<SceneFonts>,
    default_family: String,
    default_weight: u16,
    default_italic: bool,
    default_size: Option<f32>,
    text_scale: f32,
    system_bold_text: bool,
    typography: Rc<()>,
    intrinsic_measurements: xrack_layout::IntrinsicMeasurements,
    edit_layout: RefCell<Option<EditLayout>>,
    edit_line: RefCell<Option<EditLine>>,
    label_layouts: RefCell<LabelCache>,
    outline_glyphs: RefCell<HashMap<(FontId, u16), Option<OutlineGlyph>>>,
    cell_shapes: RefCell<cell_shaping::Cache>,
    pub(super) cell_glyphs: RefCell<crate::cell_glyphs::Cache>,
    styled_layouts: RefCell<crate::styled_text::LayoutCache>,
    retained_styled_text_enabled: bool,
    retained_text: RefCell<Option<crate::retained_text::Cache>>,
    pub(crate) styled_ink: RefCell<crate::styled_ink::InkCache>,
    #[cfg(target_os = "android")]
    android_emoji: Option<RefCell<Emoji>>,
    #[cfg(target_os = "ios")]
    color_glyphs: RefCell<ColorGlyphs>,
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
        // fontdb's Apple system font discovery targets macOS.
        #[cfg(target_os = "ios")]
        db.load_fonts_dir("/System/Library/Fonts");
        #[cfg(target_os = "android")]
        {
            db.load_fonts_dir("/system/fonts");
            db.set_sans_serif_family("Roboto");
        }
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
        #[cfg(target_os = "ios")]
        let color_emoji = NativeFont::with_post_script_name("AppleColorEmoji");
        // FemtoVG's fallback list holds at most eight font faces.
        for id in selected {
            // Core Text supplies Apple Color Emoji's layout independently of the bitmap strikes.
            #[cfg(target_os = "ios")]
            if color_emoji.is_some()
                && db
                    .face(id)
                    .is_some_and(|face| face.post_script_name == "AppleColorEmoji")
            {
                continue;
            }
            if let Some(source) = FontSource::from_database(&db, id) {
                let font = source.load(&context).ok();
                loaded.insert(id, font);
                if let Some(font) = font {
                    sources.push(FallbackFace {
                        id: Some(id),
                        source,
                    });
                    fonts.push(font);
                }
            }
            if fonts.len() == 8 {
                break;
            }
        }
        let system = Self::new(db, context, fonts, loaded, sources);
        #[cfg(target_os = "ios")]
        let system = if let Some(regular) = NativeFont::with_post_script_name("PingFangSC-Regular")
        {
            let mut system = system;
            let bold = NativeFont::with_post_script_name("PingFangSC-Semibold").map(Rc::new);
            system.base = system.base.with_native_fallback(Rc::new(regular), bold);
            system
        } else {
            system
        };
        #[cfg(target_os = "ios")]
        let system = if let Some(emoji) = color_emoji {
            let mut system = system;
            system.base = system.base.with_native_emoji(Rc::new(emoji));
            system
        } else {
            system
        };
        Ok(system)
    }

    pub fn from_fonts(fonts: impl IntoIterator<Item = (Vec<u8>, u32)>) -> Result<Self, String> {
        let context = TextContext::default();
        let mut db = Database::new();
        let mut loaded = HashMap::new();
        let mut fallback = Vec::new();
        let mut sources = Vec::new();
        for (data, index) in fonts.into_iter().take(8) {
            let source = FontSource {
                bytes: FontBytes(Arc::new(data)),
                index,
            };
            let font = source.load(&context)?;
            let ids = db.load_font_source(fontdb::Source::Binary(source.bytes.0.clone()));
            let id = ids
                .into_iter()
                .find(|&id| db.face(id).is_some_and(|face| face.index == index));
            if let Some(id) = id {
                loaded.insert(id, Some(font));
            }
            fallback.push(font);
            sources.push(FallbackFace { id, source });
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
        let families = font_family_names(&db, None);
        #[cfg(target_os = "android")]
        let emoji_font = android_emoji_font(&db, &loaded);
        let catalog = RefCell::new(FontCatalog::new(
            db,
            loaded,
            fallback.first().copied(),
            &fallback_sources,
        ));
        Self {
            base: FontContext {
                identity: Rc::new(()),
                families,
                context,
                fallback,
                catalog,
                bundled: None,
                #[cfg(target_os = "android")]
                emoji_font,
                #[cfg(target_os = "ios")]
                native_fallback: None,
                #[cfg(target_os = "ios")]
                native_emoji: None,
            },
            fallback_sources,
            bundled: None,
            active_fonts: None,
            default_family: String::new(),
            default_weight: 400,
            default_italic: false,
            default_size: None,
            text_scale: 1.0,
            system_bold_text: false,
            typography: Rc::new(()),
            intrinsic_measurements: Default::default(),
            edit_layout: RefCell::default(),
            edit_line: RefCell::default(),
            label_layouts: RefCell::default(),
            outline_glyphs: RefCell::default(),
            cell_shapes: RefCell::default(),
            cell_glyphs: RefCell::default(),
            styled_layouts: RefCell::default(),
            retained_styled_text_enabled: false,
            retained_text: RefCell::default(),
            styled_ink: RefCell::default(),
            #[cfg(target_os = "android")]
            android_emoji: None,
            #[cfg(target_os = "ios")]
            color_glyphs: RefCell::default(),
        }
    }

    pub(super) fn set_text_scale(&mut self, scale: f32) -> bool {
        if !scale.is_finite() || scale <= 0.0 || self.text_scale == scale {
            return false;
        }
        self.text_scale = scale;
        self.invalidate_intrinsic_measurements();
        self.edit_layout.get_mut().take();
        self.edit_line.get_mut().take();
        self.styled_layouts.get_mut().clear();
        true
    }

    pub(super) fn set_system_bold_text(&mut self, enabled: bool) -> bool {
        if self.system_bold_text == enabled {
            return false;
        }
        self.system_bold_text = enabled;
        self.invalidate_intrinsic_measurements();
        self.edit_layout.get_mut().take();
        self.edit_line.get_mut().take();
        self.styled_layouts.get_mut().clear();
        true
    }

    fn effective_weight(&self, weight: u16) -> u16 {
        if self.system_bold_text {
            (weight + 200).min(900)
        } else {
            weight
        }
    }

    pub fn set_window(&mut self, scene: &Node) {
        let fonts = SceneFonts::from_scene(scene);
        let fonts_changed = fonts != self.active_fonts;
        if fonts_changed {
            if let Some(fonts) = &fonts
                && self.bundled.as_ref().is_none_or(|(last, _)| last != fonts)
            {
                self.bundled = Some((
                    fonts.clone(),
                    self.base.with_bundle(&self.fallback_sources, fonts),
                ));
            }
            self.active_fonts = fonts;
            self.edit_layout.get_mut().take();
            self.edit_line.get_mut().take();
            self.styled_layouts.get_mut().clear();
        }
        let font = scene.font("default-font");
        let family = scene
            .text("default-font-family")
            .or_else(|| font.map(|font| font.family.as_str()))
            .unwrap_or("")
            .trim();
        let weight = weight(
            scene
                .int("default-font-weight")
                .or_else(|| font.map(|font| font.weight)),
            400,
        );
        let italic = scene.boolean("default-font-italic", font.is_some_and(|font| font.italic));
        let size = scene
            .length("default-font-size")
            .or_else(|| font.and_then(xrack_runtime::Font::finite_size))
            .filter(|size| *size > 0.0);
        if fonts_changed
            || self.default_family != family
            || self.default_weight != weight
            || self.default_italic != italic
            || self.default_size != size
        {
            self.invalidate_intrinsic_measurements();
        }
        if self.default_family != family {
            self.default_family = family.to_owned();
        }
        self.default_weight = weight;
        self.default_italic = italic;
        self.default_size = size;
    }

    fn invalidate_intrinsic_measurements(&mut self) {
        self.typography = Rc::new(());
        self.intrinsic_measurements = Default::default();
        self.label_layouts.get_mut().clear();
        self.outline_glyphs.get_mut().clear();
        self.cell_glyphs.get_mut().clear();
    }

    pub(crate) fn outline_glyph(&self, font: FontId, glyph: u16) -> Option<OutlineGlyph> {
        const MAX_OUTLINES: usize = 4096;
        let mut outlines = self.outline_glyphs.borrow_mut();
        let key = (font, glyph);
        if let Some(outline) = outlines.get(&key) {
            return *outline;
        }
        let context = self.font_context();
        let outline = context
            .bundled
            .as_ref()
            .and_then(|catalog| catalog.borrow().outline_glyph(font, glyph))
            .or_else(|| context.catalog.borrow().outline_glyph(font, glyph));
        if outlines.len() >= MAX_OUTLINES {
            outlines.clear();
        }
        outlines.insert(key, outline);
        outline
    }

    fn font_context(&self) -> &FontContext {
        match (&self.bundled, &self.active_fonts) {
            (Some((_, context)), Some(_)) => context,
            _ => &self.base,
        }
    }

    pub(super) fn shape_cell(
        &self,
        text: &str,
        size: f32,
        fonts: &CellFonts,
    ) -> Option<crate::cell_glyphs::Run> {
        if cfg!(any(
            xrack_femtovg_glyph_samples,
            target_os = "android",
            target_os = "ios"
        )) {
            return None;
        }
        self.cell_shapes.borrow_mut().shape(self, text, size, fonts)
    }

    pub(super) fn measure_cell(
        &self,
        text: &str,
        paint: &Paint,
        fonts: Option<&CellFonts>,
    ) -> Option<crate::cell_glyphs::Measurement> {
        use crate::cell_glyphs::Measurement;
        if let Some(fonts) = fonts
            && let Some(run) = self.shape_cell(text, paint.font_size(), fonts)
        {
            return Some(Measurement::Portable(run));
        }
        self.context()
            .measure_text(0.0, 0.0, text, paint)
            .ok()
            .map(Measurement::Legacy)
    }

    pub(crate) fn context(&self) -> &TextContext {
        &self.font_context().context
    }

    pub(crate) fn set_styled_text_incremental_enabled(&mut self, enabled: bool) {
        self.styled_layouts
            .get_mut()
            .set_incremental_enabled(enabled);
        self.styled_ink.get_mut().set_incremental_enabled(enabled);
    }

    pub(crate) fn styled_text_reused_fragments(&self) -> u64 {
        self.styled_layouts.borrow().reused_fragments
    }

    pub(crate) fn reused_styled_prefix(
        &self,
        plan: &Rc<StyledTextLayout>,
        document: [(usize, usize); 3],
    ) -> Option<crate::styled_text::ReusedPrefix> {
        self.styled_layouts.borrow().reused_prefix(plan, document)
    }

    pub(crate) fn registered_font_counts(&self) -> [usize; 2] {
        let fonts = self.font_context();
        [
            fonts.catalog.borrow().loaded.len(),
            fonts
                .bundled
                .as_ref()
                .map_or(0, |catalog| catalog.borrow().loaded.len()),
        ]
    }

    #[cfg(target_os = "android")]
    #[doc(hidden)]
    pub fn set_android_glyph_rasterizer(
        &mut self,
        rasterize: impl Fn(&std::path::Path, u32, u16, u16) -> Option<AndroidGlyphBitmap> + 'static,
    ) {
        self.android_emoji = Some(RefCell::new(Emoji::new(Box::new(rasterize))));
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(crate) fn begin_color_glyph_frame(&mut self, canvas_changed: bool) {
        #[cfg(target_os = "android")]
        if let Some(emoji) = &mut self.android_emoji {
            emoji.get_mut().begin_frame(canvas_changed);
        }
        #[cfg(target_os = "ios")]
        self.color_glyphs.get_mut().begin_frame(canvas_changed);
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(crate) fn end_color_glyph_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        #[cfg(target_os = "android")]
        if let Some(emoji) = &mut self.android_emoji {
            emoji.get_mut().end_frame(canvas);
        }
        #[cfg(target_os = "ios")]
        self.color_glyphs.get_mut().end_frame(canvas);
    }

    pub(crate) fn fill_text(
        &self,
        canvas: &mut Canvas<WGPURenderer>,
        x: f32,
        y: f32,
        text: &str,
        paint: &Paint,
    ) -> Result<TextMetrics, ErrorKind> {
        let metrics = canvas.fill_text(x, y, text, paint)?;
        #[cfg(target_os = "android")]
        if let Some((emoji, font)) = self
            .android_emoji
            .as_ref()
            .zip(self.font_context().emoji_font.as_ref())
        {
            emoji.borrow_mut().draw(canvas, &metrics, paint, font);
        }
        #[cfg(target_os = "ios")]
        if let Some((id, font)) = &self.font_context().native_emoji {
            self.color_glyphs
                .borrow_mut()
                .draw(canvas, &metrics, paint, *id, |glyph, size| {
                    font.rasterize_color_glyph(glyph, size)
                });
        }
        #[cfg(target_os = "ios")]
        if let Some(native) = &self.font_context().native_fallback {
            for (font_id, font) in std::iter::once(&native.regular).chain(native.bold.as_ref()) {
                for glyph in metrics
                    .glyphs
                    .iter()
                    .filter(|glyph| glyph.font_id == *font_id)
                {
                    font.draw(
                        canvas,
                        glyph.glyph_id,
                        Point {
                            x: glyph.x,
                            y: glyph.y,
                        },
                        paint,
                    );
                }
            }
        }
        Ok(metrics)
    }

    pub(crate) fn context_key(&self) -> &Rc<()> {
        &self.font_context().identity
    }

    pub(crate) fn default_style(&self) -> (&str, u16, bool, Option<f32>) {
        (
            &self.default_family,
            self.effective_weight(self.default_weight),
            self.default_italic,
            Some(self.default_size.unwrap_or(xrack_layout::DEFAULT_FONT_SIZE) * self.text_scale),
        )
    }

    pub(crate) fn font_attributes<'a>(&'a self, node: Option<&'a Node>) -> (&'a str, u16, bool) {
        let font = node.and_then(|node| node.font("font"));
        let family = node
            .and_then(|node| node.text("font-family"))
            .or_else(|| font.map(|font| font.family.as_str()))
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .unwrap_or(&self.default_family);
        let weight = weight(
            node.and_then(|node| node.int("font-weight"))
                .or_else(|| font.map(|font| font.weight)),
            self.default_weight,
        );
        let italic = node.map_or(self.default_italic, |node| {
            node.boolean(
                "font-italic",
                font.map_or(self.default_italic, |font| font.italic),
            )
        });
        (family, weight, italic)
    }

    pub(crate) fn set_retained_styled_text_enabled(&mut self, enabled: bool) -> bool {
        if self.retained_styled_text_enabled == enabled {
            return false;
        }
        self.retained_styled_text_enabled = enabled;
        self.invalidate_intrinsic_measurements();
        self.styled_layouts.get_mut().clear();
        true
    }

    fn retained_styled_text_available(&self) -> bool {
        self.retained_styled_text_enabled
            && !cfg!(any(
                target_os = "ios",
                target_os = "android",
                target_arch = "wasm32"
            ))
            && self.active_fonts.is_none()
            && !self.base.fallback.is_empty()
    }

    pub(crate) fn retained_text_font<'a>(&'a self, node: &'a Node) -> (&'a str, u16, bool) {
        let (family, weight, italic) = self.font_attributes(Some(node));
        (family, self.effective_weight(weight), italic)
    }

    pub(crate) fn retained_glyph_font(
        &self,
        font: &xrack_layout::StyledGlyphFont,
    ) -> Result<FontId, ErrorKind> {
        self.retained_text
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .register_font(self, font)
    }

    fn face(&self, node: Option<&Node>, style: TextStyle) -> Option<StyledFace> {
        self.resolve_face(self.font_attributes(node), style)
    }

    fn resolve_face(
        &self,
        (family, weight, italic): (&str, u16, bool),
        style: TextStyle,
    ) -> Option<StyledFace> {
        let family = if style.monospace { "monospace" } else { family };
        let weight = if style.bold { weight.max(700) } else { weight };
        let weight = self.effective_weight(weight);
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

    pub(super) fn cell_paint(&self, node: &Node, size: f32, color: Color) -> CellPaint {
        self.paint_with_fonts(
            self.face(Some(node), TextStyle::default()),
            size,
            color,
            false,
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
        self.paint_with_fonts(face, font_size, color, centered)
            .paint
    }

    fn paint_with_fonts(
        &self,
        face: Option<StyledFace>,
        font_size: f32,
        color: Color,
        centered: bool,
    ) -> CellPaint {
        let mut paint = Paint::color(color);
        let fallback = &self.font_context().fallback;
        #[cfg(target_os = "ios")]
        let fallback = self
            .font_context()
            .native_fallback
            .as_ref()
            .filter(|_| self.system_bold_text)
            .map_or(fallback, |native| &native.bold_fonts);
        let primary = face.and_then(|face| face.id);
        let ordered = primary.into_iter().chain(
            fallback
                .iter()
                .copied()
                .filter(|font| Some(*font) != primary),
        );
        let mut fonts = [None; 8];
        for (slot, font) in fonts.iter_mut().zip(ordered) {
            *slot = Some(font);
        }
        if let Some(first) = fonts[0] {
            let paint_fonts = fonts.map(|font| font.unwrap_or(first));
            let count = fonts.iter().take_while(|font| font.is_some()).count();
            paint.set_font(&paint_fonts[..count]);
        }
        if let Some(face) = face {
            paint.set_font_weight(f32::from(face.weight));
            paint.set_font_italic(face.italic);
        }
        paint.set_font_size(font_size);
        paint.set_text_baseline(Baseline::Top);
        paint.set_text_align(if centered { Align::Center } else { Align::Left });
        CellPaint { paint, fonts }
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
            return 0.0;
        }
        #[cfg(xrack_femtovg_glyph_samples)]
        {
            self.context()
                .measure_text_width(text, paint)
                .unwrap_or(0.0)
        }
        #[cfg(not(xrack_femtovg_glyph_samples))]
        {
            self.context()
                .measure_text(0.0, 0.0, text, paint)
                .map_or(0.0, |metrics| metrics.width())
        }
    }

    pub(crate) fn shape_visual_line(&self, text: &str, paint: &Paint) -> VisualTextLine {
        let metrics = self
            .context()
            .measure_text(0.0, 0.0, text, paint)
            .unwrap_or_default();
        VisualTextLine::new(
            text,
            metrics
                .glyphs
                .into_iter()
                .map(|glyph| (glyph.byte_index, glyph.advance_x)),
        )
    }

    pub fn single_line_layout(&self, node: &Node, text: &str) -> Rc<VisualTextLine> {
        self.single_line_layout_at_size(node, text, self.font_size(node))
    }

    #[doc(hidden)]
    pub fn single_line_layout_at_size(
        &self,
        node: &Node,
        text: &str,
        size: f32,
    ) -> Rc<VisualTextLine> {
        let face = self.face(Some(node), TextStyle::default());
        if let Some(last) = &*self.edit_line.borrow()
            && last.size == size
            && last.face == face
            && last.text == text
        {
            return last.geometry.clone();
        }
        let paint = self.paint_face(face, size, Color::white(), false);
        let geometry = Rc::new(self.shape_visual_line(text, &paint));
        self.edit_line.replace(Some(EditLine {
            text: text.into(),
            size,
            face,
            geometry: geometry.clone(),
        }));
        geometry
    }

    pub fn text_area_layout(&self, node: &Node, bounds: Rect, text: &str) -> TextAreaLayout {
        self.text_area_layout_in_direction(node, bounds, text, node.layout_direction())
    }

    #[doc(hidden)]
    pub fn text_area_layout_in_direction(
        &self,
        node: &Node,
        bounds: Rect,
        text: &str,
        direction: Option<LayoutDirection>,
    ) -> TextAreaLayout {
        let alignment = TextAlignment::editor(node, direction);
        let area = text_editor_viewport(node, bounds);
        let size = self.font_size(node);
        let face = self.face(Some(node), TextStyle::default());
        let wrap = node.text_wrap();
        if let Some(last) = &*self.edit_layout.borrow()
            && last.layout.area.width == area.width
            && last.size == size
            && last.face == face
            && last.wrap == wrap
            && last.alignment == alignment
            && last.text == text
        {
            return last.layout.clone().with_viewport(area);
        }
        let paint = self.paint_face(face, size, Color::white(), false);
        let layout = TextAreaLayout::shaped(
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
            |text| self.shape_visual_line(text, &paint),
        )
        .aligned(text, alignment, |text| self.advance_painted(text, &paint));
        self.edit_layout.replace(Some(EditLayout {
            text: text.into(),
            size,
            face,
            wrap,
            alignment,
            layout: layout.clone(),
        }));
        layout
    }

    pub(crate) fn lines(&self, text: &str, paint: &Paint, width: f32) -> Vec<Range<usize>> {
        if text.is_empty() || width <= 0.0 {
            return vec![];
        }
        // FemtoVG wraps on equality. Allow rounding when layout reapplies measured widths.
        self.context()
            .break_text_vec(width + 0.001, text, paint)
            .unwrap_or_else(|_| std::iter::once(0..text.len()).collect())
    }

    pub(crate) fn prepared_label(
        &self,
        node: &Node,
        available: Size,
        direction: Option<LayoutDirection>,
    ) -> Rc<PreparedLabel> {
        let font = self.label_font(self.font_attributes(Some(node)), self.font_size(node));
        self.prepared_label_with_font(node, available, direction, font)
    }

    pub(super) fn label_font(&self, attributes: (&str, u16, bool), size: f32) -> LabelFont {
        LabelFont {
            size,
            face: self.resolve_face(attributes, TextStyle::default()),
        }
    }

    pub(super) fn prepared_label_with_font(
        &self,
        node: &Node,
        available: Size,
        direction: Option<LayoutDirection>,
        LabelFont { size, face }: LabelFont,
    ) -> Rc<PreparedLabel> {
        let text = node.text("text").unwrap_or("");
        let style = LabelStyle::for_node(node, direction);
        let settings = LabelSettings {
            available: [available.width.to_bits(), available.height.to_bits()],
            style,
            font_size: size.to_bits(),
            face,
            registered_fonts: self.registered_font_counts(),
        };
        self.label_layouts.borrow_mut().get(text, settings, || {
            let paint = self.paint_face(face, size, Color::white(), false);
            let layout = LabelLayout::with_style(text, available, size, style, |value| {
                self.advance_painted(value, &paint)
            });
            (layout, paint)
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

    fn typography_revision(&self) -> Option<Rc<()>> {
        Some(self.typography.clone())
    }

    fn intrinsic_measurements(&mut self) -> Option<&mut xrack_layout::IntrinsicMeasurements> {
        Some(&mut self.intrinsic_measurements)
    }

    fn font_families(&self) -> &[String] {
        &self.font_context().families
    }

    fn default_font_size(&self) -> Option<f32> {
        self.default_size
    }

    fn text_scale(&self) -> f32 {
        self.text_scale
    }

    fn layout_styled_text(&mut self, node: &Node, available: Size) -> Option<StyledTextLayout> {
        self.shared_styled_text_layout(node, available)
            .map(|plan| (*plan).clone())
    }

    fn shared_styled_text_layout(
        &mut self,
        node: &Node,
        available: Size,
    ) -> Option<Rc<StyledTextLayout>> {
        if self.retained_styled_text_available()
            && let Some(plan) = self
                .retained_text
                .borrow_mut()
                .get_or_insert_with(Default::default)
                .layout(self, node, available)
        {
            return Some(plan);
        }
        self.styled_layouts
            .borrow_mut()
            .get(self, node, available, true)
    }

    fn measure_label(&mut self, node: &Node, max_width: f32) -> Size {
        let available = Size::new(max_width, f32::INFINITY);
        if self.retained_styled_text_available()
            && let Some(size) = self
                .retained_text
                .borrow_mut()
                .get_or_insert_with(Default::default)
                .measure(self, node, max_width)
        {
            return size;
        }
        self.styled_layouts
            .borrow_mut()
            .get(self, node, available, false)
            .map_or_else(
                || {
                    self.prepared_label(node, available, node.layout_direction())
                        .layout
                        .size
                },
                |plan| plan.size,
            )
    }

    fn font_size(&self, node: &Node) -> f32 {
        let size = node
            .font_size()
            .or(self.default_size)
            .unwrap_or(xrack_layout::DEFAULT_FONT_SIZE);
        (size * self.text_scale).clamp(1.0, 512.0)
    }

    fn measure_node(&mut self, node: &Node, text: &str, max_width: f32) -> Size {
        let size = self.font_size(node);
        self.measure_node_at_size(node, text, size, max_width)
    }

    fn measure_node_at_size(&mut self, node: &Node, text: &str, size: f32, max_width: f32) -> Size {
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
