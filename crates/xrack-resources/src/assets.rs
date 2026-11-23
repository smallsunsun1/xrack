use crate::{
    EmbeddedImage, EmbeddedPath,
    animated_image::{MAX_DECODED_BYTES, MAX_FRAMES, encode_gif_frames},
    mesh::MeshSource,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
use xrack_assets::{
    ImageFormat, MAX_ASSET_BYTES, color_lut_size, image_size, parse_path, validate_image_size,
    vector_bounds, vector_segments,
};

use xrack_mlir::{Module, OperationRef};
use xrack_runtime::{
    Image, ImageAsset,
    scene3d::{
        EnvironmentProbe, MeshAsset, MeshImportOptions, environment_resource_key, mesh_resource_key,
    },
};

#[derive(Default)]
pub struct ResolvedAssets {
    pub images: BTreeMap<String, EmbeddedImage>,
    pub fonts: Vec<Vec<u8>>,
    pub paths: BTreeMap<String, EmbeddedPath>,
    pub scene: SceneAssets,
    pub inputs: BTreeSet<PathBuf>,
}

#[derive(Default)]
pub struct SceneAssets {
    pub meshes: BTreeMap<String, MeshAsset>,
    pub environments: BTreeMap<String, Arc<EnvironmentProbe>>,
}

pub(super) fn walk<'m>(
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
    let mut mesh_sources = BTreeMap::<(PathBuf, MeshSource), MeshAsset>::new();
    let mut chart_sources = BTreeMap::<(PathBuf, MeshSource), MeshAsset>::new();
    let mut lod_sources = BTreeMap::<((PathBuf, MeshSource), bool), MeshAsset>::new();
    let mut lut_images = Vec::new();
    let mut visit = |op: OperationRef<'_>| {
        if op.name() == "xr.image_url" {
            let url = op.attr("value")?;
            if url.is_empty() || assets.images.contains_key(url) {
                return Ok(());
            }
            let (path, image) = load_image(base, url, declared.as_ref()).map_err(|error| {
                let span = op.span();
                let source = op.source_file().unwrap_or(&filename);
                format!(
                    "{source}:{}:{}: image {url:?}: {error}",
                    span.line, span.column
                )
            })?;
            assets.inputs.insert(path);
            assets.images.insert(url.to_owned(), image);
        } else if let Some(options) = MeshImportOptions::from_operation(op.name()) {
            let url = op.attr("value")?;
            let key = mesh_resource_key(url, options);
            if let std::collections::btree_map::Entry::Vacant(entry) =
                assets.scene.meshes.entry(key)
            {
                let error_at_mesh = |error: String| {
                    let span = op.span();
                    let source = op.source_file().unwrap_or(&filename);
                    format!(
                        "{source}:{}:{}: mesh {url:?}: {error}",
                        span.line, span.column
                    )
                };
                let resolved =
                    MeshSource::resolve(base, url, declared.as_ref()).map_err(error_at_mesh)?;
                let kind = resolved.1;
                let mesh = match mesh_sources.entry(resolved.clone()) {
                    std::collections::btree_map::Entry::Occupied(mesh) => mesh.get().clone(),
                    std::collections::btree_map::Entry::Vacant(source) => {
                        let (mesh, inputs) = kind
                            .load(&source.key().0, declared.as_ref())
                            .map_err(error_at_mesh)?;
                        assets.inputs.extend(inputs);
                        source.insert(mesh).clone()
                    }
                };
                let mesh = if options.lightmap_uv {
                    match chart_sources.entry(resolved.clone()) {
                        std::collections::btree_map::Entry::Occupied(chart) => chart.get().clone(),
                        std::collections::btree_map::Entry::Vacant(chart) => {
                            let generated =
                                kind.generate_lightmap_uv(mesh).map_err(error_at_mesh)?;
                            chart.insert(generated).clone()
                        }
                    }
                } else {
                    mesh
                };
                let mesh = if options.automatic_lod {
                    lod_sources
                        .entry((resolved, options.lightmap_uv))
                        .or_insert_with(|| crate::mesh_lod::generate(mesh))
                        .clone()
                } else {
                    mesh
                };
                entry.insert(mesh);
            }
        } else if op.name() == "xr.environment_url" {
            let url = op.attr("value")?;
            let key = environment_resource_key(url);
            if let std::collections::btree_map::Entry::Vacant(entry) =
                assets.scene.environments.entry(key)
            {
                let resolve = || {
                    let path = asset_path(
                        base,
                        local_path(url)?,
                        declared.as_ref(),
                        "environment probe",
                    )?;
                    let probe = crate::environment::load(&path)?;
                    Ok::<_, String>((path, probe))
                };
                let (path, probe) = resolve().map_err(|error| {
                    let span = op.span();
                    let source = op.source_file().unwrap_or(&filename);
                    format!(
                        "{source}:{}:{}: environment {url:?}: {error}",
                        span.line, span.column
                    )
                })?;
                assets.inputs.insert(path);
                entry.insert(probe);
            }
        } else {
            if op.name() == "xr.element"
                && op.attribute("kind") == Some("SceneView3D")
                && let Some(check) = static_lut_check(op, &filename)?
            {
                lut_images.push(check);
            }
            collect_path(
                op,
                &filename,
                module.has_visual_cache_plan(),
                &|_| false,
                &mut assets.paths,
            )?;
        }
        Ok(())
    };
    for op in module.operations() {
        walk(op, &mut visit)?;
    }
    for (url, size, location) in lut_images {
        match assets.images.get(url.as_str()) {
            Some(EmbeddedImage::Static {
                format: ImageFormat::Png,
                size: dimensions,
                ..
            }) if color_lut_size(*dimensions, size).is_some() => {}
            Some(EmbeddedImage::Static {
                format: ImageFormat::Png,
                size: [width, height],
                ..
            }) => {
                return Err(format!(
                    "{location}: scene-lut {url:?} has dimensions {width}×{height}; expected a horizontal N²×N strip (2–64), matching scene-lut-size {size} (0 infers the size)"
                ));
            }
            _ => {
                return Err(format!(
                    "{location}: scene-lut {url:?} requires a static PNG"
                ));
            }
        }
    }
    Ok(assets)
}

fn static_lut_check<'m>(
    element: OperationRef<'m>,
    filename: &str,
) -> Result<Option<(String, i64, String)>, String> {
    let binding = |name| {
        element
            .children()
            .find(|op| op.name() == "xr.bind" && op.attribute("name") == Some(name))
    };
    let definition =
        |bind: OperationRef<'m>| bind.operand(0).and_then(|value| value.defining_operation());
    let Some(image) = binding("scene-lut").and_then(definition) else {
        return Ok(None);
    };
    let span = image.span();
    let source = image.source_file().unwrap_or(filename);
    let location = || format!("{source}:{}:{}", span.line, span.column);
    if image.name() == "xr.nine_slice" {
        return Err(format!(
            "{}: scene-lut requires an unsliced PNG",
            location()
        ));
    }
    if image.name() != "xr.image_url" {
        return Ok(None);
    }
    let url = image.attr("value")?;
    if url.is_empty() {
        return Ok(None);
    }
    let size = binding("scene-lut-size")
        .and_then(definition)
        .filter(|op| op.name() == "xr.literal")
        .and_then(|op| op.attribute("value"))
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    Ok(Some((url.into(), size, location())))
}

/// Check that the caller resolved all 3D assets referenced by the module.
pub fn require_scene_assets(module: &Module, scene: &SceneAssets) -> Result<(), String> {
    for op in module.operations() {
        walk(op, &mut |op| {
            if let Some(options) = MeshImportOptions::from_operation(op.name()) {
                let url = op.attr("value")?;
                if !scene.meshes.contains_key(&mesh_resource_key(url, options)) {
                    return Err(format!("unresolved mesh {url:?}"));
                }
            } else if op.name() == "xr.environment_url" {
                let url = op.attr("value")?;
                if !scene
                    .environments
                    .contains_key(&environment_resource_key(url))
                {
                    return Err(format!("unresolved environment {url:?}"));
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// Validate literal SVG commands and collect the plans selected by MLIR.
pub fn collect_vector_paths(module: &Module) -> Result<BTreeMap<String, EmbeddedPath>, String> {
    collect_missing_vector_paths(module, |_| false)
}

/// Validate literal SVG commands and prepare missing plans. The supplied plans
/// are expected to have been validated when they were built.
pub fn collect_missing_vector_paths(
    module: &Module,
    has_plan: impl Fn(&str) -> bool,
) -> Result<BTreeMap<String, EmbeddedPath>, String> {
    let mut paths = BTreeMap::new();
    let mut visit = |op| {
        collect_path(
            op,
            "<MLIR>",
            module.has_visual_cache_plan(),
            &has_plan,
            &mut paths,
        )
    };
    for op in module.operations() {
        walk(op, &mut visit)?;
    }
    Ok(paths)
}

fn collect_path(
    op: OperationRef<'_>,
    filename: &str,
    use_visual_plan: bool,
    has_plan: &impl Fn(&str) -> bool,
    paths: &mut BTreeMap<String, EmbeddedPath>,
) -> Result<(), String> {
    if op.name() != "xr.element" {
        return Ok(());
    }
    let kind = op.attr("kind")?;
    let property = match kind {
        "Path" => "commands",
        "PathView" => "path",
        _ => return Ok(()),
    };
    for binding in op.children() {
        if binding.name() != "xr.bind" || binding.attr("name")? != property {
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
        let planned = use_visual_plan && binding.attribute("xr.precompiled_path") == Some("true");
        if paths.contains_key(&commands) || planned && has_plan(&commands) {
            continue;
        }
        let path = parse_path(&commands).map_err(|error| {
            let span = binding.span();
            let source = binding.source_file().unwrap_or(filename);
            format!(
                "{source}:{}:{}: invalid {kind} {property}: {error}",
                span.line, span.column
            )
        })?;
        if planned {
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

/// Validate the source bytes and dimensions supplied for code generation.
pub fn check_image_sources(images: &BTreeMap<String, EmbeddedImage>) -> Result<(), String> {
    for (url, image) in images {
        match image {
            EmbeddedImage::Static {
                format,
                size,
                bytes,
            } => validate_image(*format, *size, bytes),
            EmbeddedImage::Animated { size, frames, .. } => validate_animation(
                *size,
                frames.iter().map(|(end, png)| (*end, png.as_slice())),
            ),
        }
        .map_err(|error| format!("image {url:?}: {error}"))?;
    }
    Ok(())
}

/// Validate the same constraints for images supplied to the interpreter.
pub fn check_loaded_image_sources(images: &BTreeMap<String, ImageAsset>) -> Result<(), String> {
    for (url, image) in images {
        match image {
            ImageAsset::Embedded(Image::Empty) => Ok(()),
            ImageAsset::Embedded(Image::Animated(animation)) => {
                validate_animation(image.size(), animation.frames())
            }
            ImageAsset::Animated(animation) => validate_animation(image.size(), animation.frames()),
            _ => {
                let pixels = image
                    .pixels()
                    .ok_or_else(|| format!("image {url:?}: no image data"))?;
                validate_image(pixels.format, pixels.size, pixels.bytes).and_then(|()| {
                    if pixels.edges.is_some_and(|edges| !edges.fits(pixels.size)) {
                        return Err("nine-slice edges must leave a nonempty center".into());
                    }
                    Ok(())
                })
            }
        }
        .map_err(|error| format!("image {url:?}: {error}"))?;
    }
    Ok(())
}

fn validate_image(format: ImageFormat, size: [u32; 2], bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_ASSET_BYTES {
        return Err("image file exceeds 16 MiB".into());
    }
    validate_image_size(size)?;
    let actual = image_size(format, bytes)?;
    if actual != size {
        return Err(format!(
            "declared dimensions {} × {} do not match decoded dimensions {} × {}",
            size[0], size[1], actual[0], actual[1]
        ));
    }
    Ok(())
}

fn validate_animation<'a>(
    size: [u32; 2],
    frames: impl ExactSizeIterator<Item = (u32, &'a [u8])>,
) -> Result<(), String> {
    let [width, height] = validate_image_size(size)?;
    if frames.len() == 0 {
        return Err("animation contains no frames".into());
    }
    if frames.len() > MAX_FRAMES
        || frames.len() as u64 * u64::from(width) * u64::from(height) * 4 > MAX_DECODED_BYTES
    {
        return Err(format!(
            "animation exceeds {MAX_FRAMES} frames or 64 MiB of composited pixels"
        ));
    }
    let mut previous_end = 0;
    let mut encoded_bytes = 0;
    for (index, (end, png)) in frames.enumerate() {
        let frame = index + 1;
        if end <= previous_end {
            return Err(format!("frame {frame} must end after {previous_end} ms"));
        }
        previous_end = end;
        encoded_bytes += png.len() as u64;
        if encoded_bytes > MAX_ASSET_BYTES {
            return Err("animation exceeds 16 MiB of PNG frames".into());
        }
        validate_image(ImageFormat::Png, size, png)
            .map_err(|error| format!("frame {frame}: {error}"))?;
    }
    Ok(())
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

/// Validate each supplied font, including any extra fonts added by the host.
pub fn check_font_sources<'a>(sources: impl IntoIterator<Item = &'a [u8]>) -> Result<(), String> {
    for (index, bytes) in sources.into_iter().enumerate() {
        validate_font(bytes).map_err(|error| format!("supplied font {}: {error}", index + 1))?;
    }
    Ok(())
}

fn validate_font(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_ASSET_BYTES {
        return Err("font file exceeds 16 MiB".into());
    }
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
