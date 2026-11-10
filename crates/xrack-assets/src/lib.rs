//! Image decoding and SVG path parsing shared by the compiler and renderer.
use resvg::{tiny_skia, usvg};
use std::io::Cursor;
use svgtypes::{SimplePathSegment, SimplifyingPathParser};
pub use tiny_skia::{Path as VectorPath, PathSegment};
pub use xrack_runtime::ImageFormat;
use xrack_runtime::VectorSegment;

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
