use std::{collections::HashSet, fs::File, io::Read, path::Path, sync::Arc};
use xrack_runtime::scene_effect::{
    CAPTURE_POSITION_HEADER, DepthInput, MAX_SOURCE_BYTES, SURFACE_HEADER, shader,
};

pub(super) fn load(path: &Path) -> Result<Arc<str>, String> {
    let source = load_source(path, "scene effect")?;
    let wgsl = shader(&source, DepthInput::None, 0, 0, 0);
    let module =
        naga::front::wgsl::parse_str(&wgsl).map_err(|error| error.emit_to_string(&wgsl))?;
    if module.entry_points.len() != 2
        || module
            .global_variables
            .iter()
            .filter(|(_, global)| global.binding.is_some())
            .count()
            != 4
    {
        return Err("scene effect must use the supplied entry points and resource bindings".into());
    }
    validate(&module, &wgsl)?;
    let effect = module.functions.iter().find_map(|(_, function)| {
        (function.name.as_deref() == Some("scene_effect")).then_some(&function.body)
    });
    let surface = effect
        .filter(|body| reaches_function(&module, body, &["scene_surface"]))
        .map_or("", |_| SURFACE_HEADER);
    let position = effect
        .filter(|body| reaches_function(&module, body, &["scene_buffer_world_position"]))
        .map_or("", |_| CAPTURE_POSITION_HEADER);
    Ok(if surface.is_empty() && position.is_empty() {
        source.into()
    } else {
        format!("{surface}{position}{source}").into()
    })
}

pub(super) fn load_source(path: &Path, kind: &str) -> Result<String, String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("wgsl") {
        return Err(format!("{kind} requires a .wgsl file"));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(format!("{kind} shader exceeds 64 KiB"));
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

pub(super) fn validate(module: &naga::Module, wgsl: &str) -> Result<(), String> {
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(module)
    .map(|_| ())
    .map_err(|error| error.emit_to_string(wgsl))
}

pub(super) fn reaches_function(module: &naga::Module, body: &naga::Block, names: &[&str]) -> bool {
    visit(module, body, names, &mut HashSet::new())
}

fn visit(
    module: &naga::Module,
    body: &naga::Block,
    names: &[&str],
    visited: &mut HashSet<naga::Handle<naga::Function>>,
) -> bool {
    body.iter().any(|statement| match statement {
        naga::Statement::Call { function, .. } => {
            if !visited.insert(*function) {
                return false;
            }
            let function = &module.functions[*function];
            function
                .name
                .as_deref()
                .is_some_and(|name| names.contains(&name))
                || visit(module, &function.body, names, visited)
        }
        naga::Statement::Block(block) => visit(module, block, names, visited),
        naga::Statement::If { accept, reject, .. } => {
            visit(module, accept, names, visited) || visit(module, reject, names, visited)
        }
        naga::Statement::Switch { cases, .. } => cases
            .iter()
            .any(|case| visit(module, &case.body, names, visited)),
        naga::Statement::Loop {
            body, continuing, ..
        } => visit(module, body, names, visited) || visit(module, continuing, names, visited),
        _ => false,
    })
}
