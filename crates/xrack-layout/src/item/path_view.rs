use crate::{Point, Rect};
use xrack_assets::{parse_path, vector_segments};
use xrack_runtime::VectorSegment;

const FLATNESS: f32 = 0.25;
const MAX_SUBDIVISIONS: u8 = 10;

pub(crate) struct MeasuredPath {
    points: Vec<(f32, Point)>,
    length: f32,
}

impl MeasuredPath {
    pub fn new(commands: &str, bounds: Rect, mirrored: bool) -> Option<Self> {
        let path = parse_path(commands).ok()??;
        Self::from_segments(vector_segments(&path), bounds, mirrored)
    }

    pub fn from_segments(
        segments: impl IntoIterator<Item = VectorSegment>,
        bounds: Rect,
        mirrored: bool,
    ) -> Option<Self> {
        let map = |[x, y]: [f32; 2]| Point {
            x: bounds.x + if mirrored { 1.0 - x } else { x } * bounds.width,
            y: bounds.y + y * bounds.height,
        };
        let mut measured = Self {
            points: Vec::new(),
            length: 0.0,
        };
        let mut current = Point::default();
        let mut start = None;
        for segment in segments {
            match segment {
                VectorSegment::MoveTo(p) if start.is_none() => {
                    current = map(p);
                    start = Some(current);
                    measured.points.push((0.0, current));
                    continue;
                }
                // A single continuous path has an unambiguous drawing and traversal order.
                VectorSegment::MoveTo(_) => return None,
                VectorSegment::LineTo(p) => measured.push(map(p)),
                VectorSegment::QuadTo(c, p) => {
                    let (c, p) = (map(c), map(p));
                    measured.cubic(
                        current,
                        mix(current, c, 2.0 / 3.0),
                        mix(p, c, 2.0 / 3.0),
                        p,
                        0,
                    );
                }
                VectorSegment::CubicTo(a, b, p) => {
                    measured.cubic(current, map(a), map(b), map(p), 0);
                }
                VectorSegment::Close => measured.push(start?),
            }
            current = measured.points.last()?.1;
        }
        (measured.length.is_finite() && measured.length > 0.0).then_some(measured)
    }

    pub fn length(&self) -> f32 {
        self.length
    }

    pub fn closed(&self) -> bool {
        self.points.first().map(|&(_, point)| point) == self.points.last().map(|&(_, point)| point)
    }

    fn push(&mut self, point: Point) {
        if let Some(&(_, last)) = self.points.last() {
            let length = distance(last, point);
            let end = self.length + length;
            if end <= self.length {
                return;
            }
            self.length = end;
        }
        self.points.push((self.length, point));
    }

    fn cubic(&mut self, p0: Point, p1: Point, p2: Point, p3: Point, depth: u8) {
        let polygon = distance(p0, p1) + distance(p1, p2) + distance(p2, p3);
        let chord = distance(p0, p3);
        let deviation = |p: Point| {
            if chord == 0.0 {
                distance(p0, p)
            } else {
                ((p3.x - p0.x) * (p.y - p0.y) - (p3.y - p0.y) * (p.x - p0.x)).abs() / chord
            }
        };
        if depth == MAX_SUBDIVISIONS
            || (polygon - chord <= FLATNESS && deviation(p1).max(deviation(p2)) <= FLATNESS)
        {
            self.push(p3);
            return;
        }
        let a = mix(p0, p1, 0.5);
        let b = mix(p1, p2, 0.5);
        let c = mix(p2, p3, 0.5);
        let d = mix(a, b, 0.5);
        let e = mix(b, c, 0.5);
        let middle = mix(d, e, 0.5);
        self.cubic(p0, a, d, middle, depth + 1);
        self.cubic(middle, e, c, p3, depth + 1);
    }

    pub fn point_at(&self, progress: f32) -> Point {
        let at = self.length * progress.clamp(0.0, 1.0);
        let index = self
            .points
            .partition_point(|&(length, _)| length < at)
            .clamp(1, self.points.len() - 1);
        let (start, p) = self.points[index - 1];
        let (end, q) = self.points[index];
        mix(p, q, (at - start) / (end - start))
    }
}

fn distance(a: Point, b: Point) -> f32 {
    (b.x - a.x).hypot(b.y - a.y)
}

fn mix(a: Point, b: Point, t: f32) -> Point {
    Point {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_distance_and_mirrors_in_the_same_view_box() {
        let bounds = Rect::new(10.0, 20.0, 100.0, 100.0);
        let path = MeasuredPath::new("M 0 0 L 0.2 0 L 0.2 0.8", bounds, false).unwrap();
        assert_eq!(path.length(), 100.0);
        assert_eq!(path.point_at(0.5), Point { x: 30.0, y: 50.0 });
        let mirror = MeasuredPath::new("M 0 0 L 0.2 0 L 0.2 0.8", bounds, true).unwrap();
        assert_eq!(mirror.point_at(0.5), Point { x: 90.0, y: 50.0 });

        let curve = MeasuredPath::new("M 0 0 Q 0.5 1 1 0", bounds, false).unwrap();
        let midpoint = curve.point_at(0.5);
        assert!((midpoint.x - 60.0).abs() < 0.1);
        assert!((midpoint.y - 70.0).abs() < 0.1);
        assert!(MeasuredPath::new("M 0 0 L 1 0 M 0 1 L 1 1", bounds, false).is_none());
    }
}
