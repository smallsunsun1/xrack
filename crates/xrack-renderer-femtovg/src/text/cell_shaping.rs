use super::{CellFonts, FontSource, TextSystem};
use crate::cell_glyphs::{Glyph, Run};
use femtovg::FontId;
use rustybuzz::{Direction, Face, Script, ShapePlan, UnicodeBuffer};
use std::{
    cell::OnceCell,
    collections::{HashMap, VecDeque},
    hash::BuildHasher,
    rc::Rc,
};
use unicode_bidi::{BidiClass, bidi_class};
use unicode_segmentation::UnicodeSegmentation;

fn supports(text: &str) -> bool {
    !text.is_ascii()
        && text
            .chars()
            .all(|ch| matches!(bidi_class(ch), BidiClass::L | BidiClass::EN))
}

self_cell::self_cell!(
    struct ParsedFace {
        owner: FontSource,
        #[covariant]
        dependent: Face,
    }
);

struct Font {
    face: ParsedFace,
    plans: HashMap<Script, ShapePlan>,
    buffer: UnicodeBuffer,
    ascii_glyphs: OnceCell<Option<Rc<[AsciiGlyph; 128]>>>,
}

#[derive(Clone, Copy, Default)]
struct AsciiGlyph {
    id: u16,
    advance: u16,
}

pub(crate) struct AsciiMetrics {
    glyphs: Rc<[AsciiGlyph; 128]>,
    font: FontId,
    scale: f32,
}

impl AsciiMetrics {
    fn glyph(&self, byte: u8) -> Option<AsciiGlyph> {
        self.glyphs
            .get(usize::from(byte))
            .copied()
            .filter(|glyph| glyph.id != 0)
    }

    pub(crate) fn width(&self, text: &str) -> Option<f32> {
        let word_width = |word: &str| {
            word.bytes().try_fold(0.0, |width, byte| {
                Some(width + (f32::from(self.glyph(byte)?.advance) * self.scale + 0.0))
            })
        };
        // This alphabet is left-to-right. Keep FemtoVG's word boundaries
        // and floating-point addition order, including on punctuation.
        let width = if text.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            word_width(text)
        } else {
            text.split_word_bounds()
                .try_fold(0.0, |width, word| Some(width + word_width(word)?))
        }?;
        width.is_finite().then_some(width)
    }

    pub(crate) fn word<T>(
        &self,
        text: &str,
        mut make: impl FnMut(FontId, u16, f32) -> T,
    ) -> Option<Vec<T>> {
        text.bytes()
            .map(|byte| {
                let glyph = self.glyph(byte)?;
                Some(make(
                    self.font,
                    glyph.id,
                    f32::from(glyph.advance) * self.scale,
                ))
            })
            .collect()
    }
}

pub(super) struct Word {
    pub(super) glyphs: Vec<Glyph>,
    pub(super) units_per_em: f32,
    ascender: f32,
}

struct CachedWord {
    text: String,
    fonts: CellFonts,
    word: Word,
}

#[derive(Default)]
pub(super) struct Cache {
    context: Option<Rc<()>>,
    fonts: HashMap<FontId, Option<Font>>,
    words: HashMap<u64, CachedWord>,
    word_order: VecDeque<u64>,
}

impl Cache {
    pub(super) fn shape(
        &mut self,
        system: &TextSystem,
        text: &str,
        size: f32,
        fonts: &CellFonts,
    ) -> Option<Run> {
        if !supports(text) {
            return None;
        }

        self.select_context(system);
        let mut glyphs = Vec::with_capacity(text.chars().count());
        let mut baseline = 0.0_f32;
        // Use the same word boundaries and ordered faces as FemtoVG.
        for word in text.split_word_bounds() {
            let word = self.word(system, word, fonts)?;
            let scale = size / word.units_per_em;
            glyphs.extend(word.glyphs.iter().map(|glyph| Glyph {
                advance: glyph.advance.map(|value| value * scale),
                offset: glyph.offset.map(|value| value * scale),
                ..*glyph
            }));
            baseline = baseline.max(word.ascender * scale);
        }
        // FemtoVG uses the primary face when the shaped ascent is zero.
        (baseline > f32::EPSILON).then_some(Run { baseline, glyphs })
    }

    pub(super) fn ascii_metrics(
        &mut self,
        system: &TextSystem,
        size: f32,
        id: FontId,
    ) -> Option<AsciiMetrics> {
        self.select_context(system);
        let font = self.font(system, id)?;
        Some(AsciiMetrics {
            glyphs: font.independent_ascii_glyphs()?.clone(),
            font: id,
            scale: size / font.face.borrow_dependent().units_per_em() as f32,
        })
    }

    #[cfg(any(test, not(xrack_femtovg_glyph_samples)))]
    pub(super) fn ltr_width_and_ink(
        &mut self,
        system: &TextSystem,
        text: &str,
        fonts: &CellFonts,
        size: f32,
    ) -> Option<(f32, Option<[f32; 2]>)> {
        use BidiClass::{CS, EN, ES, ET, L, ON, WS};

        if !text
            .chars()
            .all(|ch| matches!(bidi_class(ch), L | EN | ES | ET | CS | ON | WS))
        {
            return None;
        }
        self.select_context(system);
        let mut width = 0.0;
        let mut ink = Some([0.0_f32; 3]);
        for token in text.split_word_bounds() {
            let word = self.word(system, token, fonts)?;
            let scale = size / word.units_per_em;
            let mut advance = 0.0;
            for glyph in &word.glyphs {
                // Match the native accumulation within a word and between words.
                advance += glyph.advance[0] * scale + 0.0;
                if let Some([cursor, top, bottom]) = ink {
                    ink = system.outline_glyph(glyph.font, glyph.id).map(|outline| {
                        let [bearing, height] = outline.bounds.map_or([0.0, 0.0], |bounds| {
                            [
                                f32::from(bounds.y_max) * scale,
                                f32::from(bounds.height()) * scale,
                            ]
                        });
                        let y = cursor + glyph.offset[1] * scale - bearing;
                        [
                            cursor + glyph.advance[1] * scale,
                            top.min(y),
                            bottom.max(y + height),
                        ]
                    });
                }
            }
            width += advance;
        }
        Some((
            width,
            ink.map(|[_, top, bottom]| [top, top + (bottom - top)]),
        ))
    }

    pub(super) fn word_advance(
        &mut self,
        system: &TextSystem,
        text: &str,
        fonts: &CellFonts,
        size: f32,
    ) -> Option<f32> {
        self.select_context(system);
        let word = self.word(system, text, fonts)?;
        let scale = size / word.units_per_em;
        Some(
            word.glyphs
                .iter()
                .fold(0.0, |width, glyph| width + (glyph.advance[0] * scale + 0.0)),
        )
    }

    pub(super) fn label_word(
        &mut self,
        system: &TextSystem,
        text: &str,
        font: FontId,
    ) -> Option<&Word> {
        self.select_context(system);
        let mut fonts = [None; 8];
        fonts[0] = Some(font);
        self.word(system, text, &fonts)
    }

    fn select_context(&mut self, system: &TextSystem) {
        let context = system.context_key();
        if self
            .context
            .as_ref()
            .is_none_or(|old| !Rc::ptr_eq(old, context))
        {
            self.context = Some(context.clone());
            self.fonts.clear();
            self.words.clear();
            self.word_order.clear();
        }
    }

    fn word(&mut self, system: &TextSystem, text: &str, fonts: &CellFonts) -> Option<&Word> {
        const MAX_WORDS: usize = 4_096;
        let key = self.words.hasher().hash_one((text, fonts));
        if !self
            .words
            .get(&key)
            .is_some_and(|entry| entry.text == text && entry.fonts == *fonts)
        {
            let mut shaped = None;
            for id in fonts.iter().copied().flatten() {
                if let Some(word) = self.font(system, id)?.shape(text, id) {
                    shaped = Some(word);
                    break;
                }
            }
            // FemtoVG can search beyond the faces assigned to the paint.
            let word = shaped?;
            if self.words.contains_key(&key) {
                self.word_order.retain(|old| *old != key);
            } else if self.words.len() >= MAX_WORDS
                && let Some(old) = self.word_order.pop_front()
            {
                self.words.remove(&old);
            }
            self.word_order.push_back(key);
            self.words.insert(
                key,
                CachedWord {
                    text: text.to_owned(),
                    fonts: *fonts,
                    word,
                },
            );
        }
        self.words.get(&key).map(|entry| &entry.word)
    }

    fn font(&mut self, system: &TextSystem, id: FontId) -> Option<&mut Font> {
        const MAX_FONTS: usize = 32;
        if self.fonts.len() >= MAX_FONTS && !self.fonts.contains_key(&id) {
            self.fonts.clear();
        }
        self.fonts
            .entry(id)
            .or_insert_with(|| {
                let context = system.font_context();
                let source = context
                    .bundled
                    .as_ref()
                    .and_then(|catalog| catalog.borrow().outline_faces.get(&id).cloned())
                    .or_else(|| context.catalog.borrow().outline_faces.get(&id).cloned())?;
                let face = ParsedFace::try_new(source, |source| {
                    let face = Face::from_slice(source.bytes.as_ref(), source.index).ok_or(())?;
                    // A variation can alter both the plan and its rasterization.
                    if face.is_variable() {
                        return Err(());
                    }
                    Ok(face)
                })
                .ok()?;
                Some(Font {
                    face,
                    plans: HashMap::new(),
                    buffer: UnicodeBuffer::new(),
                    ascii_glyphs: OnceCell::new(),
                })
            })
            .as_mut()
    }
}

impl Font {
    fn independent_ascii_glyphs(&self) -> Option<&Rc<[AsciiGlyph; 128]>> {
        let face = self.face.borrow_dependent();
        self.ascii_glyphs
            .get_or_init(|| {
                independent_ascii_glyphs(face, ASCII_WITH_SEPARATORS)
                    .or_else(|| independent_ascii_glyphs(face, ASCII_ALPHANUMERIC))
                    .map(Rc::from)
            })
            .as_ref()
    }

    fn shape(&mut self, word: &str, id: FontId) -> Option<Word> {
        let face = self.face.borrow_dependent();
        let mut buffer = std::mem::take(&mut self.buffer);
        buffer.push_str(word);
        buffer.set_direction(Direction::LeftToRight);
        buffer.guess_segment_properties();
        let script = buffer.script();
        let plan = self.plans.entry(script).or_insert_with(|| {
            ShapePlan::new(face, Direction::LeftToRight, Some(script), None, &[])
        });
        let shaped = rustybuzz::shape_with_plan(face, plan, buffer);
        let positions = shaped
            .glyph_positions()
            .iter()
            .zip(shaped.glyph_infos().iter().zip(word.chars()));
        if positions.clone().any(|(_, (info, _))| info.glyph_id == 0) {
            self.buffer = shaped.clear();
            return None;
        }
        let glyphs = positions
            .map(|(position, (info, character))| Glyph {
                advance: [position.x_advance as f32, position.y_advance as f32],
                offset: [position.x_offset as f32, position.y_offset as f32],
                font: id,
                id: info.glyph_id as u16,
                drawable: !character.is_control(),
            })
            .collect();
        self.buffer = shaped.clear();
        Some(Word {
            glyphs,
            units_per_em: face.units_per_em() as f32,
            ascender: f32::from(face.ascender()),
        })
    }
}

const ASCII_ALPHANUMERIC: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const ASCII_WITH_SEPARATORS: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz -_.:,";

fn independent_ascii_glyphs(face: &Face<'_>, alphabet: &[u8]) -> Option<Box<[AsciiGlyph; 128]>> {
    use ttf_parser::{GlyphId, PlatformId, Tag, morx::SubtableKind};

    let raw = face.raw_face();
    if face.is_variable()
        || [
            b"GSUB", b"GPOS", b"GDEF", b"kern", b"kerx", b"trak", b"mort", b"ankr",
        ]
        .into_iter()
        .any(|tag| raw.table(Tag::from_bytes(tag)).is_some())
    {
        return None;
    }

    let mut glyphs = Vec::new();
    for subtable in face.tables().cmap?.subtables {
        if subtable.platform_id == PlatformId::Windows && subtable.encoding_id == 0 {
            return None;
        }
        glyphs.extend(alphabet.iter().filter_map(|&byte| {
            subtable
                .glyph_index(u32::from(byte))
                .filter(|glyph| glyph.0 != 0)
        }));
    }
    glyphs.sort_unstable();
    glyphs.dedup();

    if raw.table(Tag::from_bytes(b"morx")).is_some() {
        // Rustybuzz receives no user features here, so only the chain's
        // default horizontal subtables can apply.
        for chain in face.tables().morx.as_ref()?.chains {
            for subtable in chain.subtables {
                if subtable.feature_flags & chain.default_flags == 0
                    || subtable.coverage.is_vertical() && !subtable.coverage.is_all_directions()
                {
                    continue;
                }
                let SubtableKind::Ligature(table) = subtable.kind else {
                    return None;
                };
                if !ligatures_preserve_glyphs(&table, &glyphs) {
                    return None;
                }
            }
        }
    }

    // Check the cmap that Rustybuzz actually selects and the final position.
    // A missing character would make FemtoVG use a different font.
    let mut buffer = UnicodeBuffer::new();
    let mut chars = Box::new([AsciiGlyph::default(); 128]);
    for &byte in alphabet {
        buffer.add(char::from(byte), 0);
        buffer.set_direction(Direction::LeftToRight);
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(face, &[], buffer);
        let [glyph] = shaped.glyph_infos() else {
            return None;
        };
        let id = GlyphId(u16::try_from(glyph.glyph_id).ok()?);
        if glyphs.binary_search(&id).is_err() {
            return None;
        }
        let [position] = shaped.glyph_positions() else {
            return None;
        };
        let advance = face.glyph_hor_advance(id)?;
        if position.x_advance != i32::from(advance)
            || position.y_advance != 0
            || position.x_offset != 0
            || position.y_offset != 0
        {
            return None;
        }
        chars[usize::from(byte)] = AsciiGlyph { id: id.0, advance };
        buffer = shaped.clear();
    }
    Some(chars)
}

fn ligatures_preserve_glyphs(
    table: &ttf_parser::morx::LigatureSubtable<'_>,
    glyphs: &[ttf_parser::GlyphId],
) -> bool {
    const PERFORM_ACTION: u16 = 0x2000;
    const MAX_STATES: usize = 256;

    let mut classes: Vec<_> = glyphs
        .iter()
        .map(|&glyph| table.state.class(glyph).unwrap_or(1))
        .collect();
    classes.sort_unstable();
    classes.dedup();

    // Explore all reachable transitions, including the end of the input.
    // The state machine can keep track of components without changing them.
    let mut states = vec![0];
    let mut cursor = 0;
    while let Some(&state) = states.get(cursor) {
        cursor += 1;
        let Some(end) = table.state.entry(state, 0) else {
            return false;
        };
        if end.flags & PERFORM_ACTION != 0 {
            return false;
        }
        for &class in &classes {
            let Some(entry) = table.state.entry(state, class) else {
                return false;
            };
            if entry.flags & PERFORM_ACTION != 0 {
                return false;
            }
            if !states.contains(&entry.new_state) {
                if states.len() == MAX_STATES {
                    return false;
                }
                states.push(entry.new_state);
            }
        }
    }
    true
}

mod ascii_width_tests {
    #[test]
    fn ascii_glyphs_match_native_and_leave_kerning_to_shaping() {
        use super::{Cache, TextSystem};
        use femtovg::{Baseline, Color, Paint};

        let system = TextSystem::from_fonts([(test_font(false), 0)]).unwrap();
        let font = system.font_context().fallback[0];
        let fonts = [Some(font), None, None, None, None, None, None, None];
        let mut cache = Cache::default();
        for size in [9.125, 13.25, 33.75] {
            let mut paint = Paint::color(Color::white())
                .with_font(&[font])
                .with_font_size(size);
            paint.set_text_baseline(Baseline::Alphabetic);
            let metrics = cache.ascii_metrics(&system, size, font).unwrap();
            for text in ["AV9", "release-0000F2a", "a.b:0,2 -3", "9 - 90", " A  b_1 "] {
                let native = system
                    .context()
                    .measure_text(0.0, 0.0, text, &paint)
                    .unwrap();
                let (width, ink) = cache
                    .ltr_width_and_ink(&system, text, &fonts, size)
                    .unwrap();
                assert_eq!(width.to_bits(), native.width().to_bits(), "{text:?}");
                assert_eq!(
                    ink,
                    Some([native.y, native.y + native.height()]),
                    "{text:?}"
                );
                let cached = metrics.width(text).unwrap();
                assert_eq!(
                    cached.to_bits(),
                    native.width().to_bits(),
                    "{text:?} at {size}"
                );
                let cached = metrics
                    .word(text, |font, id, advance| (font, id, advance.to_bits()))
                    .unwrap();
                assert_eq!(cached.len(), native.glyphs.len(), "{text:?} at {size}");
                for (cached, glyph) in cached.into_iter().zip(native.glyphs) {
                    assert_eq!(
                        cached,
                        (glyph.font_id, glyph.glyph_id, glyph.advance_x.to_bits())
                    );
                    assert_eq!([glyph.advance_y, glyph.offset_x, glyph.offset_y], [0.0; 3]);
                }
            }
        }
        assert!(
            cache
                .ascii_metrics(&system, 12.0, font)
                .unwrap()
                .width("ترقيم A")
                .is_none()
        );
        assert!(
            cache
                .ltr_width_and_ink(&system, "A שלום", &fonts, 16.0)
                .is_none()
        );
        let system = TextSystem::from_fonts([(test_font(true), 0)]).unwrap();
        let font = system.font_context().fallback[0];
        assert!(
            system
                .font_context()
                .catalog
                .borrow()
                .outline_faces
                .contains_key(&font)
        );
        let paint = Paint::color(Color::white())
            .with_font(&[font])
            .with_font_size(16.0);
        let native = |text| {
            system
                .context()
                .measure_text(0.0, 0.0, text, &paint)
                .unwrap()
                .width()
        };
        assert!(native("AV") < native("A") + native("V"));
        assert!(
            Cache::default()
                .ascii_metrics(&system, 16.0, font)
                .is_none()
        );
        fn test_font(with_kerning: bool) -> Vec<u8> {
            const GLYPHS: u16 = 128;
            let mut head = vec![0; 54];
            head[..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
            head[18..20].copy_from_slice(&1000u16.to_be_bytes());
            let mut hhea = vec![0; 36];
            hhea[..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
            hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
            hhea[34..36].copy_from_slice(&GLYPHS.to_be_bytes());
            let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
            maxp.extend(GLYPHS.to_be_bytes());
            let mut hmtx = Vec::new();
            for glyph in 0..GLYPHS {
                hmtx.extend((400 + glyph * 7 % 101).to_be_bytes());
                hmtx.extend(0i16.to_be_bytes());
            }
            // Windows Unicode BMP cmap containing a single format 0 subtable.
            let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 1, 0, 0, 0, 12, 0, 0, 1, 6, 0, 0];
            cmap.extend((0u8..=255).map(|byte| if byte.is_ascii() { byte } else { 0 }));
            // Font discovery requires family and PostScript names.
            let mut name = vec![0, 0, 0, 2, 0, 30];
            let mut strings = Vec::new();
            for (id, value) in [(1u16, "XRack Width Test"), (6u16, "XRackWidthTest")] {
                name.extend([0, 3, 0, 1, 4, 9]);
                name.extend(id.to_be_bytes());
                name.extend(((value.len() * 2) as u16).to_be_bytes());
                name.extend((strings.len() as u16).to_be_bytes());
                strings.extend(value.encode_utf16().flat_map(u16::to_be_bytes));
            }
            name.extend(strings);
            let mut tables = vec![
                (*b"cmap", cmap),
                (*b"head", head),
                (*b"hhea", hhea),
                (*b"hmtx", hmtx),
                (*b"maxp", maxp),
                (*b"name", name),
            ];
            if with_kerning {
                // Horizontal format 0 kerning with a single AV pair.
                let mut kern = vec![0, 0, 0, 1, 0, 0, 0, 20, 0, 1, 0, 1, 0, 6, 0, 0, 0, 0];
                kern.extend([0, b'A', 0, b'V']);
                kern.extend((-150i16).to_be_bytes());
                tables.push((*b"kern", kern));
            }
            tables.sort_unstable_by_key(|(tag, _)| *tag);
            let mut data = vec![0; 12 + 16 * tables.len()];
            data[..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
            data[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
            for (index, (tag, table)) in tables.into_iter().enumerate() {
                let offset = data.len() as u32;
                let record = 12 + index * 16;
                data[record..record + 4].copy_from_slice(&tag);
                data[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
                data[record + 12..record + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
                data.extend(table);
                data.resize(data.len().next_multiple_of(4), 0);
            }
            data
        }
    }
}
