use crate::{
    codegen::EmbeddedImage,
    mlir::{Module, OperationRef},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use xrack_assets::{ImageFormat, MAX_ASSET_BYTES, image_size, parse_path};

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

pub fn resolve(
    module: &Module,
    filename: &str,
    declared_assets: Option<&[PathBuf]>,
) -> Result<BTreeMap<String, EmbeddedImage>, String> {
    let declared = declared_assets
        .map(|paths| {
            paths
                .iter()
                .map(|path| {
                    path.canonicalize()
                        .map_err(|error| format!("{}: {error}", path.display()))
                })
                .collect::<Result<BTreeSet<_>, _>>()
        })
        .transpose()?;
    let base = Path::new(filename)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mut images = BTreeMap::new();
    let mut visit = |op: OperationRef<'_>| {
        if op.name() == "xr.image_url" {
            let url = op.attr("value")?;
            if url.is_empty() || images.contains_key(url) {
                return Ok(());
            }
            let image = load_image(base, url, declared.as_ref()).map_err(|error| {
                let span = op.span();
                format!(
                    "{filename}:{}:{}: image {url:?}: {error}",
                    span.line, span.column
                )
            })?;
            images.insert(url.to_owned(), image);
        } else if op.name() == "xr.element" && op.attr("kind")? == "Path" {
            for binding in op.children() {
                if binding.name() == "xr.bind"
                    && binding.attr("name")? == "commands"
                    && let Some(literal) = binding
                        .operand(0)
                        .and_then(|value| value.defining_operation())
                    && literal.name() == "xr.literal"
                {
                    let commands: String = serde_json::from_str(literal.attr("value")?)
                        .map_err(|error| error.to_string())?;
                    parse_path(&commands).map_err(|error| {
                        let span = binding.span();
                        format!(
                            "{filename}:{}:{}: invalid Path commands: {error}",
                            span.line, span.column,
                        )
                    })?;
                }
            }
        }
        Ok(())
    };
    for op in module.operations() {
        walk(op, &mut visit)?;
    }
    Ok(images)
}

fn load_image(
    base: &Path,
    url: &str,
    declared: Option<&BTreeSet<PathBuf>>,
) -> Result<EmbeddedImage, String> {
    let relative = Path::new(url);
    if relative.is_absolute() || url.contains(':') {
        return Err("expected a local path relative to the .xr file".into());
    }
    let extension = relative
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    let format = if extension.eq_ignore_ascii_case("png") {
        ImageFormat::Png
    } else if extension.eq_ignore_ascii_case("svg") {
        ImageFormat::Svg
    } else {
        return Err("expected a .png or .svg image".into());
    };
    let path = base.join(relative).canonicalize().map_err(|error| {
        if declared.is_some() {
            format!("{error}; list the image in xr_ui_library(assets = [...])")
        } else {
            error.to_string()
        }
    })?;
    if declared.is_some_and(|inputs| !inputs.contains(&path)) {
        return Err("not declared in xr_ui_library(assets = [...])".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(MAX_ASSET_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_ASSET_BYTES {
        return Err("image file exceeds 16 MiB".into());
    }
    let size = image_size(format, &bytes)?;
    Ok(EmbeddedImage {
        format,
        size,
        bytes,
    })
}
