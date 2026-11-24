use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    sync::Arc,
};

use xrack_runtime::material_shader::{
    INTERFACE, MaterialHooks, VALIDATION_TEXTURES, VERTEX_APPLICATION,
};
use xrack_runtime::scene_effect::CAPTURE_POSITION;

use super::deform::shader_source;

struct Program<K> {
    source: Arc<str>,
    module: Option<wgpu::ShaderModule>,
    variants: HashMap<K, wgpu::RenderPipeline>,
}

pub(super) struct VertexPipelines<K> {
    source: &'static str,
    module: wgpu::ShaderModule,
    variants: HashMap<K, wgpu::RenderPipeline>,
    programs: VecDeque<Program<K>>,
}

impl<K: Eq + Hash> VertexPipelines<K> {
    pub fn new(device: &wgpu::Device, source: &'static str) -> Self {
        Self {
            source,
            module: module(device, &MaterialHooks::default().prepare(""), source),
            variants: HashMap::new(),
            programs: VecDeque::new(),
        }
    }

    pub fn get(
        &mut self,
        device: &wgpu::Device,
        source: Option<&Arc<str>>,
        key: K,
        create: impl Fn(&wgpu::ShaderModule) -> wgpu::RenderPipeline,
    ) -> &wgpu::RenderPipeline {
        if let Some(source) = source {
            let index = self
                .programs
                .iter()
                .position(|program| program.source == *source);
            if index != Some(0) {
                let program = index.and_then(|index| self.programs.remove(index)).unwrap_or_else(|| {
                    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                    let module = module(device, source, self.source);
                    let module = match pollster::block_on(scope.pop()) {
                        Some(error) => {
                            tracing::warn!(%error, "XRack disabled an invalid vertex program in an auxiliary pass");
                            None
                        }
                        None => Some(module),
                    };
                    Program { source: source.clone(), module, variants: HashMap::new() }
                });
                self.programs.truncate(15);
                self.programs.push_front(program);
            }
            if let Some(program) = self.programs.front_mut() {
                return program.variants.entry(key).or_insert_with(|| {
                    let Some(module) = &program.module else {
                        return create(&self.module);
                    };
                    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                    let pipeline = create(module);
                    if let Some(error) = pollster::block_on(scope.pop()) {
                        tracing::warn!(%error, "XRack fell back to undeformed geometry in an auxiliary pass");
                        create(&self.module)
                    } else {
                        pipeline
                    }
                });
            }
        }
        self.variants
            .entry(key)
            .or_insert_with(|| create(&self.module))
    }
}

fn module(device: &wgpu::Device, program: &str, pass: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack 3D vertex program"),
        source: shader_source(&format!(
            "{program}\n{INTERFACE}\n{CAPTURE_POSITION}\n{VALIDATION_TEXTURES}\n{VERTEX_APPLICATION}\n{pass}"
        )),
    })
}
