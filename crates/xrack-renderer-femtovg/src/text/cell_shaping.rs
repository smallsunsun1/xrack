use super::{CellFonts, FontSource, TextSystem};
use crate::cell_glyphs::{Glyph, Run};
use femtovg::FontId;
use rustybuzz::{Direction, Face, Script, ShapePlan, UnicodeBuffer};
use std::{
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
}

struct Word {
    glyphs: Vec<Glyph>,
    units_per_em: f32,
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
                })
            })
            .as_mut()
    }
}

impl Font {
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
