use xrack_layout::{Rect, Size, tile_extent};
use xrack_runtime::{ImageTileMode, NineSlice};

pub(crate) struct Patch {
    pub(crate) destination: Rect,
    pub(crate) source: [u32; 4],
    pub(crate) tile: Size,
    pub(crate) repeat: [bool; 2],
}

struct Segment {
    start: f32,
    extent: f32,
    source_start: u32,
    source_extent: u32,
    tile: f32,
    repeat: bool,
}

fn segments(
    source: u32,
    raster: u32,
    edges: [u32; 2],
    start: f32,
    extent: f32,
    mode: ImageTileMode,
) -> [Segment; 3] {
    let [before, after] = edges;
    let cuts = [0, before, source - after, source];
    let raster_cuts = cuts.map(|cut| {
        ((u64::from(cut) * u64::from(raster) + u64::from(source / 2)) / u64::from(source)) as u32
    });
    let edge_sum = (before + after) as f32;
    let end = start + extent;
    let positions = if edge_sum > extent {
        let middle = start + extent * (before as f32 / edge_sum);
        [start, middle, middle, end]
    } else {
        [start, start + before as f32, end - after as f32, end]
    };
    std::array::from_fn(|index| {
        let extent = positions[index + 1] - positions[index];
        let natural = (cuts[index + 1] - cuts[index]) as f32;
        let mode = if index == 1 {
            mode
        } else {
            ImageTileMode::Stretch
        };
        let tile = tile_extent(extent, natural, mode);
        Segment {
            start: positions[index],
            extent,
            source_start: raster_cuts[index],
            source_extent: raster_cuts[index + 1] - raster_cuts[index],
            tile,
            repeat: mode != ImageTileMode::Stretch,
        }
    })
}

pub(crate) fn patches(
    bounds: Rect,
    size: [u32; 2],
    raster: [u32; 2],
    edges: NineSlice,
    modes: [ImageTileMode; 2],
) -> impl Iterator<Item = Patch> {
    let columns = segments(
        size[0],
        raster[0],
        [edges.left, edges.right],
        bounds.x,
        bounds.width,
        modes[0],
    );
    let rows = segments(
        size[1],
        raster[1],
        [edges.top, edges.bottom],
        bounds.y,
        bounds.height,
        modes[1],
    );
    rows.into_iter().flat_map(move |row| {
        let patches: [Option<Patch>; 3] = std::array::from_fn(|index| {
            let column = &columns[index];
            (column.extent > 0.0
                && row.extent > 0.0
                && column.source_extent > 0
                && row.source_extent > 0)
                .then(|| Patch {
                    destination: Rect::new(column.start, row.start, column.extent, row.extent),
                    source: [
                        column.source_start,
                        row.source_start,
                        column.source_extent,
                        row.source_extent,
                    ],
                    tile: Size::new(column.tile, row.tile),
                    repeat: [column.repeat, row.repeat],
                })
        });
        patches.into_iter().flatten()
    })
}
