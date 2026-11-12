use crate::{EmbeddedImage, EmbeddedPath, animated_image::encode_gif_frames};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use xrack_assets::{
    ImageFormat, MAX_ASSET_BYTES, image_size, parse_path, vector_bounds, vector_segments,
};

use xrack_mlir::{Module, OperationRef};

#[derive(Default)]
pub struct ResolvedAssets {
    pub images: BTreeMap<String, EmbeddedImage>,
    pub fonts: Vec<Vec<u8>>,
    pub paths: BTreeMap<String, EmbeddedPath>,
    pub inputs: BTreeSet<PathBuf>,
}

fn walk<'m>(
    op: OperationRef<'m>,
    visit: &mut impl FnMut(OperationRef<'m>) -> Result<(), String>,
) -> Result<(), String> {
    visit(op)?;
    for child in op.children() {
        walk(child, visit)?;
    }
    Ok(())
}

pub fn resolve_assets(
    module: &Module,
    filename: &Path,
    declared_assets: Option<&[PathBuf]>,
) -> Result<ResolvedAssets, String> {
    let declared = declared_paths(declared_assets)?;
    let base = filename.parent().unwrap_or_else(|| Path::new("."));
    let filename = filename.to_string_lossy();
    let mut assets = ResolvedAssets::default();
    let (fonts, font_inputs) = resolve_fonts_at(module, &filename, base, declared.as_ref())?;
    assets.fonts = fonts;
    assets.inputs.extend(font_inputs);
    let mut visit = |op: OperationRef<'_>| {
        if op.name() == "xr.image_url" {
            let url = op.attr("value")?;
            if url.is_empty() || assets.images.contains_key(url) {
                return Ok(());
            }
            let (path, image) = load_image(base, url, declared.as_ref()).map_err(|error| {
                let span = op.span();
                format!(
                    "{filename}:{}:{}: image {url:?}: {error}",
                    span.line, span.column
                )
            })?;
            assets.inputs.insert(path);
            assets.images.insert(url.to_owned(), image);
        } else {
            collect_path(
                op,
                &filename,
                module.has_visual_cache_plan(),
                &mut assets.paths,
            )?;
        }
        Ok(())
    };
    for op in module.operations() {
        walk(op, &mut visit)?;
    }
    Ok(assets)
}

/// Validate literal SVG commands and collect the plans selected by MLIR.
pub fn collect_vector_paths(module: &Module) -> Result<BTreeMap<String, EmbeddedPath>, String> {
    let mut paths = BTreeMap::new();
    let mut visit = |op| collect_path(op, "<MLIR>", module.has_visual_cache_plan(), &mut paths);
    for op in module.operations() {
        walk(op, &mut visit)?;
    }
    Ok(paths)
}

fn collect_path(
    op: OperationRef<'_>,
    filename: &str,
    use_visual_plan: bool,
    paths: &mut BTreeMap<String, EmbeddedPath>,
) -> Result<(), String> {
    if op.name() != "xr.element" || op.attr("kind")? != "Path" {
        return Ok(());
    }
    for binding in op.children() {
        if binding.name() != "xr.bind" || binding.attr("name")? != "commands" {
            continue;
        }
        let Some(literal) = binding
            .operand(0)
            .and_then(|value| value.defining_operation())
            .filter(|literal| literal.name() == "xr.literal")
        else {
            continue;
        };
        let commands: String =
            serde_json::from_str(literal.attr("value")?).map_err(|error| error.to_string())?;
        if paths.contains_key(&commands) {
            continue;
        }
        let path = parse_path(&commands).map_err(|error| {
            let span = binding.span();
            let source = binding.source_file().unwrap_or(filename);
            format!(
                "{source}:{}:{}: invalid Path commands: {error}",
                span.line, span.column
            )
        })?;
        if use_visual_plan && binding.attribute("xr.precompiled_path") == Some("true") {
            let plan = match path {
                Some(path) => EmbeddedPath {
                    bounds: vector_bounds(&path),
                    segments: vector_segments(&path).collect(),
                },
                None => EmbeddedPath {
                    bounds: [0.0; 4],
                    segments: Vec::new(),
                },
            };
            paths.insert(commands, plan);
        }
    }
    Ok(())
}

fn resolve_fonts_at(
    module: &Module,
    filename: &str,
    base: &Path,
    declared: Option<&BTreeSet<PathBuf>>,
) -> Result<(Vec<Vec<u8>>, BTreeSet<PathBuf>), String> {
    let mut fonts = Vec::new();
    let mut seen_fonts = BTreeSet::new();
    for op in module.operations().filter(|op| op.name() == "xr.font") {
        let url = op.attr("path")?;
        let span = op.span();
        let source = op.source_file().unwrap_or(filename);
        let error_at_import = |error| {
            format!(
                "{source}:{}:{}: font {url:?}: {error}",
                span.line, span.column
            )
        };
        let relative = local_path(url).map_err(error_at_import)?;
        let extension = relative
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default();
        if !["ttf", "otf", "ttc"]
            .iter()
            .any(|format| extension.eq_ignore_ascii_case(format))
        {
            return Err(error_at_import("expected a .ttf, .otf or .ttc font".into()));
        }
        let path = asset_path(base, relative, declared, "font").map_err(error_at_import)?;
        if seen_fonts.insert(path.clone()) {
            let bytes = read_asset(&path, "font").map_err(error_at_import)?;
            validate_font(&bytes).map_err(error_at_import)?;
            fonts.push(bytes);
        }
    }
    Ok((fonts, seen_fonts))
}

fn load_image(
    base: &Path,
    url: &str,
    declared: Option<&BTreeSet<PathBuf>>,
) -> Result<(PathBuf, EmbeddedImage), String> {
    let relative = local_path(url)?;
    let extension = relative
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    if !["png", "svg", "gif"]
        .iter()
        .any(|format| extension.eq_ignore_ascii_case(format))
    {
        return Err("expected a .png, .svg or .gif image".into());
    }
    let path = asset_path(base, relative, declared, "image")?;
    let bytes = read_asset(&path, "image")?;
    let image = if extension.eq_ignore_ascii_case("gif") {
        let animation = encode_gif_frames(&bytes)?;
        EmbeddedImage::Animated {
            size: animation.size,
            frames: animation.frames,
            plays: animation.plays,
        }
    } else {
        let format = if extension.eq_ignore_ascii_case("png") {
            ImageFormat::Png
        } else {
            ImageFormat::Svg
        };
        let size = image_size(format, &bytes)?;
        EmbeddedImage::Static {
            format,
            size,
            bytes,
        }
    };
    Ok((path, image))
}

pub(crate) fn declared_paths(
    paths: Option<&[PathBuf]>,
) -> Result<Option<BTreeSet<PathBuf>>, String> {
    paths
        .map(|paths| {
            paths
                .iter()
                .map(|path| {
                    path.canonicalize()
                        .map_err(|error| format!("{}: {error}", path.display()))
                })
                .collect()
        })
        .transpose()
}

pub(crate) fn local_path(url: &str) -> Result<&Path, String> {
    let relative = Path::new(url);
    if url.is_empty() || relative.is_absolute() || url.contains(':') {
        return Err("expected a local path relative to the .xr file".into());
    }
    Ok(relative)
}

pub(crate) fn asset_path(
    base: &Path,
    relative: &Path,
    declared: Option<&BTreeSet<PathBuf>>,
    kind: &str,
) -> Result<PathBuf, String> {
    let path = base.join(relative).canonicalize().map_err(|error| {
        if declared.is_some() {
            format!("{error}; list the {kind} in xr_ui_library(assets = [...])")
        } else {
            error.to_string()
        }
    })?;
    if declared.is_some_and(|inputs| !inputs.contains(&path)) {
        return Err("not declared in xr_ui_library(assets = [...])".into());
    }
    Ok(path)
}

pub(crate) fn read_asset(path: &Path, kind: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(MAX_ASSET_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_ASSET_BYTES {
        return Err(format!("{kind} file exceeds 16 MiB"));
    }
    Ok(bytes)
}

fn validate_font(bytes: &[u8]) -> Result<(), String> {
    let count = ttf_parser::fonts_in_collection(bytes).unwrap_or(1);
    if !(1..=256).contains(&count) {
        return Err("a font file must contain between 1 and 256 faces".into());
    }
    for index in 0..count {
        let face = ttf_parser::Face::parse(bytes, index)
            .map_err(|error| format!("invalid font face {index}: {error}"))?;
        let named = |names: &[u16]| {
            face.names().into_iter().any(|name| {
                names.contains(&name.name_id)
                    && (name.to_string().is_some_and(|name| !name.trim().is_empty())
                        || name.platform_id == ttf_parser::PlatformId::Macintosh
                            && name.encoding_id == 0
                            && !name.name.is_empty())
            })
        };
        use ttf_parser::name_id;
        if !named(&[name_id::FAMILY, name_id::TYPOGRAPHIC_FAMILY])
            || !named(&[name_id::POST_SCRIPT_NAME])
            || face.tables().cmap.is_none()
        {
            return Err(format!(
                "font face {index} requires family and PostScript names and a character map"
            ));
        }
    }
    Ok(())
}
