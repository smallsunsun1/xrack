//! Shaped outline text converted to the same triangle meshes as other geometry.

mod fonts;
mod jobs;
mod outline;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use fonts::FontSources;
pub use fonts::SceneTextFonts;
pub(super) use jobs::{TextRequest, poll_text_jobs, text_jobs_pending};
use outline::{FilledOutline, Outlines, extrude_fill};

use crate::{Font, scene3d::Mesh3D};

const MAX_TEXT_BYTES: usize = 4096;
const MAX_GLYPHS: usize = 256;
const MAX_TEXT_VERTICES: usize = 180_000;
const CACHE_VERTICES: usize = 360_000;
const CACHE_ENTRIES: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GeometryStatus {
    #[default]
    Empty,
    Loading,
    Ready,
    Error(&'static str),
}

impl GeometryStatus {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Error(_) => "error",
        }
    }

    pub(crate) fn error(self) -> &'static str {
        match self {
            Self::Error(error) => error,
            _ => "",
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct PreparedText {
    pub(crate) mesh: Option<Mesh3D>,
    pub(crate) status: GeometryStatus,
}

impl PreparedText {
    fn loading() -> Self {
        Self {
            status: GeometryStatus::Loading,
            mesh: None,
        }
    }

    fn error(error: &'static str) -> Self {
        Self {
            status: GeometryStatus::Error(error),
            mesh: None,
        }
    }

    fn from_mesh(mesh: Option<Mesh3D>) -> Self {
        if mesh.is_some() {
            Self {
                status: GeometryStatus::Ready,
                mesh,
            }
        } else {
            Self::error("Text did not produce a usable outline mesh")
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct TextGeometry<'a> {
    pub text: &'a str,
    pub font: Option<&'a Font>,
    pub scale: f64,
    pub depth: f64,
    pub fonts: SceneTextFonts<'a>,
}

#[derive(Clone, PartialEq)]
struct ShapeRecipe {
    text: String,
    font: Font,
    fonts: FontSources,
}

#[derive(Clone, PartialEq)]
struct TextRecipe {
    outline: ShapeRecipe,
    scale: f32,
    depth: f32,
}

struct CachedText {
    recipe: TextRecipe,
    mesh: Mesh3D,
}

#[derive(Default)]
struct GeometryCache {
    entries: VecDeque<CachedText>,
    vertices: usize,
}

impl GeometryCache {
    const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            vertices: 0,
        }
    }

    fn get(&mut self, recipe: &TextRecipe) -> Option<Mesh3D> {
        let index = self
            .entries
            .iter()
            .position(|entry| &entry.recipe == recipe)?;
        let entry = self.entries.remove(index)?;
        let mesh = entry.mesh.clone();
        self.entries.push_front(entry);
        Some(mesh)
    }

    fn insert(&mut self, recipe: TextRecipe, mesh: Mesh3D) -> Option<()> {
        let vertices = mesh.vertices().len();
        while self.entries.len() >= CACHE_ENTRIES || self.vertices + vertices > CACHE_VERTICES {
            self.vertices -= self.entries.pop_back()?.mesh.vertices().len();
        }
        self.vertices += vertices;
        self.entries.push_front(CachedText { recipe, mesh });
        Some(())
    }
}

impl ShapeRecipe {
    fn fill(&self) -> Option<Arc<FilledOutline>> {
        static OUTLINES: Mutex<VecDeque<(ShapeRecipe, Arc<FilledOutline>)>> =
            Mutex::new(VecDeque::new());
        if let Ok(mut cache) = OUTLINES.lock()
            && let Some(index) = cache.iter().position(|(recipe, _)| recipe == self)
            && let Some(entry) = cache.remove(index)
        {
            let fill = entry.1.clone();
            cache.push_front(entry);
            return Some(fill);
        }
        let mut outline = Outlines::new();
        let tolerance = (0.25 / self.font.size.0 as f32).clamp(0.0003, 0.025);
        let embedded = self.fonts.embedded();
        fonts::shape(
            &self.text,
            &self.font,
            embedded.as_deref(),
            |face, glyph, origin, scale| outline.add_glyph(face, glyph, origin, scale, tolerance),
        )?;
        let fill = Arc::new(outline.fill()?);
        if let Ok(mut cache) = OUTLINES.lock() {
            cache.truncate(7);
            cache.push_front((self.clone(), fill.clone()));
        }
        Some(fill)
    }
}

static CACHE: Mutex<GeometryCache> = Mutex::new(GeometryCache::new());

impl TextRecipe {
    fn cached(&self) -> Option<Mesh3D> {
        CACHE.lock().ok().and_then(|mut cache| cache.get(self))
    }

    fn mesh(self) -> Option<Mesh3D> {
        if let Some(mesh) = self.cached() {
            return Some(mesh);
        }
        let outline = self.outline.fill()?;
        let mesh = Mesh3D::prepare_procedural(extrude_fill(&outline, self.scale, self.depth)?)?;
        if let Ok(mut cache) = CACHE.lock() {
            if let Some(existing) = cache.get(&self) {
                return Some(existing);
            }
            cache.insert(self, mesh.clone())?;
        }
        Some(mesh)
    }
}

impl TextGeometry<'_> {
    pub(super) fn is_valid(self) -> bool {
        self.text.len() <= MAX_TEXT_BYTES
            && (0.001..=200.0).contains(&self.scale)
            && (0.0..=200.0).contains(&self.depth)
            && self
                .font
                .is_none_or(|font| (1.0..=4096.0).contains(&font.size.0))
    }

    fn recipe(self) -> TextRecipe {
        TextRecipe {
            outline: ShapeRecipe {
                text: self.text.into(),
                font: self.font.cloned().unwrap_or_default(),
                fonts: self.fonts.owned(),
            },
            scale: self.scale as f32,
            depth: self.depth as f32,
        }
    }

    pub(super) fn prepare(
        self,
        asynchronous: bool,
        inputs: &super::authored::SceneMeshInputs,
    ) -> PreparedText {
        if !asynchronous {
            return self.prepare_sync();
        }
        if !self.is_valid() {
            return PreparedText::error("Invalid text, font size, scale or depth");
        }
        if self.text.trim().is_empty() {
            return PreparedText::default();
        }
        jobs::prepare(self.recipe(), inputs)
    }

    pub(super) fn prepare_sync(self) -> PreparedText {
        if !self.is_valid() {
            PreparedText::error("Invalid text, font size, scale or depth")
        } else if self.text.trim().is_empty() {
            PreparedText::default()
        } else {
            PreparedText::from_mesh(self.recipe().mesh())
        }
    }
}
