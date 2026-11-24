use super::{Attachment, HDR_FORMAT, attachment_view};

pub(super) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

#[derive(Default)]
pub(super) struct StencilBuffers {
    depth: Option<Attachment>,
    color: Option<Attachment>,
    transfer: Option<wgpu::Buffer>,
}

pub(super) struct StencilPass {
    pub view: wgpu::TextureView,
    clear: u8,
    load: bool,
    reference: u8,
}

impl StencilPass {
    pub fn operations(&self, first: bool) -> wgpu::Operations<u32> {
        wgpu::Operations {
            load: if first && !self.load {
                wgpu::LoadOp::Clear(u32::from(self.clear))
            } else {
                wgpu::LoadOp::Load
            },
            store: wgpu::StoreOp::Store,
        }
    }

    pub fn set_reference(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_stencil_reference(u32::from(self.reference));
    }
}

impl StencilBuffers {
    pub fn color(&mut self, device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
        Attachment::get(&mut self.color, device, size, HDR_FORMAT, 1, true).clone()
    }

    pub fn prepare(
        &mut self,
        (device, encoder): (&wgpu::Device, &mut wgpu::CommandEncoder),
        size: [u32; 2],
        exported: Option<&wgpu::TextureView>,
        source: Option<&wgpu::TextureView>,
        (clear, reference): (u8, u8),
    ) -> (StencilPass, bool) {
        let view = exported.cloned().unwrap_or_else(|| {
            if self
                .depth
                .as_ref()
                .is_some_and(|target| target.size != size)
            {
                self.depth = None;
            }
            self.depth
                .get_or_insert_with(|| Attachment {
                    size,
                    view: attachment_view(
                        device,
                        size,
                        FORMAT,
                        1,
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::COPY_DST
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        "XRack capture stencil scratch",
                    ),
                })
                .view
                .clone()
        });
        let load = source.is_some_and(|source| {
            self.copy(device, encoder, size, source.texture(), view.texture())
        });
        (
            StencilPass {
                view,
                clear,
                load,
                reference,
            },
            load,
        )
    }

    fn copy(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        source: &wgpu::Texture,
        destination: &wgpu::Texture,
    ) -> bool {
        if [source.width(), source.height()] != size || !can_copy(device, size) {
            return false;
        }
        if source == destination {
            return true;
        }
        let [width, height] = size;
        let bytes_per_row = aligned_row(width);
        let bytes = u64::from(bytes_per_row) * u64::from(height);
        if self
            .transfer
            .as_ref()
            .is_none_or(|buffer| buffer.size() < bytes)
        {
            self.transfer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("XRack capture stencil transfer"),
                size: bytes,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(buffer) = &self.transfer else {
            return false;
        };
        let buffer = wgpu::TexelCopyBufferInfo {
            buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
        };
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        // WebGPU texture-to-texture copies require all depth/stencil aspects.
        // The intermediate buffer moves only the byte-wide stencil aspect.
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: source,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::StencilOnly,
            },
            buffer,
            extent,
        );
        encoder.copy_buffer_to_texture(
            buffer,
            wgpu::TexelCopyTextureInfo {
                texture: destination,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::StencilOnly,
            },
            extent,
        );
        true
    }
}

pub(super) fn can_copy(device: &wgpu::Device, [width, height]: [u32; 2]) -> bool {
    u64::from(aligned_row(width)) * u64::from(height) <= device.limits().max_buffer_size
}

fn aligned_row(width: u32) -> u32 {
    let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    width.div_ceil(alignment) * alignment
}
