//! Resolve assets and localization catalogs referenced by XR dialect modules.
mod animated_image;
mod assets;
pub mod field;
pub mod literal;
mod localization;
mod scene_plan;
pub mod visual;

pub use assets::{
    ResolvedAssets, check_font_sources, check_image_sources, check_loaded_image_sources,
    collect_missing_vector_paths, collect_vector_paths, resolve_assets,
};
pub use localization::{
    ResolvedTranslations, check_missing_translation_catalog, check_translation_sources,
    resolve_translations,
};
pub use scene_plan::ScenePlan;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use xrack_mlir::{Module, OperationRef};
use xrack_runtime::{Image, ImageAsset, ImageFormat, NineSlice, StyledText, VectorSegment};

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

impl EmbeddedTranslations {
    pub fn sources(&self) -> impl Iterator<Item = (&str, &str, bool)> {
        self.locales
            .iter()
            .map(|(locale, text)| (locale.as_str(), text.as_str(), *locale == self.default))
    }
}

/// Image representations accepted by the two XR backends.
pub enum ResourceImages<'a> {
    Embedded(&'a BTreeMap<String, EmbeddedImage>),
    Loaded(&'a BTreeMap<String, ImageAsset>),
}

/// Validate the resources for a verified module in the same order for both backends.
pub fn validate_module_resources<'a>(
    module: &Module,
    images: ResourceImages<'_>,
    fonts: impl ExactSizeIterator<Item = &'a [u8]>,
    translations: Option<impl Iterator<Item = (impl std::fmt::Display, &'a str, bool)>>,
    validate_asset_data: bool,
) -> Result<(), String> {
    match &images {
        ResourceImages::Embedded(images) => check_nine_slices(module, images)?,
        ResourceImages::Loaded(images) => check_loaded_nine_slices(module, images)?,
    }
    check_resource_bindings(
        module,
        |url| match &images {
            ResourceImages::Embedded(images) => images.contains_key(url),
            ResourceImages::Loaded(images) => images.contains_key(url),
        },
        fonts.len() != 0,
        translations.is_some(),
    )?;
    if validate_asset_data {
        match images {
            ResourceImages::Embedded(images) => check_image_sources(images)?,
            ResourceImages::Loaded(images) => check_loaded_image_sources(images)?,
        }
        check_font_sources(fonts)?;
    }
    if let Some(sources) = translations {
        check_translation_sources(module, sources)?;
    }
    Ok(())
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
    check_nine_slice_sources(module, |url| {
        images.get(url).map(|image| {
            (
                image.size(),
                matches!(image, EmbeddedImage::Animated { .. }),
            )
        })
    })
}

/// Check that callers supplied the resources required by a verified module.
pub fn check_resource_bindings(
    module: &Module,
    has_image: impl Fn(&str) -> bool,
    has_fonts: bool,
    has_translations: bool,
) -> Result<(), String> {
    if !has_fonts && let Some(font) = module.operations().find(|op| op.name() == "xr.font") {
        return Err(format!("unresolved font {:?}", font.attr("path")?));
    }
    let mut check = |op: OperationRef<'_>| {
        match op.name() {
            "xr.image_url" => {
                let url = op.attr("value")?;
                if !url.is_empty() && !has_image(url) {
                    return Err(format!("unresolved image {url:?}"));
                }
            }
            "xr.translate" if !has_translations => {
                return Err(localization::MISSING_TRANSLATION_CATALOG.into());
            }
            _ => {}
        }
        Ok(())
    };
    for op in module.operations() {
        assets::walk(op, &mut check)?;
    }
    if !has_translations {
        localization::check_catalog_declaration(module)?;
    }
    Ok(())
}

/// Check the same constraints when a host supplies decoded image assets.
pub fn check_loaded_nine_slices(
    module: &Module,
    images: &BTreeMap<String, ImageAsset>,
) -> Result<(), String> {
    check_nine_slice_sources(module, |url| {
        images.get(url).map(|image| {
            (
                image.size(),
                matches!(
                    image,
                    ImageAsset::Animated(_) | ImageAsset::Embedded(Image::Animated(_))
                ),
            )
        })
    })
}

fn check_nine_slice_sources(
    module: &Module,
    lookup: impl Fn(&str) -> Option<([u32; 2], bool)>,
) -> Result<(), String> {
    let mut check = |op: OperationRef<'_>| {
        if op.name() == "xr.nine_slice" {
            let url = op
                .operand(0)
                .and_then(|value| value.defining_operation())
                .ok_or("missing nine-slice source")?
                .attr("value")?;
            let (size, animated) =
                lookup(url).ok_or_else(|| format!("unresolved image {url:?}"))?;
            if animated {
                return Err("nine-slice requires a static PNG or SVG source".into());
            }
            if !nine_slice_edges(op)?.fits(size) {
                let span = op.span();
                let source = op.source_file().unwrap_or("<unknown>");
                let [width, height] = size;
                return Err(format!(
                    "{source}:{}:{}: nine-slice edges must leave a nonempty center in image {url:?} ({width} × {height})",
                    span.line, span.column
                ));
            }
        }
        Ok(())
    };
    for op in module.operations() {
        assets::walk(op, &mut check)?;
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
    prepare_module(module, filename, None, &BTreeMap::new())
}

/// Validate translations before optimization can remove unused calls.
pub fn prepare_module(
    module: Module,
    filename: &Path,
    declared_assets: Option<&[PathBuf]>,
    buffers: &BTreeMap<PathBuf, String>,
) -> Result<(Module, ResolvedTranslations), PrepareMlirError> {
    module.verify().map_err(PrepareMlirError::Mlir)?;
    let translations = resolve_translations(&module, filename, declared_assets, buffers)
        .map_err(PrepareMlirError::Resources)?;
    let module = module.optimize().map_err(PrepareMlirError::Mlir)?;
    Ok((module, translations))
}
