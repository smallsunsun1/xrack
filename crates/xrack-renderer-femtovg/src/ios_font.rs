use crate::color_glyphs::GlyphBitmap;
use femtovg::{Canvas, FillRule, Paint, Path, renderer::WGPURenderer};
use objc2_core_foundation::{CFRetained, CFString, CGPoint};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGImageAlphaInfo, CGImageByteOrderInfo, CGPath,
    CGPathElement, CGPathElementType, kCGColorSpaceSRGB,
};
use objc2_core_text::{CTFont, CTFontOrientation, CTFontSymbolicTraits, CTFontTableOptions};
use std::{cell::RefCell, collections::HashMap, ffi::c_void, ptr::NonNull, sync::Arc};
use xrack_layout::Point;

pub(crate) struct NativeFont {
    font: CFRetained<CTFont>,
    pub(crate) data: Arc<[u8]>,
    outlines: RefCell<HashMap<u16, Option<Path>>>,
}

impl NativeFont {
    pub(crate) fn with_post_script_name(name: &str) -> Option<Self> {
        let name = CFString::from_str(name);
        // SAFETY: The null transform is the identity transform.
        let font = unsafe { CTFont::with_name(&name, 1.0, std::ptr::null()) };
        // SAFETY: The font is a retained Core Text object.
        if *unsafe { font.post_script_name() } != *name {
            return None;
        }
        let data = layout_tables(&font)?.into();
        Some(Self {
            font,
            data,
            outlines: RefCell::default(),
        })
    }

    pub(crate) fn draw(
        &self,
        canvas: &mut Canvas<WGPURenderer>,
        id: u16,
        origin: Point,
        paint: &Paint,
    ) {
        let mut outlines = self.outlines.borrow_mut();
        if outlines.len() >= 1024 && !outlines.contains_key(&id) {
            outlines.clear();
        }
        let Some(path) = outlines
            .entry(id)
            .or_insert_with(|| glyph_path(&self.font, id))
        else {
            return;
        };
        let size = paint.font_size();
        canvas.save();
        canvas.translate(origin.x, origin.y);
        canvas.scale(size, -size);
        canvas.fill_path(path, &paint.clone().with_fill_rule(FillRule::NonZero));
        canvas.restore();
    }

    pub(crate) fn rasterize_color_glyph(&self, id: u16, pixels_per_em: u16) -> Option<GlyphBitmap> {
        // SAFETY: The null transform and descriptor retain the original face.
        let font = unsafe {
            self.font
                .copy_with_attributes(f64::from(pixels_per_em), std::ptr::null(), None)
        };
        let glyph = NonNull::from(&id);
        // SAFETY: The single glyph is valid for the resized copy of the shaping face.
        let bounds = unsafe {
            font.bounding_rects_for_glyphs(
                CTFontOrientation::Horizontal,
                glyph,
                std::ptr::null_mut(),
                1,
            )
        };
        let left = bounds.origin.x.floor() - 1.0;
        let bottom = bounds.origin.y.floor() - 1.0;
        let right = (bounds.origin.x + bounds.size.width).ceil() + 1.0;
        let top = (bounds.origin.y + bounds.size.height).ceil() + 1.0;
        let (width, height) = (right - left, top - bottom);
        if !(1.0..=1024.0).contains(&width) || !(1.0..=1024.0).contains(&height) {
            return None;
        }
        let (width, height) = (width as usize, height as usize);
        let mut rgba = vec![0; width * height * 4];
        // SAFETY: kCGColorSpaceSRGB is a system-owned string constant.
        let color_space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
        let bitmap_info =
            CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::PremultipliedLast.0;
        // SAFETY: The pixel buffer stays valid and exclusively owned until the context is released.
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                Some(&color_space),
                bitmap_info,
            )
        }?;
        let origin = CGPoint::new(-left, -bottom);
        // SAFETY: Both arrays contain one element and the context owns its drawing state.
        unsafe { font.draw_glyphs(glyph, NonNull::from(&origin), 1, &context) };
        drop(context);
        // FemtoVG's color-glyph shader takes straight RGBA.
        let mut visible = false;
        for pixel in rgba.chunks_exact_mut(4) {
            let alpha = u16::from(pixel[3]);
            if alpha != 0 {
                visible = true;
                for channel in &mut pixel[..3] {
                    *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        visible.then_some(GlyphBitmap {
            offset: [left as i32, -top as i32],
            size: [width as u32, height as u32],
            rgba,
        })
    }
}

/// Supply the same native face's shaping tables. Core Text supplies the ink
/// separately, so the large bitmap strikes don't need to be copied.
fn layout_tables(font: &CTFont) -> Option<Vec<u8>> {
    let tags = [
        *b"GDEF", *b"GPOS", *b"GSUB", *b"OS/2", *b"cmap", *b"head", *b"hhea", *b"hmtx", *b"kern",
        *b"maxp", *b"name", *b"post",
    ];
    // SAFETY: The retained face supplies its symbolic traits.
    let color = unsafe { font.symbolic_traits() }.contains(CTFontSymbolicTraits::TraitColorGlyphs);
    // Apple Color Emoji uses AAT substitutions for flags and ZWJ sequences.
    let aat = [*b"ankr", *b"feat", *b"kerx", *b"morx", *b"trak"];
    let mut tables: Vec<_> = tags
        .into_iter()
        .chain(color.then_some(aat).into_iter().flatten())
        .filter_map(|tag| {
            // SAFETY: Each tag is a standard OpenType tag supported by Core Text.
            let data =
                unsafe { font.table(u32::from_be_bytes(tag), CTFontTableOptions::NoOptions) }?;
            Some((tag, data.to_vec()))
        })
        .collect();
    tables.sort_unstable_by_key(|(tag, _)| *tag);
    for required in [*b"cmap", *b"head", *b"hhea", *b"hmtx", *b"maxp"] {
        if !tables.iter().any(|(tag, _)| *tag == required) {
            return None;
        }
    }
    let count = u16::try_from(tables.len()).ok()?;
    let exponent = count.ilog2() as u16;
    let search_range = (1_u16 << exponent) * 16;
    let mut file = Vec::new();
    file.extend_from_slice(b"OTTO");
    file.extend_from_slice(&count.to_be_bytes());
    file.extend_from_slice(&search_range.to_be_bytes());
    file.extend_from_slice(&exponent.to_be_bytes());
    file.extend_from_slice(&(count * 16 - search_range).to_be_bytes());
    let directory = file.len();
    file.resize(directory + usize::from(count) * 16, 0);
    let mut head_adjustment = None;
    for (index, (tag, mut table)) in tables.into_iter().enumerate() {
        let offset = u32::try_from(file.len()).ok()?;
        let size = u32::try_from(table.len()).ok()?;
        if &tag == b"head" {
            table.get_mut(8..12)?.fill(0);
            head_adjustment = Some(file.len() + 8);
        }
        let entry = directory + index * 16;
        file[entry..entry + 4].copy_from_slice(&tag);
        file[entry + 4..entry + 8].copy_from_slice(&checksum(&table).to_be_bytes());
        file[entry + 8..entry + 12].copy_from_slice(&offset.to_be_bytes());
        file[entry + 12..entry + 16].copy_from_slice(&size.to_be_bytes());
        file.extend_from_slice(&table);
        file.resize(file.len().next_multiple_of(4), 0);
    }
    let adjustment = head_adjustment?;
    let value = 0xb1b0_afba_u32.wrapping_sub(checksum(&file));
    file[adjustment..adjustment + 4].copy_from_slice(&value.to_be_bytes());
    Some(file)
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0_u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

fn glyph_path(font: &CTFont, id: u16) -> Option<Path> {
    // SAFETY: The null transform is the identity transform.
    let native = unsafe { font.path_for_glyph(id, std::ptr::null()) }?;
    let mut path = Path::new();
    // SAFETY: Core Graphics visits path elements synchronously and gives each
    // callback the points for the reported element type.
    unsafe {
        CGPath::apply(
            Some(&native),
            std::ptr::from_mut(&mut path).cast(),
            Some(append_segment),
        );
    }
    (!path.is_empty()).then_some(path)
}

unsafe extern "C-unwind" fn append_segment(info: *mut c_void, segment: NonNull<CGPathElement>) {
    // SAFETY: glyph_path provides the valid path and CGPathApply provides the
    // valid element and its point array for this call.
    let (path, segment) = unsafe { (&mut *info.cast::<Path>(), segment.as_ref()) };
    let point = |index| {
        let point = unsafe { *segment.points.as_ptr().add(index) };
        (point.x as f32, point.y as f32)
    };
    match segment.r#type {
        CGPathElementType::MoveToPoint => {
            let (x, y) = point(0);
            path.move_to(x, y);
        }
        CGPathElementType::AddLineToPoint => {
            let (x, y) = point(0);
            path.line_to(x, y);
        }
        CGPathElementType::AddQuadCurveToPoint => {
            let (cx, cy) = point(0);
            let (x, y) = point(1);
            path.quad_to(cx, cy, x, y);
        }
        CGPathElementType::AddCurveToPoint => {
            let (c1x, c1y) = point(0);
            let (c2x, c2y) = point(1);
            let (x, y) = point(2);
            path.bezier_to(c1x, c1y, c2x, c2y, x, y);
        }
        CGPathElementType::CloseSubpath => path.close(),
        _ => {}
    }
}
