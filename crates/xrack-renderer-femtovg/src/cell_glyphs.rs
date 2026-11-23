use femtovg::{FontId, TextMetrics};
use std::{
    collections::{HashMap, VecDeque},
    hash::BuildHasher,
};

// Canvas text measurement quantizes the transform and then applies DPI.
pub(super) fn measurement_scale(device_scale: f32) -> f32 {
    ((device_scale / 0.1 + 0.5).trunc() * 0.1).min(7.0) * device_scale
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Settings {
    pub(super) font_size: u32,
    pub(super) measurement_scale: u32,
    pub(super) fonts: [usize; 2],
}

#[derive(Clone, Copy)]
struct Glyph {
    advance: [f32; 2],
    offset: [f32; 2],
    font: FontId,
    id: u16,
    drawable: bool,
}

struct Entry {
    text: String,
    settings: Settings,
    run: Run,
}

struct Run {
    baseline: f32,
    glyphs: Vec<Glyph>,
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

fn placed_glyphs(
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
pub(super) struct Cache {
    entries: HashMap<u64, Entry>,
    order: VecDeque<u64>,
    recent: Vec<u64>,
}

impl Cache {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.recent.fill(0);
    }

    pub(super) fn visit(
        &mut self,
        text: &str,
        settings: Settings,
        origin: [f32; 2],
        measure: impl FnOnce() -> Option<TextMetrics>,
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
        // One-off cells encountered during a fast scroll only leave a hash.
        if self.recent.is_empty() {
            self.recent.resize(RECENT_ENTRIES, 0);
        }
        let recent = &mut self.recent[key as usize % RECENT_ENTRIES];
        if std::mem::replace(recent, key) == key {
            self.insert(key, text, settings, &metrics);
        }
        placed_glyphs(measured_glyphs(&metrics), metrics.baseline(), origin, scale).all(visit)
    }

    fn insert(&mut self, key: u64, text: &str, settings: Settings, metrics: &TextMetrics) {
        const MAX_ENTRIES: usize = 2048;
        let run = Run {
            baseline: metrics.baseline(),
            glyphs: measured_glyphs(metrics).collect(),
        };
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
            },
        );
    }
}
