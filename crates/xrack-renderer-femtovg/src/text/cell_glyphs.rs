use femtovg::{FontId, TextMetrics};
use std::{
    collections::{HashMap, VecDeque},
    hash::BuildHasher,
};

// Canvas text measurement quantizes the transform and then applies DPI.
pub(crate) fn measurement_scale(device_scale: f32) -> f32 {
    ((device_scale / 0.1 + 0.5).trunc() * 0.1).min(7.0) * device_scale
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Settings {
    pub(crate) font_size: u32,
    pub(crate) measurement_scale: u32,
    pub(crate) fonts: [usize; 2],
    pub(crate) cell_fonts: Option<crate::text::CellFonts>,
}

#[derive(Clone, Copy)]
pub(crate) struct Glyph {
    pub(crate) advance: [f32; 2],
    pub(crate) offset: [f32; 2],
    pub(crate) font: FontId,
    pub(crate) id: u16,
    pub(crate) drawable: bool,
}

struct Entry {
    text: String,
    settings: Settings,
    run: Run,
    portable: bool,
}

pub(crate) struct Run {
    pub(crate) baseline: f32,
    pub(crate) glyphs: Vec<Glyph>,
}

impl Run {
    pub(crate) fn from_metrics(metrics: &TextMetrics) -> Self {
        Self {
            baseline: metrics.baseline(),
            glyphs: measured_glyphs(metrics).collect(),
        }
    }
}

#[cfg(xrack_femtovg_glyph_samples)]
type MeasuredCell = (f32, Vec<femtovg::ShapedGlyph>);
#[cfg(not(xrack_femtovg_glyph_samples))]
type MeasuredCell = TextMetrics;

pub(crate) enum Measurement {
    Legacy(MeasuredCell),
    Portable(Run),
}

fn measured_glyphs(metrics: &TextMetrics) -> impl Iterator<Item = Glyph> + '_ {
    metrics.glyphs.iter().map(|glyph| Glyph {
        advance: [glyph.advance_x, glyph.advance_y],
        offset: [glyph.offset_x, glyph.offset_y],
        font: glyph.font_id,
        id: glyph.glyph_id,
        drawable: !glyph.c.is_control(),
    })
}

pub(crate) fn placed_glyphs(
    glyphs: impl Iterator<Item = Glyph>,
    baseline: f32,
    origin: [f32; 2],
    scale: f32,
) -> impl Iterator<Item = (FontId, u16, f32, f32)> {
    // Keep the original shaping-space addition order, including vertical
    // advances and combining-mark offsets, before converting to user space.
    let [mut x, mut y] = origin.map(|position| position * scale);
    let inverse = 1.0 / scale;
    glyphs.filter_map(move |glyph| {
        let px = (x + glyph.offset[0]) * inverse;
        let py = (y + baseline + glyph.offset[1]) * inverse;
        x += glyph.advance[0];
        y += glyph.advance[1];
        glyph.drawable.then_some((glyph.font, glyph.id, px, py))
    })
}

#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<u64, Entry>,
    order: VecDeque<u64>,
    recent: Vec<u64>,
}

impl Cache {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.recent.fill(0);
    }

    pub(crate) fn visit(
        &mut self,
        text: &str,
        settings: Settings,
        origin: [f32; 2],
        measure: impl FnOnce() -> Option<Measurement>,
        mut visit: impl FnMut(FontId, u16, f32, f32) -> bool,
    ) -> bool {
        const RECENT_ENTRIES: usize = 2048;
        let scale = f32::from_bits(settings.measurement_scale);
        let visit = |(font, id, x, y)| visit(font, id, x, y);
        let key = self.entries.hasher().hash_one((text, settings));
        if let Some(entry) = self.entries.get(&key)
            && entry.text == text
            && entry.settings == settings
        {
            let run = &entry.run;
            return placed_glyphs(run.glyphs.iter().copied(), run.baseline, origin, scale)
                .all(visit);
        }
        let Some(metrics) = measure() else {
            return false;
        };
        match metrics {
            Measurement::Legacy(metrics) => {
                // One-off cells encountered during a fast scroll only leave a hash.
                if self.recent.is_empty() {
                    self.recent.resize(RECENT_ENTRIES, 0);
                }
                #[cfg(xrack_femtovg_glyph_samples)]
                let (baseline, glyphs) = (metrics.0, metrics.1.as_slice());
                #[cfg(not(xrack_femtovg_glyph_samples))]
                let (baseline, glyphs) = (metrics.baseline(), metrics.glyphs.as_slice());
                let glyphs = glyphs.iter().map(|glyph| Glyph {
                    advance: [glyph.advance_x, glyph.advance_y],
                    offset: [glyph.offset_x, glyph.offset_y],
                    font: glyph.font_id,
                    id: glyph.glyph_id,
                    drawable: !glyph.c.is_control(),
                });
                let recent = &mut self.recent[key as usize % RECENT_ENTRIES];
                if std::mem::replace(recent, key) == key {
                    let run = Run {
                        baseline,
                        glyphs: glyphs.clone().collect(),
                    };
                    self.insert(key, text, settings, run, false);
                }
                placed_glyphs(glyphs, baseline, origin, scale).all(visit)
            }
            Measurement::Portable(run) => {
                let result = placed_glyphs(run.glyphs.iter().copied(), run.baseline, origin, scale)
                    .all(visit);
                // The scalar draw can reuse this run immediately when a mask is missing.
                self.insert(key, text, settings, run, true);
                result
            }
        }
    }

    pub(crate) fn portable_run(
        &mut self,
        text: &str,
        settings: Settings,
        shape: impl FnOnce() -> Option<Run>,
    ) -> Option<&Run> {
        let key = self.entries.hasher().hash_one((text, settings));
        if self
            .entries
            .get(&key)
            .is_none_or(|entry| entry.text != text || entry.settings != settings)
        {
            let run = shape()?;
            self.insert(key, text, settings, run, true);
        }
        self.entries
            .get(&key)
            .filter(|entry| entry.portable)
            .map(|entry| &entry.run)
    }

    fn insert(&mut self, key: u64, text: &str, settings: Settings, run: Run, portable: bool) {
        const MAX_ENTRIES: usize = 2048;
        if self.entries.contains_key(&key) {
            self.order.retain(|previous| *previous != key);
        } else if self.entries.len() >= MAX_ENTRIES
            && let Some(oldest) = self.order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        self.order.push_back(key);
        self.entries.insert(
            key,
            Entry {
                text: text.to_owned(),
                settings,
                run,
                portable,
            },
        );
    }
}
