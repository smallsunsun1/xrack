use clap::{Args, ValueEnum};
use std::{collections::BTreeSet, fmt::Write as _, fs, path::Path};

use xrack_compiler::CompilerError;
use xrack_interpreter::Program;
use xrack_resources::lightmap_bake::{BakeOptions, BakedAtlas, bake_lightmaps};
use xrack_runtime::{
    Node, Value,
    scene3d::{LightmapMode, MAIN_LAYER, viewport_lighting},
};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Mode {
    /// Bake ambient and bounced light; leave direct diffuse lighting live.
    Indirect,
    /// Bake direct and indirect diffuse lighting.
    #[default]
    Diffuse,
}

#[derive(Debug, Args)]
pub struct Arguments {
    /// Component to instantiate when baking (required if the file exports several).
    #[arg(long, value_name = "COMPONENT")]
    bake_component: Option<String>,

    /// SceneView3D ID to bake (required if the component contains several).
    #[arg(long, value_name = "ID")]
    bake_viewport: Option<String>,

    /// Receiver ID. Repeat to bake objects with authored or generated lightmap UVs.
    #[arg(long = "bake-receiver", value_name = "ID")]
    bake_receivers: Vec<String>,

    /// Interior texture resolution per receiver.
    #[arg(long, default_value_t = 64)]
    bake_size: u32,

    /// Hemisphere paths per texel.
    #[arg(long, default_value_t = 32)]
    bake_samples: u32,

    /// Maximum diffuse bounces per path.
    #[arg(long, default_value_t = 1)]
    bake_bounces: u8,

    /// Ray origin offset in world units.
    #[arg(long, default_value_t = 0.002)]
    bake_bias: f32,

    /// Lighting terms stored by the bake.
    #[arg(long, value_enum, default_value_t)]
    bake_mode: Mode,
}

pub fn run(program: &Program, arguments: &Arguments, output: &Path) -> Result<(), CompilerError> {
    let fail = |message: &str| CompilerError::Resources(message.into());
    let component = if let Some(component) = &arguments.bake_component {
        component.as_str()
    } else {
        let mut components = program.exported_components();
        let only = components
            .next()
            .ok_or_else(|| fail("a bake needs a component"))?;
        if components.next().is_some() {
            return Err(fail("select a component with --bake-component"));
        }
        only
    };
    let root = program
        .create(component)
        .and_then(|component| component.try_render())
        .map_err(|error| CompilerError::Resources(error.to_string()))?;
    let mut scenes = Vec::new();
    find_scenes(&root, arguments.bake_viewport.as_deref(), &mut scenes);
    let [viewport] = scenes.as_slice() else {
        return Err(fail(if scenes.is_empty() {
            "no matching visible SceneView3D was found"
        } else {
            "select a viewport with --bake-viewport"
        }));
    };
    let Some(Value::SceneObjects(model)) = viewport.properties.get("model") else {
        return Err(fail("SceneView3D has no model to bake"));
    };
    let mut layers = MAIN_LAYER;
    if let Some(camera) = viewport.text("camera").filter(|name| !name.is_empty()) {
        layers = model
            .camera(camera, [1.0, 1.0])
            .ok_or_else(|| fail("SceneView3D references an unknown camera"))?
            .layer_mask();
    }
    if let Some(mask) = viewport.int("layer-mask") {
        layers = mask as u64;
    }
    let (lights, fallback, ambient) = viewport_lighting(viewport, Some(model), layers);
    let lights = lights.as_deref().unwrap_or(std::slice::from_ref(&fallback));
    let atlas = bake_lightmaps(
        model.objects(),
        lights,
        ambient,
        layers,
        &arguments.bake_receivers,
        BakeOptions {
            size: arguments.bake_size,
            samples: arguments.bake_samples,
            bounces: arguments.bake_bounces,
            bias: arguments.bake_bias,
            mode: match arguments.bake_mode {
                Mode::Indirect => LightmapMode::Indirect,
                Mode::Diffuse => LightmapMode::Diffuse,
            },
        },
    )
    .map_err(|error| CompilerError::Resources(error.to_string()))?;
    write_atlas(output, atlas)
}

fn find_scenes<'a>(node: &'a Node, id: Option<&str>, result: &mut Vec<&'a Node>) {
    if !node.boolean("visible", true) {
        return;
    }
    if node.kind == "SceneView3D" && id.is_none_or(|id| node.id == Some(id)) {
        result.push(node);
    }
    for child in &node.children {
        find_scenes(child, id, result);
    }
}

fn write_atlas(output: &Path, atlas: BakedAtlas) -> Result<(), CompilerError> {
    let mut xr = String::from(
        "export struct BakedLightmapRegion { u: float, v: float, width: float, height: float }\n\n\
         export global BakedLightmaps {\n    out property <image> atlas: @image-url(\"atlas.png\");\n",
    );
    let mode = match atlas.mode {
        LightmapMode::Indirect => "indirect",
        LightmapMode::Diffuse => "diffuse",
    };
    let _ = writeln!(
        xr,
        "    out property <string> mode: \"{mode}\";\n    out property <float> intensity: {:.8};",
        atlas.intensity
    );
    let mut names = BTreeSet::new();
    let regions: Vec<_> = atlas.regions.iter().enumerate().map(|(index, region)| {
        let mut name = String::from("region-");
        for letter in region.id.chars() {
            name.push(if letter.is_ascii_alphanumeric() { letter.to_ascii_lowercase() } else { '-' });
        }
        // XRack's field identifiers must contain a name after each hyphen.
        name = name.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
        if !names.insert(name.clone()) {
            let prefix = name.clone();
            let mut suffix = index + 1;
            while !names.insert(name.clone()) {
                name = format!("{prefix}-{suffix}");
                suffix += 1;
            }
        }
        let [u, v, width, height] = region.atlas;
        let _ = writeln!(
            xr,
            "    out property <BakedLightmapRegion> {name}: BakedLightmapRegion {{ u: {u:.10}, v: {v:.10}, width: {width:.10}, height: {height:.10} }};"
        );
        serde_json::json!({"id": region.id, "property": name, "region": region.atlas})
    }).collect();
    xr.push_str("}\n");
    let manifest = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 1, "atlas": "atlas.png", "module": "lighting.xr",
        "mode": mode, "intensity": atlas.intensity, "regions": regions,
    }))?;

    let io = |path: &Path, source| CompilerError::Io {
        path: path.into(),
        source,
    };
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
    }
    // Claim the directory before writing generated assets, as for --emit package.
    fs::create_dir(output).map_err(|error| io(output, error))?;
    let files = [
        ("atlas.png", atlas.png),
        ("lighting.xr", xr.into_bytes()),
        ("lightmaps.json", manifest),
    ];
    for (name, data) in files {
        let path = output.join(name);
        if let Err(error) = fs::write(&path, data) {
            let _ = fs::remove_dir_all(output);
            return Err(io(&path, error));
        }
    }
    Ok(())
}
