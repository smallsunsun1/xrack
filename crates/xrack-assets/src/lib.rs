//! Image decoding and SVG path parsing shared by the compiler and renderer.
use resvg::{tiny_skia, usvg};
use std::io::Cursor;
use svgtypes::{SimplePathSegment, SimplifyingPathParser};
pub use tiny_skia::{Path as VectorPath, PathSegment};
pub use xrack_runtime::ImageFormat;
use xrack_runtime::{Image, ImageAsset, VectorSegment};

pub const MAX_IMAGE_DIMENSION: u32 = 4096;
pub const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;

pub fn vector_bounds(path: &VectorPath) -> [f32; 4] {
    let bounds = path.compute_tight_bounds().unwrap_or_else(|| path.bounds());
    [bounds.x(), bounds.y(), bounds.width(), bounds.height()]
}

pub fn vector_segments(path: &VectorPath) -> impl Iterator<Item = VectorSegment> + '_ {
    let point = |p: tiny_skia::Point| [p.x, p.y];
    path.segments().map(move |segment| match segment {
        PathSegment::MoveTo(p) => VectorSegment::MoveTo(point(p)),
        PathSegment::LineTo(p) => VectorSegment::LineTo(point(p)),
        PathSegment::QuadTo(c, p) => VectorSegment::QuadTo(point(c), point(p)),
        PathSegment::CubicTo(c1, c2, p) => VectorSegment::CubicTo(point(c1), point(c2), point(p)),
        PathSegment::Close => VectorSegment::Close,
    })
}

pub struct Raster {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

pub fn validate_image_size(size: [u32; 2]) -> Result<[u32; 2], String> {
    if size
        .into_iter()
        .all(|n| (1..=MAX_IMAGE_DIMENSION).contains(&n))
    {
        Ok(size)
    } else {
        Err(format!(
            "image dimensions must be between 1 and {MAX_IMAGE_DIMENSION} pixels"
        ))
    }
}

pub fn image_size(format: ImageFormat, bytes: &[u8]) -> Result<[u32; 2], String> {
    match format {
        ImageFormat::Png => Ok(decode_png(bytes)?.size),
        ImageFormat::Svg => {
            let size = parse_svg(bytes)?.size().to_int_size();
            validate_image_size([size.width(), size.height()])
        }
    }
}

pub fn decode_png(bytes: &[u8]) -> Result<Raster, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    validate_image_size([reader.info().width, reader.info().height])?;
    let mut data = vec![0; reader.output_buffer_size().ok_or("invalid PNG size")?];
    let frame = reader
        .next_frame(&mut data)
        .map_err(|error| error.to_string())?;
    data.truncate(frame.buffer_size());

    let expand = |channels: usize, rgba: fn(&[u8]) -> [u8; 4]| {
        data.chunks_exact(channels).flat_map(rgba).collect()
    };
    let rgba = match frame.color_type {
        png::ColorType::Rgba => data,
        png::ColorType::Rgb => expand(3, |p| [p[0], p[1], p[2], 255]),
        png::ColorType::Grayscale => expand(1, |p| [p[0], p[0], p[0], 255]),
        png::ColorType::GrayscaleAlpha => expand(2, |p| [p[0], p[0], p[0], p[1]]),
        png::ColorType::Indexed => return Err("PNG palette expansion failed".into()),
    };
    Ok(Raster {
        size: [frame.width, frame.height],
        rgba,
    })
}

fn parse_svg(bytes: &[u8]) -> Result<usvg::Tree, String> {
    let options = usvg::Options {
        // Embedded resources are reproducible; external URLs and files are not.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            ..Default::default()
        },
        ..Default::default()
    };
    usvg::Tree::from_data(bytes, &options).map_err(|error| error.to_string())
}

/// Rasterize at the requested device pixel size. The result has premultiplied alpha.
pub fn rasterize_svg(bytes: &[u8], size: [u32; 2]) -> Result<Raster, String> {
    rasterize_svg_region(bytes, size, None)
}

/// Fit an embedded image into an icon canvas with transparent padding and premultiplied RGBA.
pub fn rasterize_icon(image: Image, size: [u32; 2]) -> Result<Option<Raster>, String> {
    rasterize_icon_asset(&image.into(), size)
}

pub fn rasterize_icon_asset(image: &ImageAsset, size: [u32; 2]) -> Result<Option<Raster>, String> {
    let frame = image.frame(0);
    let Some(pixels) = frame.pixels() else {
        return Ok(None);
    };
    let (format, bytes, intrinsic) = (pixels.format, pixels.bytes, pixels.size);
    let [width, height] = validate_image_size(size)?;
    let fit = |[source_width, source_height]: [u32; 2]| {
        let scale = (f64::from(width) / f64::from(source_width))
            .min(f64::from(height) / f64::from(source_height));
        [
            (f64::from(source_width) * scale)
                .round()
                .clamp(1.0, f64::from(width)) as u32,
            (f64::from(source_height) * scale)
                .round()
                .clamp(1.0, f64::from(height)) as u32,
        ]
    };
    let (source, fitted) = match format {
        ImageFormat::Svg => {
            let fitted = fit(validate_image_size(intrinsic)?);
            (rasterize_svg(bytes, fitted)?, fitted)
        }
        ImageFormat::Png => {
            let mut source = decode_png(bytes)?;
            for pixel in source.rgba.chunks_exact_mut(4) {
                let alpha = u16::from(pixel[3]);
                for channel in &mut pixel[..3] {
                    *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
                }
            }
            let fitted = fit(source.size);
            (source, fitted)
        }
    };
    let [source_width, source_height] = source.size;
    let source_size = tiny_skia::IntSize::from_wh(source_width, source_height)
        .ok_or("invalid icon source size")?;
    let source =
        tiny_skia::Pixmap::from_vec(source.rgba, source_size).ok_or("invalid icon pixel buffer")?;
    let mut canvas = tiny_skia::Pixmap::new(width, height).ok_or("invalid icon canvas size")?;
    let transform = tiny_skia::Transform::from_row(
        fitted[0] as f32 / source_width as f32,
        0.0,
        0.0,
        fitted[1] as f32 / source_height as f32,
        ((width - fitted[0]) / 2) as f32,
        ((height - fitted[1]) / 2) as f32,
    );
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bicubic,
        ..Default::default()
    };
    canvas.draw_pixmap(0, 0, source.as_ref(), &paint, transform, None);
    Ok(Some(Raster {
        size,
        rgba: canvas.take(),
    }))
}

/// Encode a fitted icon for native menus that accept PNG data.
pub fn rasterize_icon_png(image: Image, size: [u32; 2]) -> Result<Vec<u8>, String> {
    rasterize_icon_asset_png(&image.into(), size)
}

pub fn rasterize_icon_asset_png(image: &ImageAsset, size: [u32; 2]) -> Result<Vec<u8>, String> {
    let Some(raster) = rasterize_icon_asset(image, size)? else {
        return Ok(Vec::new());
    };
    let size =
        tiny_skia::IntSize::from_wh(raster.size[0], raster.size[1]).ok_or("invalid icon size")?;
    tiny_skia::Pixmap::from_vec(raster.rgba, size)
        .ok_or("invalid icon pixel buffer")?
        .encode_png()
        .map_err(|error| error.to_string())
}

/// Rasterize a clipped SVG view in the same integer source coordinates as
/// `image_size`. Only the selected region is allocated at the requested density.
pub fn rasterize_svg_region(
    bytes: &[u8],
    size: [u32; 2],
    region: Option<[u32; 4]>,
) -> Result<Raster, String> {
    let [width, height] = validate_image_size(size)?;
    let tree = parse_svg(bytes)?;
    let source = tree.size();
    let transform = if let Some([x, y, w, h]) = region {
        let natural = source.to_int_size();
        if w == 0
            || h == 0
            || x.saturating_add(w) > natural.width()
            || y.saturating_add(h) > natural.height()
        {
            return Err("SVG source clip is outside its intrinsic size".into());
        }
        let sx = width as f32 / w as f32;
        let sy = height as f32 / h as f32;
        tiny_skia::Transform::from_row(
            sx * natural.width() as f32 / source.width(),
            0.0,
            0.0,
            sy * natural.height() as f32 / source.height(),
            -(x as f32) * sx,
            -(y as f32) * sy,
        )
    } else {
        tiny_skia::Transform::from_scale(
            width as f32 / source.width(),
            height as f32 / source.height(),
        )
    };
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("invalid SVG raster size")?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(Raster {
        size,
        rgba: pixmap.take(),
    })
}

/// Normalize relative commands, arcs and shorthand curves into simple segments.
pub fn parse_path(commands: &str) -> Result<Option<VectorPath>, String> {
    let coordinate = |value: f64| {
        if value.abs() <= 1_000_000.0 {
            Ok(value as f32)
        } else {
            Err("path coordinate must be finite and within ±1000000".to_owned())
        }
    };
    let mut path = tiny_skia::PathBuilder::new();
    for segment in SimplifyingPathParser::from(commands) {
        match segment.map_err(|error| error.to_string())? {
            SimplePathSegment::MoveTo { x, y } => path.move_to(coordinate(x)?, coordinate(y)?),
            SimplePathSegment::LineTo { x, y } => path.line_to(coordinate(x)?, coordinate(y)?),
            SimplePathSegment::Quadratic { x1, y1, x, y } => {
                path.quad_to(
                    coordinate(x1)?,
                    coordinate(y1)?,
                    coordinate(x)?,
                    coordinate(y)?,
                );
            }
            SimplePathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                path.cubic_to(
                    coordinate(x1)?,
                    coordinate(y1)?,
                    coordinate(x2)?,
                    coordinate(y2)?,
                    coordinate(x)?,
                    coordinate(y)?,
                );
            }
            SimplePathSegment::ClosePath => path.close(),
        }
    }
    Ok(path.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_icon_premultiplies_pixels_and_preserves_padding() {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 0, 0, 128, 23, 142, 255, 0])
            .unwrap();
        let image = Image::Embedded {
            format: ImageFormat::Png,
            bytes: bytes.leak(),
            size: [2, 1],
        };
        let icon = rasterize_icon(image, [2, 3]).unwrap().unwrap();
        assert_eq!(icon.size, [2, 3]);
        assert_eq!(&icon.rgba[..8], &[0; 8]);
        assert_eq!(&icon.rgba[8..16], &[128, 0, 0, 128, 0, 0, 0, 0]);
        assert_eq!(&icon.rgba[16..], &[0; 8]);
    }

    #[test]
    fn svg_icon_preserves_aspect_ratio_and_premultiplied_alpha() {
        let bytes = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4">
            <rect width="8" height="4" fill="red" fill-opacity="0.5"/>
        </svg>"#;
        let image = Image::Embedded {
            format: ImageFormat::Svg,
            bytes,
            size: [8, 4],
        };
        let loaded = ImageAsset::Loaded(xrack_runtime::LoadedImage::new(
            ImageFormat::Svg,
            bytes.to_vec(),
            [8, 4],
        ));
        let icon = rasterize_icon(image, [16, 16]).unwrap().unwrap();
        let loaded_icon = rasterize_icon_asset(&loaded, [16, 16]).unwrap().unwrap();
        assert_eq!(icon.rgba, loaded_icon.rgba);
        assert_eq!(icon.size, [16, 16]);
        let rows = icon.rgba.chunks_exact(64).collect::<Vec<_>>();
        assert_eq!(rows[3], &[0; 64]);
        assert_eq!(rows[12], &[0; 64]);
        assert_eq!(&rows[4][..4], &[128, 0, 0, 128]);
        assert_eq!(&rows[11][60..], &[128, 0, 0, 128]);
    }
}
