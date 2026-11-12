//! Resolve assets and localization catalogs referenced by XR dialect modules.
mod animated_image;
mod assets;
mod localization;
mod scene_plan;
pub mod visual;

pub use assets::{ResolvedAssets, collect_vector_paths, resolve_assets};
pub use localization::{ResolvedTranslations, resolve_translations};
pub use scene_plan::ScenePlan;
use std::{collections::BTreeMap, path::Path};
use xrack_mlir::{Module, OperationRef};
use xrack_runtime::{ImageFormat, NineSlice, StyledText, VectorSegment};

pub enum EmbeddedImage {
    Static {
        format: ImageFormat,
        size: [u32; 2],
        bytes: Vec<u8>,
    },
    Animated {
        size: [u32; 2],
        frames: Vec<(u32, Vec<u8>)>,
        plays: u32,
    },
}

impl EmbeddedImage {
    pub fn into_image_asset(self) -> xrack_runtime::ImageAsset {
        use xrack_runtime::{ImageAsset, LoadedAnimation, LoadedImage};
        match self {
            Self::Static {
                format,
                size,
                bytes,
            } => ImageAsset::Loaded(LoadedImage::new(format, bytes, size)),
            Self::Animated {
                size,
                frames,
                plays,
            } => ImageAsset::Animated(std::rc::Rc::new(LoadedAnimation::new(size, frames, plays))),
        }
    }

    pub fn size(&self) -> [u32; 2] {
        match self {
            Self::Static { size, .. } | Self::Animated { size, .. } => *size,
        }
    }
}

pub struct EmbeddedPath {
    pub bounds: [f32; 4],
    pub segments: Vec<VectorSegment>,
}

pub struct EmbeddedTranslations {
    pub default: String,
    pub locales: BTreeMap<String, String>,
}

pub fn nine_slice_edges(op: OperationRef<'_>) -> Result<NineSlice, String> {
    let [top, right, bottom, left] = xrack_mlir::nine_slice_edges(op)?;
    Ok(NineSlice {
        top,
        right,
        bottom,
        left,
    })
}

pub fn check_nine_slices(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
) -> Result<(), String> {
    fn check(op: OperationRef<'_>, images: &BTreeMap<String, EmbeddedImage>) -> Result<(), String> {
        if op.name() == "xr.nine_slice" {
            let url = op
                .operand(0)
                .and_then(|value| value.defining_operation())
                .ok_or("missing nine-slice source")?
                .attr("value")?;
            let image = images
                .get(url)
                .ok_or_else(|| format!("unresolved image {url:?}"))?;
            if matches!(image, EmbeddedImage::Animated { .. }) {
                return Err("nine-slice requires a static PNG or SVG source".into());
            }
            if !nine_slice_edges(op)?.fits(image.size()) {
                let span = op.span();
                let source = op.source_file().unwrap_or("<unknown>");
                let [width, height] = image.size();
                return Err(format!(
                    "{source}:{}:{}: nine-slice edges must leave a nonempty center in image {url:?} ({width} × {height})",
                    span.line, span.column
                ));
            }
        }
        for child in op.children() {
            check(child, images)?;
        }
        Ok(())
    }
    for op in module.operations() {
        check(op, images)?;
    }
    Ok(())
}

/// Validate and parse each distinct Markdown literal in an XR module.
pub fn collect_markdown(module: &Module) -> Result<BTreeMap<&str, StyledText>, String> {
    let mut texts = BTreeMap::new();
    for op in module.operations() {
        collect_in(op, &mut texts)?;
    }
    Ok(texts)
}

fn collect_in<'m>(
    op: OperationRef<'m>,
    texts: &mut BTreeMap<&'m str, StyledText>,
) -> Result<(), String> {
    if op.name() == "xr.markdown"
        && let std::collections::btree_map::Entry::Vacant(entry) = texts.entry(op.attr("value")?)
    {
        let text = StyledText::from_markdown(entry.key()).map_err(|error| {
            let file = op.source_file().unwrap_or("<unknown>");
            let span = op.span();
            format!("{file}:{}:{}: {error}", span.line, span.column)
        })?;
        entry.insert(text);
    }
    for child in op.children() {
        collect_in(child, texts)?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareMlirError {
    #[error("{0}")]
    Mlir(String),
    #[error("{0}")]
    Resources(String),
}

/// Parse XR dialect text and validate its translations before optimization.
pub fn prepare_mlir(
    source: &str,
    filename: &Path,
) -> Result<(Module, ResolvedTranslations), PrepareMlirError> {
    let module = Module::parse(source).map_err(PrepareMlirError::Mlir)?;
    let translations = resolve_translations(&module, filename, None, &BTreeMap::new())
        .map_err(PrepareMlirError::Resources)?;
    let module = module.optimize().map_err(PrepareMlirError::Mlir)?;
    Ok((module, translations))
}
