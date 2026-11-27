use std::{collections::BTreeSet, ops::Range, path::Path, sync::Arc};
use xrack_runtime::material_shader::{
    CAPTURE_AUGMENT_ENTRY, CAPTURE_AUGMENT_PREFIX, EMPTY_AUGMENT_TEXTURES, INTERFACE,
    MaterialHooks, VALIDATION_AUGMENT_INPUTS, VALIDATION_ENTRY, VALIDATION_TEXTURES,
    VALIDATION_VERTEX_ENTRY, VERTEX_APPLICATION, prepare_capture_augment,
};
use xrack_runtime::scene_effect::CAPTURE_POSITION;

pub(super) fn load(path: &Path) -> Result<Arc<str>, String> {
    let source = crate::scene_effect::load_source(path, "material")?;
    let library = format!(
        "{source}\n{INTERFACE}\n{CAPTURE_POSITION}\n{VALIDATION_TEXTURES}\n{VALIDATION_AUGMENT_INPUTS}\n{EMPTY_AUGMENT_TEXTURES}"
    );
    let module = parse(&library)?;
    if !module.entry_points.is_empty()
        || module
            .global_variables
            .iter()
            .any(|(_, global)| global.binding.is_some())
    {
        return Err("material shader must use the supplied material functions and inputs".into());
    }
    if module
        .functions
        .iter()
        .any(|(_, function)| discards(&function.body))
    {
        return Err("material shader cannot discard fragments because coverage is shared with depth, shadows and picking".into());
    }
    let find = |name| {
        module
            .functions
            .iter()
            .find(|(_, function)| function.name.as_deref() == Some(name))
            .map(|(handle, _)| handle)
    };
    let augment = find("scene_capture_augment").is_some();
    let tokens = if augment || source.contains(CAPTURE_AUGMENT_PREFIX) {
        wgsl_tokens(&source)
    } else {
        Vec::new()
    };
    if tokens
        .iter()
        .any(|range| source[range.clone()].starts_with(CAPTURE_AUGMENT_PREFIX))
    {
        return Err("the xrack_augment_ prefix is reserved for linked capture shaders".into());
    }
    if augment {
        if [
            "scene_vertex",
            "scene_material",
            "scene_light",
            "scene_ambient",
            "scene_material_data",
            "scene_material_data_at",
        ]
        .into_iter()
        .any(|name| find(name).is_some())
        {
            return Err("a capture augment shader must define scene_capture_augment separately from model material hooks".into());
        }
        return load_augment(&source, &tokens);
    }
    let vertex = find("scene_vertex");
    let hooks = MaterialHooks {
        vertex: vertex.is_some(),
        surface: find("scene_material").is_some(),
        light: find("scene_light").is_some(),
        ambient: find("scene_ambient").is_some(),
        data: find("scene_material_data").is_some(),
        indexed_data: find("scene_material_data_at").is_some(),
        capture_position: [
            "scene_material",
            "scene_light",
            "scene_ambient",
            "scene_material_data",
            "scene_material_data_at",
        ]
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
        return Err("material shader must define scene_material, scene_vertex, scene_light, scene_ambient, scene_material_data or scene_material_data_at".into());
    }
    let prepared = hooks.prepare(&source);
    let wgsl = format!(
        "{prepared}\n{INTERFACE}\n{CAPTURE_POSITION}\n{VALIDATION_TEXTURES}\n{VERTEX_APPLICATION}\n{VALIDATION_ENTRY}\n{VALIDATION_VERTEX_ENTRY}"
    );
    let validated = parse(&wgsl)
        .map_err(|error| format!("invalid material hook interface (scene_material, scene_vertex, scene_light, scene_ambient, scene_material_data, scene_material_data_at)\n{error}"))?;
    crate::scene_effect::validate(&validated, &wgsl)?;
    if let Some(vertex) = vertex
        && crate::scene_effect::reaches_function(
            &module,
            &module.functions[vertex].body,
            &[
                "material_texture",
                "material_texture_lod",
                "material_texture_grad",
                "material_has_texture",
                "material_texture_size",
                "material_texture_world_position",
                "material_has_cubemap",
                "material_cubemap",
                "material_cubemap_lod",
            ],
        )
    {
        return Err("scene_vertex cannot sample fragment material textures".into());
    }
    Ok(prepared.into())
}

fn load_augment(source: &str, ranges: &[Range<usize>]) -> Result<Arc<str>, String> {
    let tokens: Vec<_> = ranges.iter().map(|range| &source[range.clone()]).collect();
    let mut names = BTreeSet::new();
    let mut depth = 0usize;
    let mut directives = 0;
    let mut at_directive = false;
    let mut leading = true;
    for (i, &token) in tokens.iter().enumerate() {
        if depth == 0 {
            if leading {
                if at_directive && token == ";" {
                    directives = ranges[i].end;
                    at_directive = false;
                } else if !at_directive {
                    at_directive = matches!(token, "enable" | "requires" | "diagnostic");
                    leading = at_directive;
                }
            }
            if matches!(
                token,
                "fn" | "struct" | "alias" | "const" | "override" | "var"
            ) {
                let mut name = i + 1;
                if tokens.get(name) == Some(&"<") {
                    name += tokens[name..]
                        .iter()
                        .position(|&token| token == ">")
                        .unwrap_or(0)
                        + 1;
                }
                if let Some(&name) = tokens.get(name) {
                    names.insert(name);
                }
            }
        }
        match token {
            "{" => depth += 1,
            "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    let mut body = String::with_capacity(source.len());
    let mut last = directives;
    let mut depth = 0usize;
    let mut in_struct = false;
    for (i, range) in ranges
        .iter()
        .enumerate()
        .filter(|(_, range)| range.start >= directives)
    {
        let token = tokens[i];
        if depth == 0 && token == "struct" {
            in_struct = true;
        }
        let member = i.checked_sub(1).and_then(|previous| tokens.get(previous)) == Some(&".")
            || in_struct && tokens.get(i + 1) == Some(&":");
        if !member && names.contains(token) {
            body.push_str(&source[last..range.start]);
            body.push_str(CAPTURE_AUGMENT_PREFIX);
            body.push_str(token);
            last = range.end;
        }
        match token {
            "{" => depth += 1,
            "}" => {
                depth = depth.saturating_sub(1);
                in_struct &= depth != 0;
            }
            _ => {}
        }
    }
    body.push_str(&source[last..]);

    let directives = &source[..directives];
    let wgsl = format!(
        "{directives}\n{INTERFACE}\n{CAPTURE_POSITION}\n{VALIDATION_AUGMENT_INPUTS}\n{EMPTY_AUGMENT_TEXTURES}\n{body}\n
        @fragment fn xr_capture_check(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {{
            let input = XrMaterialInput(at.xy, at.xy, vec2(0.0), vec2(0.0), vec2(0.0), vec2(0.0), at.xyz, vec3(0.0, 0.0, 1.0),
                vec4(1.0), vec3(0.0), true, vec4(0.0), 0u, vec4(1.0));
            let surface = XrMaterialSurface(vec3(1.0), vec3(0.0), vec3(0.0, 0.0, 1.0), 0.5, 0.0);
            return {CAPTURE_AUGMENT_ENTRY}(input, surface, 0u);
        }}"
    );
    let module =
        parse(&wgsl).map_err(|error| format!("invalid capture augment shader\n{error}"))?;
    crate::scene_effect::validate(&module, &wgsl)?;
    Ok(prepare_capture_augment(directives, &body).into())
}

// Naga has already parsed this source. Keep tokens and source ranges so user
// declarations can be prefixed while preserving field names, comments and layout.
fn wgsl_tokens(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    let blank = |ch: char| {
        matches!(
            ch,
            ' ' | '\u{0009}'
                ..='\u{000d}' | '\u{0085}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}'
        )
    };
    let word = |ch: char| ch.is_ascii_alphanumeric() || ch == '_' || !ch.is_ascii() && !blank(ch);
    while index < bytes.len() {
        match bytes.get(index..index + 2) {
            Some(b"//") => {
                index += bytes[index..]
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .unwrap_or(bytes.len() - index);
            }
            Some(b"/*") => {
                index += 2;
                let mut depth = 1;
                while depth > 0 && index < bytes.len() {
                    match bytes.get(index..index + 2) {
                        Some(b"/*") => {
                            depth += 1;
                            index += 2;
                        }
                        Some(b"*/") => {
                            depth -= 1;
                            index += 2;
                        }
                        _ => index += 1,
                    }
                }
            }
            _ => {
                let start = index;
                let Some(ch) = source[index..].chars().next() else {
                    break;
                };
                index += ch.len_utf8();
                if blank(ch) {
                    continue;
                }
                if word(ch) {
                    index += source[index..]
                        .chars()
                        .take_while(|&next| word(next))
                        .map(char::len_utf8)
                        .sum::<usize>();
                }
                ranges.push(start..index);
            }
        }
    }
    ranges
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
