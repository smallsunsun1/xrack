use std::{collections::BTreeSet, sync::Arc};

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

/// Immutable geometry of a path loaded from a verified XR module.
#[derive(Clone, Debug)]
pub struct LoadedVectorPath(Arc<LoadedPathData>);

#[derive(Debug)]
struct LoadedPathData {
    commands: String,
    bounds: [f32; 4],
    segments: Vec<VectorSegment>,
}

impl LoadedVectorPath {
    pub fn new(commands: String, bounds: [f32; 4], segments: Vec<VectorSegment>) -> Self {
        Self(Arc::new(LoadedPathData {
            commands,
            bounds,
            segments,
        }))
    }

    pub(crate) fn geometry(&self, commands: &str) -> Option<([f32; 4], &[VectorSegment])> {
        (self.0.commands == commands).then_some((self.0.bounds, &self.0.segments))
    }
}

impl PartialEq for LoadedVectorPath {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// A scene plan backed by generated constants, loaded IR, or runtime layout.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub enum ScenePlanSlice<T: 'static> {
    Static(&'static [T]),
    Shared(Arc<[T]>),
    Owned(Vec<T>),
}

impl<T> std::ops::Deref for ScenePlanSlice<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        match self {
            Self::Static(values) => values,
            Self::Shared(values) => values,
            Self::Owned(values) => values,
        }
    }
}

impl<T: PartialEq> PartialEq for ScenePlanSlice<T> {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (Self::Shared(a), Self::Shared(b)) if Arc::ptr_eq(a, b))
            || **self == **other
    }
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
