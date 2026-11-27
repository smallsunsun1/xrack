use std::{
    collections::{HashMap, VecDeque},
    ops::Range,
    sync::{Arc, Mutex, OnceLock},
};

use fontdb::{Database, Family, ID, Query, Source, Style, Weight};
use rustybuzz::{Direction, UnicodeBuffer};
use unicode_bidi::BidiInfo;
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

use crate::{EmbeddedFonts, Font, LoadedFonts};

/// Fonts imported by the library that owns a geometry binding.
#[doc(hidden)]
#[derive(Clone, Copy, Default, Debug)]
pub enum SceneTextFonts<'a> {
    #[default]
    System,
    Embedded(&'static EmbeddedFonts),
    Loaded(&'a LoadedFonts),
}

#[derive(Clone, PartialEq)]
pub(super) enum FontSources {
    System,
    Embedded(&'static EmbeddedFonts),
    Loaded(LoadedFonts),
}

impl SceneTextFonts<'_> {
    pub(super) fn owned(self) -> FontSources {
        match self {
            Self::System => FontSources::System,
            Self::Embedded(fonts) if !fonts.sources().is_empty() => FontSources::Embedded(fonts),
            Self::Loaded(fonts) if !fonts.sources().is_empty() => {
                FontSources::Loaded(fonts.clone())
            }
            _ => FontSources::System,
        }
    }
}

impl FontSources {
    pub(super) fn embedded(&self) -> Option<Arc<Database>> {
        if matches!(self, Self::System) {
            return None;
        }
        static CATALOGS: Mutex<VecDeque<(FontSources, Arc<Database>)>> =
            Mutex::new(VecDeque::new());
        if let Ok(mut catalogs) = CATALOGS.lock()
            && let Some(index) = catalogs.iter().position(|(source, _)| source == self)
            && let Some(entry) = catalogs.remove(index)
        {
            let catalog = entry.1.clone();
            catalogs.push_front(entry);
            return Some(catalog);
        }
        let mut catalog = Database::new();
        match self {
            Self::Embedded(fonts) => {
                for &bytes in fonts.sources() {
                    catalog.load_font_source(Source::Binary(Arc::new(bytes)));
                }
            }
            Self::Loaded(fonts) => {
                for bytes in fonts.sources() {
                    catalog.load_font_source(Source::Binary(bytes.clone()));
                }
            }
            Self::System => {}
        }
        let catalog = Arc::new(catalog);
        if let Ok(mut catalogs) = CATALOGS.lock() {
            catalogs.truncate(5);
            catalogs.push_front((self.clone(), catalog.clone()));
        }
        Some(catalog)
    }
}

fn system() -> &'static Database {
    static SYSTEM: OnceLock<Database> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let mut catalog = Database::new();
        catalog.load_system_fonts();
        catalog
    })
}

#[derive(Clone, Copy)]
struct FaceRef<'a> {
    catalog: &'a Database,
    id: ID,
}

impl PartialEq for FaceRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.catalog, other.catalog) && self.id == other.id
    }
}

impl FaceRef<'_> {
    fn covers(self, text: &str) -> bool {
        self.catalog
            .with_face_data(self.id, |bytes, index| {
                let Ok(face) = ttf_parser::Face::parse(bytes, index) else {
                    return false;
                };
                text.chars().all(|character| {
                    character.is_whitespace()
                        || matches!(character, '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{fe0e}'..='\u{fe0f}')
                        || face.glyph_index(character).is_some()
                })
            })
            .unwrap_or(false)
    }
}

struct FontSelection<'a> {
    embedded: Option<&'a Database>,
    request: &'a Font,
}

impl<'a> FontSelection<'a> {
    fn family(&self) -> Family<'a> {
        match self.request.family.trim() {
            "" | "sans-serif" => Family::SansSerif,
            "serif" => Family::Serif,
            "monospace" => Family::Monospace,
            family => Family::Name(family),
        }
    }

    fn query(&self, catalog: &'a Database, families: &[Family<'_>]) -> Option<FaceRef<'a>> {
        catalog
            .query(&Query {
                families,
                weight: Weight(self.request.weight.clamp(1, 1000) as u16),
                style: if self.request.italic {
                    Style::Italic
                } else {
                    Style::Normal
                },
                ..Default::default()
            })
            .map(|id| FaceRef { catalog, id })
    }

    fn preferred(&self, catalog: &'a Database) -> Option<FaceRef<'a>> {
        self.query(
            catalog,
            &[
                self.family(),
                Family::SansSerif,
                Family::Name("Arial"),
                Family::Name("DejaVu Sans"),
                Family::Name("Noto Sans"),
                Family::Name("Segoe UI"),
            ],
        )
        .or_else(|| {
            catalog.faces().next().map(|face| FaceRef {
                catalog,
                id: face.id,
            })
        })
    }

    fn covering(&self, text: &str) -> Option<FaceRef<'a>> {
        let family = self.family();
        if let Some(catalog) = self.embedded
            && let Some(face) = self
                .query(catalog, &[family])
                .filter(|face| face.covers(text))
        {
            return Some(face);
        }
        let system = system();
        if let Some(face) = self
            .query(system, &[family])
            .filter(|face| face.covers(text))
        {
            return Some(face);
        }
        self.embedded
            .and_then(|catalog| self.find_in(catalog, text))
            .or_else(|| self.find_in(system, text))
    }

    fn find_in(&self, catalog: &'a Database, text: &str) -> Option<FaceRef<'a>> {
        self.preferred(catalog)
            .filter(|face| face.covers(text))
            .or_else(|| {
                catalog
                    .faces()
                    .take(192)
                    .map(|face| FaceRef {
                        catalog,
                        id: face.id,
                    })
                    .find(|face| face.covers(text))
            })
    }
}

/// Shape visual runs with their own script and font, preserving clusters when falling back.
pub(super) fn shape(
    text: &str,
    font: &Font,
    embedded: Option<&Database>,
    mut glyph: impl FnMut(&ttf_parser::Face<'_>, ttf_parser::GlyphId, [f32; 2], f32) -> Option<()>,
) -> Option<()> {
    let fonts = FontSelection {
        embedded,
        request: font,
    };
    let mut glyphs = 0;
    let mut fallback_faces = HashMap::new();
    for (line_number, line) in text.split('\n').enumerate() {
        let mut pen = [0.0, -1.25 * line_number as f32];
        let bidi = BidiInfo::new(line, None);
        for paragraph in &bidi.paragraphs {
            let (levels, visual) = bidi.visual_runs(paragraph, paragraph.range.clone());
            for visual_run in visual {
                let rtl = levels[visual_run.start].is_rtl();
                let visual_text = &line[visual_run];
                let mut scripts = scripts(visual_text);
                if rtl {
                    scripts.reverse();
                }
                for script in scripts {
                    let run = &visual_text[script];
                    let mut faces = Vec::new();
                    if let Some(face) = fonts.covering(run) {
                        faces.push((0..run.len(), face));
                    } else {
                        for (start, cluster) in run.grapheme_indices(true) {
                            let face = (*fallback_faces.entry(cluster).or_insert_with(|| {
                                fonts
                                    .covering(cluster)
                                    .or_else(|| fonts.embedded.and_then(|db| fonts.preferred(db)))
                                    .or_else(|| fonts.preferred(system()))
                            }))?;
                            let end = start + cluster.len();
                            if let Some((range, previous)) = faces.last_mut()
                                && *previous == face
                            {
                                range.end = end;
                            } else {
                                faces.push((start..end, face));
                            }
                        }
                    }
                    if rtl {
                        faces.reverse();
                    }
                    for (range, source) in faces {
                        source.catalog.with_face_data(source.id, |bytes, index| {
                            let face = ttf_parser::Face::parse(bytes, index).ok()?;
                            let shaper = rustybuzz::Face::from_slice(bytes, index)?;
                            let scale = 1.0 / f32::from(face.units_per_em());
                            let mut buffer = UnicodeBuffer::new();
                            buffer.push_str(&run[range]);
                            buffer.set_direction(if rtl {
                                Direction::RightToLeft
                            } else {
                                Direction::LeftToRight
                            });
                            buffer.guess_segment_properties();
                            let result = rustybuzz::shape(&shaper, &[], buffer);
                            for (info, position) in
                                result.glyph_infos().iter().zip(result.glyph_positions())
                            {
                                glyphs += 1;
                                if glyphs > super::MAX_GLYPHS {
                                    return None;
                                }
                                let origin = [
                                    pen[0] + position.x_offset as f32 * scale,
                                    pen[1] + position.y_offset as f32 * scale,
                                ];
                                let id = ttf_parser::GlyphId(u16::try_from(info.glyph_id).ok()?);
                                glyph(&face, id, origin, scale)?;
                                pen[0] += position.x_advance as f32 * scale;
                                pen[1] += position.y_advance as f32 * scale;
                            }
                            Some(())
                        })??;
                    }
                }
            }
        }
    }
    Some(())
}

fn scripts(text: &str) -> Vec<Range<usize>> {
    let mut start = 0;
    let mut previous = None;
    let mut result = Vec::new();
    for (index, cluster) in text.grapheme_indices(true) {
        let Some(script) = cluster
            .chars()
            .map(|character| character.script())
            .find(|script| !matches!(script, Script::Common | Script::Inherited | Script::Unknown))
        else {
            continue;
        };
        if previous.is_some_and(|before| before != script) {
            result.push(start..index);
            start = index;
        }
        previous = Some(script);
    }
    result.push(start..text.len());
    result
}
