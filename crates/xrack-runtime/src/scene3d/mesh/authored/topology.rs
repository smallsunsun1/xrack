//! Validate authored draw ranges and expand strips, loops, and fans.

use crate::scene3d::MeshTopology;

use super::{MAX_INDICES, SceneMeshPart, SceneMeshSubset};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Points,
    Lines,
    LineStrip,
    LineLoop,
    Triangles,
    TriangleStrip,
    TriangleFan,
}

impl Mode {
    pub(super) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "points" => Self::Points,
            "lines" => Self::Lines,
            "line-strip" => Self::LineStrip,
            "line-loop" => Self::LineLoop,
            "" | "triangles" => Self::Triangles,
            "triangle-strip" => Self::TriangleStrip,
            "triangle-fan" => Self::TriangleFan,
            _ => return None,
        })
    }

    pub(super) fn topology(self) -> MeshTopology {
        match self {
            Self::Points => MeshTopology::Points,
            Self::Triangles | Self::TriangleStrip | Self::TriangleFan => MeshTopology::Triangles,
            Self::Lines | Self::LineStrip | Self::LineLoop => MeshTopology::Lines,
        }
    }

    fn is_list(self) -> bool {
        matches!(self, Self::Points | Self::Lines | Self::Triangles)
    }

    fn expanded_len(self, source_len: usize) -> Option<usize> {
        match self {
            Self::LineStrip => (source_len >= 2).then(|| (source_len - 1) * 2),
            Self::LineLoop => (source_len >= 2).then_some(source_len * 2),
            Self::TriangleStrip | Self::TriangleFan => {
                (source_len >= 3).then(|| (source_len - 2) * 3)
            }
            list => (source_len != 0
                && source_len.is_multiple_of(list.topology().vertices_per_primitive()))
            .then_some(source_len),
        }
    }

    fn append(self, source: &[u32], expanded: &mut Vec<u32>) {
        match self {
            Self::LineStrip | Self::LineLoop => {
                expanded.extend(source.windows(2).flatten().copied());
                if self == Self::LineLoop {
                    expanded.extend([source[source.len() - 1], source[0]]);
                }
            }
            Self::TriangleStrip => {
                for (index, face) in source.windows(3).enumerate() {
                    let offset = index % 2;
                    expanded.extend([face[offset], face[1 - offset], face[2]]);
                }
            }
            Self::TriangleFan => {
                for edge in source[1..].windows(2) {
                    expanded.extend([source[0], edge[0], edge[1]]);
                }
            }
            _ => expanded.extend_from_slice(source),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Expansion {
    pub(super) mode: Mode,
    // Only strip, loop and fan draws restart at subset boundaries.
    runs: Vec<usize>,
    source_count: usize,
    vertex_count: usize,
}

impl Expansion {
    pub(super) fn prepare(
        mode: Mode,
        subsets: Vec<SceneMeshSubset>,
        limit: usize,
    ) -> Option<(Self, Vec<SceneMeshPart>)> {
        if limit > MAX_INDICES {
            return None;
        }
        let mut expansion = Self {
            mode,
            runs: Vec::new(),
            source_count: limit,
            vertex_count: 0,
        };
        if subsets.is_empty() {
            expansion.vertex_count = mode.expanded_len(limit)?;
            return (expansion.vertex_count <= MAX_INDICES).then_some((expansion, Vec::new()));
        }

        let mut subsets: Vec<_> = subsets
            .into_iter()
            .map(|subset| subset.prepare(limit))
            .collect::<Option<_>>()?;
        subsets.sort_unstable_by_key(|part| part.range.start);
        let mut layout: Vec<SceneMeshPart> = Vec::with_capacity(subsets.len());
        let mut append = |name: String, count| {
            let start = expansion.vertex_count;
            expansion.vertex_count += mode.expanded_len(count)?;
            if expansion.vertex_count > MAX_INDICES {
                return None;
            }
            if !mode.is_list() {
                expansion.runs.push(count);
            }
            if let Some(previous) = layout.last_mut()
                && previous.name == name
            {
                previous.range.end = expansion.vertex_count;
            } else {
                layout.push(SceneMeshPart {
                    name,
                    range: start..expansion.vertex_count,
                });
            }
            Some(())
        };
        let mut end = 0;
        for part in subsets {
            if part.range.start < end {
                return None;
            }
            if part.range.start > end {
                append("surface".into(), part.range.start - end)?;
            }
            append(part.name, part.range.len())?;
            end = part.range.end;
        }
        if end < limit {
            append("surface".into(), limit - end)?;
        }
        Some((expansion, layout))
    }

    pub(super) fn indices(&self, source: Vec<u32>) -> Option<Vec<u32>> {
        if source.len() != self.source_count {
            return None;
        }
        if self.mode.is_list() {
            return Some(source);
        }
        let mut expanded = Vec::with_capacity(self.vertex_count);
        if self.runs.is_empty() {
            self.mode.append(&source, &mut expanded);
        } else {
            let mut start = 0;
            for &count in &self.runs {
                let end = start + count;
                self.mode.append(&source[start..end], &mut expanded);
                start = end;
            }
        }
        Some(expanded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_preserves_winding_and_restarts_each_strip() {
        let input = vec![0, 1, 2, 3, 4];
        for (mode, expected) in [
            (Mode::LineStrip, &[0, 1, 1, 2, 2, 3, 3, 4][..]),
            (Mode::LineLoop, &[0, 1, 1, 2, 2, 3, 3, 4, 4, 0][..]),
            (Mode::TriangleStrip, &[0, 1, 2, 2, 1, 3, 2, 3, 4][..]),
            (Mode::TriangleFan, &[0, 1, 2, 0, 2, 3, 0, 3, 4][..]),
        ] {
            let (expansion, _) = Expansion::prepare(mode, vec![], input.len()).unwrap();
            assert_eq!(expansion.indices(input.clone()).unwrap(), expected);
        }
        let (expanded, layout) = Expansion::prepare(
            Mode::LineLoop,
            vec![
                SceneMeshSubset {
                    name: "contour".into(),
                    offset: 0,
                    count: 3,
                },
                SceneMeshSubset {
                    name: "contour".into(),
                    offset: 3,
                    count: 3,
                },
            ],
            6,
        )
        .unwrap();
        assert_eq!(layout.len(), 1);
        assert_eq!(layout[0].range, 0..12);
        assert_eq!(
            expanded.indices(vec![0, 1, 2, 3, 4, 5]).unwrap(),
            [0, 1, 1, 2, 2, 0, 3, 4, 4, 5, 5, 3],
        );
    }

    #[test]
    fn invalid_modes_partial_primitives_and_expansion_limits_are_rejected() {
        assert!(Mode::named("quads").is_none());
        assert!(Expansion::prepare(Mode::Lines, vec![], 3).is_none());
        assert!(Expansion::prepare(Mode::LineLoop, vec![], 1).is_none());
        assert!(Expansion::prepare(Mode::TriangleStrip, vec![], 2).is_none());
        assert!(Expansion::prepare(Mode::LineLoop, vec![], MAX_INDICES / 2 + 1).is_none());
        assert!(Expansion::prepare(Mode::TriangleFan, vec![], MAX_INDICES / 3 + 3).is_none());
        assert!(Expansion::prepare(Mode::Points, vec![], 1).is_some());
    }
}
