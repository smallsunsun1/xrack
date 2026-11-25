use clap::{Args, ValueEnum};
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs,
    io::{BufReader, BufWriter, Write as _},
    path::Path,
};

use xrack_compiler::CompilerError;
use xrack_interpreter::Program;
use xrack_resources::lightmap_bake::{
    BakeLightInputs, BakeOptions, BakedAtlas, bake_lightmaps_recording, replay_lightmap_samples,
};
use xrack_runtime::{
    Node, Value,
    scene3d::{LightmapMode, MAIN_LAYER, ViewportEnvironment, viewport_lighting},
};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Mode {
    /// Bake ambient and bounced light; leave direct diffuse lighting live.
    Indirect,
    /// Bake direct and indirect diffuse lighting.
    #[default]
    Diffuse,
    /// Bake lighting according to each light record's bake-mode.
    Mixed,
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

    /// Fallback interior texture resolution per receiver (default 64).
    #[arg(long)]
    bake_size: Option<u32>,

    /// Estimate the resolution from world area using this many texels per world unit.
    #[arg(long)]
    bake_texels_per_unit: Option<f64>,

    /// Hemisphere paths per texel (default 32).
    #[arg(long)]
    bake_samples: Option<u32>,

    /// Maximum diffuse bounces per path (default 1).
    #[arg(long)]
    bake_bounces: Option<u8>,

    /// Ray origin offset in world units (default 0.002).
    #[arg(long)]
    bake_bias: Option<f32>,

    /// Geometry-guided indirect denoise passes, from 0 to 4.
    #[arg(long)]
    bake_denoise_passes: Option<u8>,

    /// Indirect irradiance multiplier. Use 0 to bake only direct lighting.
    #[arg(long)]
    bake_indirect_factor: Option<f32>,

    /// Lighting terms stored by the bake (default diffuse).
    #[arg(long, value_enum)]
    bake_mode: Option<Mode>,

    /// Save unfiltered irradiance and geometric guides alongside the atlas.
    #[arg(long)]
    bake_save_samples: bool,
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
    let defaults = BakeOptions::default();
    let passes = arguments
        .bake_denoise_passes
        .map(i64::from)
        .or_else(|| viewport.int("bake-denoise-passes"))
        .unwrap_or(i64::from(defaults.denoise_passes));
    let options = BakeOptions {
        size: arguments.bake_size.unwrap_or(defaults.size),
        texels_per_unit: arguments
            .bake_texels_per_unit
            .unwrap_or(defaults.texels_per_unit),
        samples: arguments.bake_samples.unwrap_or(defaults.samples),
        bounces: arguments.bake_bounces.unwrap_or(defaults.bounces),
        bias: arguments.bake_bias.unwrap_or(defaults.bias),
        denoise_passes: u8::try_from(passes)
            .map_err(|_| fail("bake-denoise-passes must be between 0 and 4"))?,
        indirect_factor: arguments
            .bake_indirect_factor
            .or_else(|| viewport.float("bake-indirect-factor"))
            .unwrap_or(defaults.indirect_factor),
        mode: match arguments.bake_mode.unwrap_or_default() {
            Mode::Indirect => LightmapMode::Indirect,
            Mode::Diffuse => LightmapMode::Diffuse,
            Mode::Mixed => LightmapMode::Mixed,
        },
    };
    publish(output, || {
        let samples_path = output.join("samples.xrlm");
        let mut samples = arguments
            .bake_save_samples
            .then(|| {
                fs::File::create_new(&samples_path)
                    .map(BufWriter::new)
                    .map_err(|source| io_error(&samples_path, source))
            })
            .transpose()?;
        let atlas = bake_lightmaps_recording(
            model.objects(),
            BakeLightInputs {
                lights,
                ambient,
                environment: ViewportEnvironment::from_node(viewport),
            },
            layers,
            &arguments.bake_receivers,
            options,
            samples
                .as_mut()
                .map(|writer| writer as &mut dyn std::io::Write),
        )
        .map_err(|error| CompilerError::Resources(error.to_string()))?;
        if let Some(writer) = &mut samples {
            writer
                .flush()
                .map_err(|source| io_error(&samples_path, source))?;
        }
        write_atlas(output, atlas, options)
    })
}

pub fn replay(input: &Path, arguments: &Arguments, output: &Path) -> Result<(), CompilerError> {
    if arguments.bake_component.is_some()
        || arguments.bake_viewport.is_some()
        || !arguments.bake_receivers.is_empty()
        || arguments.bake_size.is_some()
        || arguments.bake_texels_per_unit.is_some()
        || arguments.bake_samples.is_some()
        || arguments.bake_bounces.is_some()
        || arguments.bake_bias.is_some()
        || arguments.bake_mode.is_some()
        || arguments.bake_save_samples
    {
        return Err(CompilerError::Resources(
            "sample replay accepts only --bake-denoise-passes and --bake-indirect-factor".into(),
        ));
    }
    let file = fs::File::open(input).map_err(|source| io_error(input, source))?;
    let (atlas, options) = replay_lightmap_samples(
        &mut BufReader::new(file),
        arguments.bake_denoise_passes,
        arguments.bake_indirect_factor,
    )
    .map_err(|error| CompilerError::Resources(format!("{}: {error}", input.display())))?;
    publish(output, || write_atlas(output, atlas, options))
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

fn write_atlas(
    output: &Path,
    atlas: BakedAtlas,
    options: BakeOptions,
) -> Result<(), CompilerError> {
    let mut xr = String::from(
        "export struct BakedLightmapRegion { u: float, v: float, width: float, height: float }\n\n\
         export global BakedLightmaps {\n    out property <image> atlas: @image-url(\"atlas.png\");\n",
    );
    let mode = match atlas.mode {
        LightmapMode::Indirect => "indirect",
        LightmapMode::Diffuse => "diffuse",
        LightmapMode::Mixed => "mixed",
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
        serde_json::json!({"id": region.id, "property": name, "region": region.atlas, "resolution": region.resolution})
    }).collect();
    xr.push_str("}\n");
    let manifest = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 1, "atlas": "atlas.png", "atlas_size": atlas.size, "module": "lighting.xr",
        "mode": mode, "intensity": atlas.intensity, "regions": regions,
        "sampling": {"samples": options.samples, "bounces": options.bounces, "bias": options.bias,
            "denoise_passes": options.denoise_passes, "indirect_factor": options.indirect_factor},
    }))?;

    let files = [
        ("atlas.png", atlas.png),
        ("lighting.xr", xr.into_bytes()),
        ("lightmaps.json", manifest),
    ];
    for (name, data) in files {
        let path = output.join(name);
        fs::write(&path, data).map_err(|source| io_error(&path, source))?;
    }
    Ok(())
}

fn publish(
    output: &Path,
    produce: impl FnOnce() -> Result<(), CompilerError>,
) -> Result<(), CompilerError> {
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    }
    // Claim the directory before writing any generated assets.
    fs::create_dir(output).map_err(|source| io_error(output, source))?;
    let result = produce();
    if result.is_err() {
        let _ = fs::remove_dir_all(output);
    }
    result
}

fn io_error(path: &Path, source: std::io::Error) -> CompilerError {
    CompilerError::Io {
        path: path.into(),
        source,
    }
}
