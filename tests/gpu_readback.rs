use xrack_renderer_femtovg::Renderer;

pub(super) fn read_texture(
    renderer: &Renderer,
    target: &wgpu::Texture,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (width, height) = (target.width(), target.height());
    let row_bytes = width * 4;
    let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let stride = row_bytes.div_ceil(alignment) * alignment;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("GPU test readback"),
        size: u64::from(stride) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        target.size(),
    );
    renderer.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    renderer.device.poll(wgpu::PollType::wait_indefinitely())?;
    receiver.recv_timeout(std::time::Duration::from_secs(5))??;
    Ok(buffer
        .slice(..)
        .get_mapped_range()?
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..row_bytes as usize].iter().copied())
        .collect())
}
