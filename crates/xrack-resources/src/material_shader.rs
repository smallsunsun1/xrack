use std::{path::Path, sync::Arc};
use xrack_runtime::material_shader::{
    INTERFACE, MaterialHooks, VALIDATION_ENTRY, VALIDATION_TEXTURES, VALIDATION_VERTEX_ENTRY,
    VERTEX_APPLICATION,
};
use xrack_runtime::scene_effect::CAPTURE_POSITION;

pub(super) fn load(path: &Path) -> Result<Arc<str>, String> {
    let source = crate::scene_effect::load_source(path, "material")?;
    let library = format!("{source}\n{INTERFACE}\n{CAPTURE_POSITION}\n{VALIDATION_TEXTURES}");
    let module = parse(&library)?;
    if !module.entry_points.is_empty()
        || module
            .global_variables
            .iter()
            .any(|(_, global)| global.binding.is_some())
    {
        return Err("material shader must use the supplied material functions and inputs".into());
    }
    let find = |name| {
        module
            .functions
            .iter()
            .find(|(_, function)| function.name.as_deref() == Some(name))
            .map(|(handle, _)| handle)
    };
    let vertex = find("scene_vertex");
    let hooks = MaterialHooks {
        vertex: vertex.is_some(),
        surface: find("scene_material").is_some(),
        light: find("scene_light").is_some(),
        ambient: find("scene_ambient").is_some(),
        capture_position: ["scene_material", "scene_light", "scene_ambient"]
            .into_iter()
            .filter_map(find)
            .any(|entry| {
                crate::scene_effect::reaches_function(
                    &module,
                    &module.functions[entry].body,
                    &["material_texture_world_position"],
                )
            }),
    };
    if !hooks.fragment() && !hooks.vertex {
        return Err("material shader must define scene_material, scene_vertex, scene_light or scene_ambient".into());
    }
    if module
        .functions
        .iter()
        .any(|(_, function)| discards(&function.body))
    {
        return Err("material shader cannot discard fragments because coverage is shared with depth, shadows and picking".into());
    }
    let prepared = hooks.prepare(&source);
    let wgsl = format!(
        "{prepared}\n{INTERFACE}\n{CAPTURE_POSITION}\n{VALIDATION_TEXTURES}\n{VERTEX_APPLICATION}\n{VALIDATION_ENTRY}\n{VALIDATION_VERTEX_ENTRY}"
    );
    let validated = parse(&wgsl)
        .map_err(|error| format!("invalid material hook interface (scene_material, scene_vertex, scene_light, scene_ambient)\n{error}"))?;
    crate::scene_effect::validate(&validated, &wgsl)?;
    if let Some(vertex) = vertex
        && crate::scene_effect::reaches_function(
            &module,
            &module.functions[vertex].body,
            &[
                "material_texture",
                "material_has_texture",
                "material_texture_size",
                "material_texture_world_position",
            ],
        )
    {
        return Err("scene_vertex cannot sample fragment material textures".into());
    }
    Ok(prepared.into())
}

fn parse(wgsl: &str) -> Result<naga::Module, String> {
    naga::front::wgsl::parse_str(wgsl).map_err(|error| error.emit_to_string(wgsl))
}

fn discards(block: &naga::Block) -> bool {
    block.iter().any(|statement| match statement {
        naga::Statement::Kill => true,
        naga::Statement::Block(block) => discards(block),
        naga::Statement::If { accept, reject, .. } => discards(accept) || discards(reject),
        naga::Statement::Switch { cases, .. } => cases.iter().any(|case| discards(&case.body)),
        naga::Statement::Loop {
            body, continuing, ..
        } => discards(body) || discards(continuing),
        _ => false,
    })
}
