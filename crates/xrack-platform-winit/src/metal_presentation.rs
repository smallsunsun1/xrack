//! Opt-in presentation feedback from the drawable actually submitted to Metal.
use objc2::{msg_send, rc::Retained, runtime::AnyObject};
use std::time::Instant;
use wgpu::hal::{api::Metal, metal};
use xrack_runtime::PresentationFeedback;

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {
    fn CACurrentMediaTime() -> f64;
}

pub(crate) struct Frame {
    pub(crate) texture: wgpu::Texture,
    drawable: Retained<AnyObject>,
}

pub(crate) fn acquire(surface: &wgpu::Surface<'_>, device: &wgpu::Device) -> Option<Frame> {
    let config = surface.get_configuration()?;
    // Only borrow the layer from the matching backend. We acquire a drawable
    // instead of calling Surface::get_current_texture for this frame.
    let surface = unsafe { surface.as_hal::<Metal>() }?;
    let layer = surface.render_layer().lock();
    // Transactional layers use a different present scheduling contract.
    if layer.presentsWithTransaction() {
        return None;
    }
    let drawable: Option<Retained<AnyObject>> = unsafe { msg_send![&**layer, nextDrawable] };
    let drawable = drawable?;
    drop(layer);
    drop(surface);
    let raw: Retained<_> = unsafe { msg_send![&*drawable, texture] };
    let width: usize = unsafe { msg_send![&*raw, width] };
    let height: usize = unsafe { msg_send![&*raw, height] };
    if width != config.width as usize || height != config.height as usize {
        return None;
    }
    let raw_type = unsafe { msg_send![&*raw, textureType] };
    let size = wgpu::Extent3d {
        width: config.width,
        height: config.height,
        depth_or_array_layers: 1,
    };
    // The retained drawable's texture belongs to the configured surface's
    // device and has the verified extent and matching swapchain format.
    let raw = unsafe {
        metal::Device::texture_from_raw(raw, config.format, raw_type, 1, 1, size.into(), None)
    };
    let texture = unsafe {
        device.create_texture_from_hal::<Metal>(
            raw,
            &wgpu::TextureDescriptor {
                label: Some("XRack Metal drawable with presentation feedback"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: config.format,
                usage: config.usage,
                view_formats: &[],
            },
            wgpu::TextureUses::UNINITIALIZED,
        )
    };
    Some(Frame { texture, drawable })
}

impl Frame {
    pub(crate) fn present(
        self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        feedback: Option<PresentationFeedback>,
        input: Option<Instant>,
    ) -> Result<(), String> {
        // Both clocks are sampled together on the UI thread. The callback
        // contains only Send data and never accesses live UI state.
        let now = unsafe { CACurrentMediaTime() };
        let input = input.map(|input| now - input.elapsed().as_secs_f64());
        let handler = block2::RcBlock::new(move |drawable: *mut AnyObject| {
            let Some(drawable) = (unsafe { drawable.as_ref() }) else {
                return;
            };
            let presented: f64 = unsafe { msg_send![drawable, presentedTime] };
            if presented > 0.0 && presented.is_finite() {
                let latency = input
                    .filter(|input| presented >= *input)
                    .map(|input| ((presented - input) * 1_000_000.0) as u64);
                let host_micros = (presented * 1_000_000.0) as u64;
                if let Some(feedback) = &feedback {
                    feedback.record(host_micros, latency);
                }
                xrack_runtime::ui_trace::frame_presented(host_micros, latency);
            }
        });
        unsafe {
            let _: () = msg_send![&*self.drawable, addPresentedHandler: &*handler];
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("XRack Metal presentation"),
        });
        // The encoder stays owned by wgpu. Attaching a drawable presentation
        // to the raw buffer does not end or commit the encoder.
        let scheduled = unsafe {
            encoder.as_hal_mut::<Metal, _, _>(|encoder| {
                let Some(buffer) = encoder.and_then(|encoder| encoder.raw_command_buffer()) else {
                    return false;
                };
                let _: () = msg_send![buffer, presentDrawable: &*self.drawable];
                true
            })
        };
        if !scheduled {
            return Err("could not schedule Metal presentation".into());
        }
        // wgpu records typed markers for the trace and encodes presentation
        // through the HAL. Each command encoder uses exactly one of those APIs.
        let marker = std::env::var_os("XRACK_GPU_TRACE_DIR").map(|_| {
            let mut marker = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("XRack Metal presentation trace marker"),
            });
            marker.insert_debug_marker("XRack Metal presentation");
            marker.finish()
        });
        queue.submit(marker.into_iter().chain([encoder.finish()]));
        Ok(())
    }
}
