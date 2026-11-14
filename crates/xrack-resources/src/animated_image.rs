use rgb::ComponentBytes;
use std::io::Cursor;
use xrack_assets::{MAX_ASSET_BYTES, validate_image_size};

pub(crate) const MAX_DECODED_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_FRAMES: usize = 512;

pub(crate) struct EncodedGif {
    pub size: [u32; 2],
    pub frames: Vec<(u32, Vec<u8>)>,
    /// Total passes, or zero for an infinite loop.
    pub plays: u32,
}

/// Flatten the GIF disposal and blending rules at build time. Each frame is
/// independently decodable, so playback can skip frames without replaying deltas.
pub(crate) fn encode_gif_frames(bytes: &[u8]) -> Result<EncodedGif, String> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    options.check_frame_consistency(true);
    let mut reader = options
        .read_info(Cursor::new(bytes))
        .map_err(|error| error.to_string())?;
    let [width, height] =
        validate_image_size([u32::from(reader.width()), u32::from(reader.height())])?;
    let mut screen = gif_dispose::Screen::new_decoder(&reader);
    let pixels_per_frame = u64::from(width) * u64::from(height);
    let limit = MAX_FRAMES.min((MAX_DECODED_BYTES / (pixels_per_frame * 4)) as usize);
    let mut frames = Vec::new();
    let mut end_ms = 0_u32;
    let mut encoded_bytes = 0_u64;
    while let Some(frame) = reader
        .read_next_frame()
        .map_err(|error| error.to_string())?
    {
        if frames.len() == limit {
            return Err(format!(
                "GIF exceeds {MAX_FRAMES} frames or 64 MiB of composited pixels"
            ));
        }
        screen
            .blit_frame(frame)
            .map_err(|error| error.to_string())?;
        // Zero and one hundredth of a second are frequently used as "no delay".
        // Coerce these to a useful cadence instead of a zero-time frame loop.
        end_ms += if frame.delay < 2 {
            100
        } else {
            u32::from(frame.delay) * 10
        };
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(screen.pixels_rgba().buf().as_bytes())
            .map_err(|error| error.to_string())?;
        writer.finish().map_err(|error| error.to_string())?;
        encoded_bytes += png.len() as u64;
        if encoded_bytes > MAX_ASSET_BYTES {
            return Err("GIF exceeds 16 MiB of embedded PNG frames".into());
        }
        frames.push((end_ms, png));
    }
    if frames.is_empty() {
        return Err("GIF contains no frames".into());
    }
    let plays = match reader.repeat() {
        gif::Repeat::Infinite => 0,
        gif::Repeat::Finite(repeats) => u32::from(repeats) + 1,
    };
    Ok(EncodedGif {
        size: [width, height],
        frames,
        plays,
    })
}
