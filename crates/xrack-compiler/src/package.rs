use crate::{CompilerError, emit_with_inputs, export_mlir_to, modules, prepare};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use xrack_syntax::module_manifest::{MANIFEST_FILE, MANIFEST_VERSION, ModuleManifest};

/// Publish a relocatable directory with a manifest, sources, optimized MLIR and generated Rust.
pub fn pack_module(
    input: &Path,
    output: &Path,
    sources: Option<&[PathBuf]>,
    assets: Option<&[PathBuf]>,
) -> Result<ModuleManifest, CompilerError> {
    let input = io(input, fs::canonicalize(input))?;
    let filename = text_path(&input)?;
    let source = io(&input, fs::read_to_string(&input))?;
    let (doc, mut used) = modules::parse_document_with_inputs(&source, filename, sources)?;
    let (module, translations) = prepare(&doc, filename, assets, &BTreeMap::new())?;
    let components = module
        .operations()
        .filter_map(|op| {
            (op.name() == "xr.component"
                && op.attribute("exported") == Some("true")
                && op.attribute("global") != Some("true"))
            .then(|| op.attribute("sym_name").map(str::to_owned))
            .flatten()
        })
        .collect();
    let rust = emit_with_inputs(&module, filename, assets, translations, &mut used)?;
    let used = used
        .iter()
        .map(|path| io(path, path.canonicalize()))
        .collect::<Result<Vec<_>, _>>()?;
    let root = input
        .ancestors()
        .skip(1)
        .find(|root| used.iter().all(|path| path.starts_with(root)))
        .ok_or_else(|| {
            CompilerError::Resources("module inputs do not share a filesystem root".into())
        })?;
    let relative = |path: &Path| {
        path.strip_prefix(root)
            .map(|name| Path::new("sources").join(name))
            .map_err(|error| CompilerError::Resources(error.to_string()))
    };
    let entry = relative(&input)?;
    let mlir = entry.with_extension("xr.mlir");
    let manifest = ModuleManifest {
        version: MANIFEST_VERSION,
        entry,
        mlir,
        rust: PathBuf::from("module.rs"),
        components,
        inputs: used
            .iter()
            .map(|path| relative(path))
            .collect::<Result<_, _>>()?,
    };
    manifest.validate().map_err(CompilerError::Resources)?;

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        io(parent, fs::create_dir_all(parent))?;
    }
    // create_dir fails if another invocation already owns the destination.
    io(output, fs::create_dir(output))?;
    let result = (|| {
        for (source, name) in used.iter().zip(&manifest.inputs) {
            let destination = output.join(name);
            if let Some(parent) = destination.parent() {
                io(parent, fs::create_dir_all(parent))?;
            }
            io(&destination, fs::copy(source, &destination))?;
        }
        let packaged_entry = output.join(&manifest.entry);
        let packaged_mlir = output.join(&manifest.mlir);
        let packaged_doc =
            modules::parse_document_with_inputs(&source, text_path(&packaged_entry)?, None)?.0;
        let ir = export_mlir_to(
            &packaged_doc,
            text_path(&packaged_entry)?,
            None,
            &packaged_mlir,
        )?;
        io(&packaged_mlir, fs::write(&packaged_mlir, ir))?;
        let rust_path = output.join(&manifest.rust);
        io(&rust_path, fs::write(&rust_path, format!("{rust}\n")))?;
        let manifest_path = output.join(MANIFEST_FILE);
        let json = serde_json::to_vec_pretty(&manifest)?;
        io(&manifest_path, fs::write(&manifest_path, json))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(output);
    }
    result?;
    Ok(manifest)
}

fn io<T>(path: &Path, result: std::io::Result<T>) -> Result<T, CompilerError> {
    result.map_err(|source| CompilerError::Io {
        path: path.to_owned(),
        source,
    })
}

fn text_path(path: &Path) -> Result<&str, CompilerError> {
    path.to_str().ok_or_else(|| {
        CompilerError::Resources(format!("module path {} is not UTF-8", path.display(),))
    })
}
