use crate::{Point, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowResizeEdge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl WindowResizeEdge {
    pub(crate) fn at(bounds: Rect, point: Point, border: f32) -> Option<Self> {
        if !border.is_finite() || border <= 0.0 || !bounds.contains(point) {
            return None;
        }
        let horizontal = border.min(bounds.width * 0.5);
        let vertical = border.min(bounds.height * 0.5);
        let left = point.x - bounds.x < horizontal;
        let right = bounds.x + bounds.width - point.x <= horizontal;
        let top = point.y - bounds.y < vertical;
        let bottom = bounds.y + bounds.height - point.y <= vertical;
        Some(match (left, right, top, bottom) {
            (true, _, true, _) => Self::NorthWest,
            (_, true, true, _) => Self::NorthEast,
            (true, _, _, true) => Self::SouthWest,
            (_, true, _, true) => Self::SouthEast,
            (_, _, true, _) => Self::North,
            (_, _, _, true) => Self::South,
            (true, _, _, _) => Self::West,
            (_, true, _, _) => Self::East,
            _ => return None,
        })
    }
}
