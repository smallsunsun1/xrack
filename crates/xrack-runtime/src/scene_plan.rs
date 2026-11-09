use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VectorSegment {
    MoveTo([f32; 2]),
    LineTo([f32; 2]),
    QuadTo([f32; 2], [f32; 2]),
    CubicTo([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

#[derive(Debug, PartialEq)]
pub struct StaticVectorPath {
    pub commands: &'static str,
    pub bounds: [f32; 4],
    pub segments: &'static [VectorSegment],
}

const MAX_TRACKS: usize = 128;

#[derive(Clone, Copy, Debug, Default)]
pub struct GridTrackInput {
    pub row: Option<i64>,
    pub col: Option<i64>,
    pub rowspan: Option<i64>,
    pub colspan: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridCellPosition {
    pub row: usize,
    pub col: usize,
    pub rows: usize,
    pub cols: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridCellSlot {
    pub index: usize,
    pub child_index: Option<usize>,
    pub position: GridCellPosition,
}

/// Places visible cells using the same rules at compile time and at runtime.
#[derive(Default)]
pub struct GridPlanner {
    row: usize,
    col: usize,
    occupied: BTreeSet<(usize, usize)>,
}

impl GridPlanner {
    pub fn begin_row(&mut self) {
        if self.col != 0 {
            self.end_row();
        }
    }

    pub fn end_row(&mut self) {
        self.row += 1;
        self.col = 0;
    }

    pub fn place(&mut self, input: GridTrackInput) -> Option<GridCellPosition> {
        let track = |value: Option<i64>| {
            value
                .and_then(|value| usize::try_from(value).ok())
                .filter(|&index| index < MAX_TRACKS)
        };
        if let Some(next) = track(input.row) {
            if next != self.row {
                self.col = 0;
            }
            self.row = next;
        }
        if let Some(next) = track(input.col) {
            self.col = next;
        } else {
            while self.col < MAX_TRACKS && self.occupied.contains(&(self.row, self.col)) {
                self.col += 1;
            }
        }
        if self.row >= MAX_TRACKS || self.col >= MAX_TRACKS {
            return None;
        }
        let rows = track(input.rowspan)
            .unwrap_or(1)
            .max(1)
            .min(MAX_TRACKS - self.row);
        let cols = track(input.colspan)
            .unwrap_or(1)
            .max(1)
            .min(MAX_TRACKS - self.col);
        for row in self.row..self.row + rows {
            for col in self.col..self.col + cols {
                self.occupied.insert((row, col));
            }
        }
        let result = GridCellPosition {
            row: self.row,
            col: self.col,
            rows,
            cols,
        };
        self.col += cols;
        Some(result)
    }
}
