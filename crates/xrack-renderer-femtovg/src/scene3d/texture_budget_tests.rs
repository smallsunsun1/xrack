use std::sync::Arc;

use xrack_layout::Interaction;
use xrack_runtime::{
    Color, Length, Node, RecordList, RecordValue, Value,
    material_shader::prepare_capture_augment,
    scene_effect::{record_captures, record_passes},
    scene3d::{record_lights, record_objects},
};

use crate::{RenderDevice, Renderer};

fn row<const N: usize>(fields: [(&str, Value); N]) -> RecordValue {
    RecordValue::new(
        "Row",
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}

fn scene(coated: bool) -> Node {
    let material = if coated {
        row([("clearcoat", Value::Float(0.5))])
    } else {
        row([("unlit", Value::Bool(true))])
    };
    let model = record_objects(
        &RecordList::new(
            "Row",
            vec![row([
                ("id", Value::String("box".into())),
                ("mesh", Value::String("primitive:box".into())),
                ("x", Value::Float(0.0)),
                ("y", Value::Float(0.0)),
                ("z", Value::Float(0.0)),
                ("yaw", Value::Float(0.0)),
                ("pitch", Value::Float(0.0)),
                ("roll", Value::Float(0.0)),
                ("color", Value::Color(Color(0xffffffff))),
                ("scale-x", Value::Float(2.0)),
                ("scale-y", Value::Float(2.0)),
                ("scale-z", Value::Float(2.0)),
                ("material", Value::Record(material)),
            ])],
        ),
        |_| None,
        |_| None,
    );
    let colors = [
        ("red", Color(0xff0000ff)),
        ("green", Color(0x00ff00ff)),
        ("blue", Color(0x0000ffff)),
        ("overflow", Color(0xffffffff)),
    ];
    let mut captures = colors
        .into_iter()
        .map(|(name, color)| {
            row([
                ("name", Value::String(name.into())),
                ("renderables", Value::String("none".into())),
                ("background", Value::Color(color)),
            ])
        })
        .collect::<Vec<_>>();
    let textures = colors
        .into_iter()
        .map(|(name, _)| row([("capture", Value::String(name.into()))]))
        .collect();
    captures.push(row([
        ("name", Value::String("surface".into())),
        ("renderables", Value::String("opaque".into())),
        ("augment-shader", Value::String("augment".into())),
        (
            "augment-textures",
            Value::RecordList(RecordList::new("Row", textures)),
        ),
    ]));
    let shader: Arc<str> = prepare_capture_augment(
        "",
        r#"
        fn xrack_augment_scene_capture_augment(
            input: XrMaterialInput, surface: XrMaterialSurface, index: u32,
        ) -> vec4<f32> {
            let uv = vec2(0.5);
            let r = capture_augment_texture(0u, uv).r;
            let g = capture_augment_texture(1u, uv).g;
            let b = capture_augment_texture(2u, uv).b;
            let overflow = capture_augment_has_texture(3u);
            return vec4(r, g, select(b, 0.0, overflow), 1.0);
        }
        "#,
    )
    .into();
    let captures = record_captures(
        &RecordList::new("Row", captures),
        |_| None,
        |key| (key == "augment").then(|| shader.clone()),
    );
    let reads = RecordList::new(
        "Row",
        vec![row([
            ("capture", Value::String("surface".into())),
            ("attachment", Value::String("material-data".into())),
        ])],
    );
    let passes = record_passes(
        &RecordList::new(
            "Row",
            vec![row([
                (
                    "shader",
                    Value::String(
                        "fn scene_effect(uv: vec2<f32>) -> vec4<f32> { return scene_buffer_color(0u, uv); }"
                            .into(),
                    ),
                ),
                ("buffers", Value::RecordList(reads)),
            ])],
        ),
        |_| None,
    );

    let mut view = Node::new("SceneView3D", None);
    view.properties.insert("width", Length(64.0).into());
    view.properties.insert("height", Length(64.0).into());
    view.properties.insert("model", Value::SceneObjects(model));
    view.properties
        .insert("scene-captures", Value::SceneCaptures(captures));
    view.properties
        .insert("scene-effects", Value::SceneEffects(passes));
    view.properties.insert(
        "render-overrides",
        Value::String("disable-internal-passes".into()),
    );
    let mut window = Node::new("Window", None);
    window.children.push(view);
    window
}

#[test]
#[ignore = "requires a graphics adapter to exercise native GPU pipelines"]
fn a_sixteen_texture_device_uses_only_the_current_material_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let (instance, adapter, device, queue) = pollster::block_on(async {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await?;
        let supported = adapter.limits();
        let limits = wgpu::Limits {
            max_sampled_textures_per_shader_stage: 16,
            max_inter_stage_shader_variables: wgpu::Limits::default()
                .max_inter_stage_shader_variables
                .min(supported.max_inter_stage_shader_variables),
            ..wgpu::Limits::default().using_resolution(supported)
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: limits,
                ..Default::default()
            })
            .await?;
        Ok::<_, Box<dyn std::error::Error>>((instance, adapter, device, queue))
    })?;
    assert_eq!(device.limits().max_sampled_textures_per_shader_stage, 16);
    let mut renderer = Renderer::with_device(RenderDevice {
        instance,
        adapter,
        device,
        queue,
        system_fonts: Default::default(),
    })?;
    let input = Interaction::default();
    let simple = renderer.snapshot(&scene(false), 64, 64, 1.0, &input)?;
    assert_eq!(simple.pixel(32, 32), Some([255, 255, 255, 255]));
    assert_eq!(renderer.scene_draw_stats().scene_capture_passes, 4);
    let coated = renderer.snapshot(&scene(true), 64, 64, 1.0, &input)?;
    assert_eq!(coated.pixel(32, 32), Some([0, 0, 0, 255]));
    assert_eq!(renderer.scene_draw_stats().scene_capture_passes, 5);
    let mixed = |precision: &str| {
        row([
            ("kind", Value::String("directional".into())),
            ("x", Value::Float(0.0)),
            ("y", Value::Float(0.0)),
            ("z", Value::Float(0.0)),
            ("yaw", Value::Float(0.0)),
            ("elevation", Value::Float(30.0)),
            ("outer-angle", Value::Float(30.0)),
            ("inner-angle", Value::Float(0.0)),
            ("range", Value::Float(0.0)),
            ("intensity", Value::Float(1.0)),
            ("color", Value::Color(Color(0xffffffff))),
            ("shadow-map-precision", Value::String(precision.into())),
            ("shadow-filter", Value::String("pcf64".into())),
            ("shadow-world-radius", Value::Float(0.5)),
        ])
    };
    let lights = record_lights(&RecordList::new(
        "Row",
        vec![mixed("depth32"), mixed("depth16")],
    ));
    let mut mixed = scene(true);
    let properties = &mut mixed.children[0].properties;
    properties.insert("shadows", Value::Bool(true));
    properties.insert("shadow-map-size", Value::Int(128));
    properties.insert("lights", Value::SceneLights(lights));
    properties.remove("render-overrides");
    let snapshot = renderer.snapshot(&mixed, 64, 64, 1.0, &input)?;
    assert!(snapshot.pixel(32, 32).is_some());
    let stats = renderer.scene_draw_stats();
    // Both native and packed depth fit without taking another sampled texture
    // from the coated material, the environment, or the capture.
    assert!(stats.shadow_packed_passes > 0);
    assert_eq!(stats.shadow_atlas_bytes, 128 * 128 * (4 + 2));
    Ok(())
}
