use crate::Program;
use std::{collections::BTreeMap, path::Path};
use xrack_runtime::{ExternalSource, LoadedExternal};

/// Enable file and HTTP(S) sources for Loaders in the current process.
pub fn install_external_loader() {
    xrack_runtime::register_external_loader(load);
}

fn load(source: ExternalSource) -> Result<LoadedExternal, String> {
    let filename = source
        .path
        .as_deref()
        .unwrap_or_else(|| Path::new(&source.location));
    let resource = source
        .location
        .split('?')
        .next()
        .unwrap_or(&source.location);
    let module = if resource.ends_with(".mlir") {
        xrack_mlir::Module::parse(&source.text)?
    } else if resource.ends_with(".xr") {
        let document = xrack_parser::parse(&source.text).map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        let document = if let Some(path) = source.path.as_deref() {
            xrack_ir::modules::resolve_document(
                document,
                path,
                None,
                &BTreeMap::new(),
                &mut BTreeMap::new(),
                None,
            )
            .map_err(|error| match error {
                xrack_ir::modules::ModuleError::Diagnostics { diagnostics, .. } => diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
                xrack_ir::modules::ModuleError::Io { path, source } => {
                    format!("{}: {source}", path.display())
                }
            })?
        } else {
            if !document.imports.is_empty() {
                return Err("HTTP(S) Loader sources must be standalone".into());
            }
            document
        };
        xrack_mlir::lower_module(&document, &filename.to_string_lossy()).map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?
    } else {
        return Err("external Loader source must end in .xr or .mlir".into());
    };

    let selected = match source.component {
        Some(name) => name,
        None => {
            let mut entries = module.operations().filter_map(|op| {
                (op.name() == "xr.component"
                    && op.attribute("exported") == Some("true")
                    && op.attribute("global") != Some("true"))
                .then(|| op.attribute("sym_name"))
                .flatten()
            });
            let name = entries
                .next()
                .ok_or("external Loader source has no exported component")?;
            if entries.next().is_some() {
                return Err(
                    "external Loader source has multiple exports; specify #ComponentName".into(),
                );
            }
            name.to_owned()
        }
    };
    let root = module
        .operations()
        .find(|op| {
            op.name() == "xr.component"
                && op.attribute("sym_name") == Some(selected.as_str())
                && op.attribute("exported") == Some("true")
                && op.attribute("global") != Some("true")
        })
        .and_then(|component| component.children().find(|op| op.name() == "xr.element"))
        .ok_or_else(|| format!("external Loader source does not export {selected}"))?;
    if matches!(
        root.attribute("kind"),
        Some("Window" | "ApplicationWindow" | "SecondaryWindow")
    ) {
        return Err("external Loader requires an embeddable component".into());
    }
    let program = if source.path.is_some() {
        let (module, translations) =
            xrack_resources::prepare_module(module, filename, None, &BTreeMap::new())
                .map_err(|error| error.to_string())?;
        Program::from_module_with_file_resources(&module, filename, translations.catalog)
            .map(|(program, _)| program)
    } else {
        module.verify()?;
        Program::from_module(&module.optimize()?)
    }
    .map_err(|error| error.to_string())?;
    program
        .create(&selected)
        .map(|instance| {
            let weak = instance.as_weak();
            LoadedExternal::new(instance, weak)
        })
        .map_err(|error| error.to_string())
}
