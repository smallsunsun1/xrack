use femtovg::{Canvas, FillRule, Paint, Path, renderer::WGPURenderer};
use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_graphics::{CGPath, CGPathElement, CGPathElementType};
use objc2_core_text::{CTFont, CTFontTableOptions};
use std::{cell::RefCell, collections::HashMap, ffi::c_void, ptr::NonNull, sync::Arc};
use xrack_layout::Point;

pub(crate) struct NativeFont {
    font: CFRetained<CTFont>,
    pub(crate) data: Arc<[u8]>,
    outlines: RefCell<HashMap<u16, Option<Path>>>,
}

impl NativeFont {
    pub(crate) fn ping_fang() -> Option<Self> {
        let name = CFString::from_str("PingFangSC-Regular");
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
}

/// Supply the same native face's OpenType layout tables to the shaper. Core
/// Text supplies the outlines separately, including Apple's private formats.
fn layout_tables(font: &CTFont) -> Option<Vec<u8>> {
    let tags = [
        *b"GDEF", *b"GPOS", *b"GSUB", *b"OS/2", *b"cmap", *b"head", *b"hhea", *b"hmtx", *b"kern",
        *b"maxp", *b"name", *b"post",
    ];
    let tables: Vec<_> = tags
        .into_iter()
        .filter_map(|tag| {
            // SAFETY: Each tag is a standard OpenType tag supported by Core Text.
            let data =
                unsafe { font.table(u32::from_be_bytes(tag), CTFontTableOptions::NoOptions) }?;
            Some((tag, data.to_vec()))
        })
        .collect();
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
