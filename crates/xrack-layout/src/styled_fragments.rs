use crate::StyledFragment;
use std::{
    fmt,
    ops::{Deref, DerefMut},
    slice,
    sync::{Arc, OnceLock},
};

/// Shaped text fragments in visual order.
///
/// A slice can be requested through dereferencing. Retained paragraphs can
/// also be traversed directly with their placement offsets.
#[derive(Clone)]
pub struct StyledFragments(Storage);

#[derive(Clone)]
enum Storage {
    Flat(Vec<StyledFragment>),
    Paragraphs(Arc<Paragraphs>),
}

struct Paragraphs {
    blocks: Vec<StyledFragmentBlock>,
    len: OnceLock<usize>,
    flat: OnceLock<Vec<StyledFragment>>,
}

/// A reusable group of paragraph-local fragments placed at a document offset.
#[doc(hidden)]
pub struct StyledFragmentBlock {
    content: BlockContent,
    y: f32,
    /// The union of ink and selection bounds, relative to the paragraph.
    ink: Option<[f32; 2]>,
}

enum BlockContent {
    Ready(Arc<Vec<StyledFragment>>, usize),
    Deferred(Arc<dyn StyledFragmentSource>, f32),
}

/// Paragraph geometry created when a consumer reaches the paragraph.
#[doc(hidden)]
pub trait StyledFragmentSource: Send + Sync {
    fn fragments(&self) -> &[StyledFragment];
}

impl StyledFragmentBlock {
    pub fn new(
        fragments: Arc<Vec<StyledFragment>>,
        len: usize,
        y: f32,
        ink: Option<[f32; 2]>,
    ) -> Self {
        let len = len.min(fragments.len());
        Self {
            content: BlockContent::Ready(fragments, len),
            y,
            ink,
        }
    }

    pub fn deferred(
        source: Arc<dyn StyledFragmentSource>,
        y: f32,
        height: f32,
        ink: Option<[f32; 2]>,
    ) -> Self {
        Self {
            content: BlockContent::Deferred(source, height),
            y,
            ink,
        }
    }

    fn fragments(&self) -> &[StyledFragment] {
        match &self.content {
            BlockContent::Ready(fragments, len) => &fragments[..*len],
            BlockContent::Deferred(source, height) => {
                let fragments = source.fragments();
                let end =
                    fragments.partition_point(|fragment| self.y + fragment.bounds.y < *height);
                &fragments[..end]
            }
        }
    }

    fn intersect_y(&self, clip: Option<(f32, f32)>) -> bool {
        self.ink
            .zip(clip)
            .is_none_or(|([top, bottom], (clip_top, clip_bottom))| {
                self.y + bottom >= clip_top && self.y + top <= clip_bottom
            })
    }
}

impl StyledFragments {
    #[doc(hidden)]
    pub fn from_blocks(blocks: Vec<StyledFragmentBlock>) -> Self {
        Self(Storage::Paragraphs(Arc::new(Paragraphs {
            blocks,
            len: OnceLock::new(),
            flat: OnceLock::new(),
        })))
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            Storage::Flat(fragments) => fragments.len(),
            Storage::Paragraphs(paragraphs) => *paragraphs.len.get_or_init(|| {
                paragraphs
                    .blocks
                    .iter()
                    .map(|block| block.fragments().len())
                    .sum()
            }),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.placed().next().is_none()
    }

    pub fn capacity(&self) -> usize {
        match &self.0 {
            Storage::Flat(fragments) => fragments.capacity(),
            Storage::Paragraphs(_) => self.len(),
        }
    }

    /// Reports whether the first fragment carries positioned glyph geometry.
    #[doc(hidden)]
    pub fn has_shaped_runs(&self) -> bool {
        match &self.0 {
            Storage::Flat(fragments) => fragments
                .first()
                .is_some_and(|fragment| fragment.shaped_run().is_some()),
            Storage::Paragraphs(paragraphs) => {
                match paragraphs.blocks.first().map(|block| &block.content) {
                    Some(BlockContent::Deferred(..)) => true,
                    Some(BlockContent::Ready(..)) => self
                        .placed()
                        .next()
                        .is_some_and(|(fragment, _)| fragment.shaped_run().is_some()),
                    None => false,
                }
            }
        }
    }

    /// Iterates paragraph-local fragments and their document Y offsets.
    pub fn placed(&self) -> impl DoubleEndedIterator<Item = (&StyledFragment, f32)> {
        self.placed_in_y_range(None)
    }

    /// Restricts traversal to paragraphs with ink or selection inside the range.
    #[doc(hidden)]
    pub fn placed_in_y_range(
        &self,
        clip: Option<(f32, f32)>,
    ) -> impl DoubleEndedIterator<Item = (&StyledFragment, f32)> {
        let (flat, blocks) = match &self.0 {
            Storage::Flat(fragments) => (fragments.as_slice(), &[][..]),
            Storage::Paragraphs(paragraphs) => (&[][..], paragraphs.blocks.as_slice()),
        };
        flat.iter().map(|fragment| (fragment, 0.0)).chain(
            blocks
                .iter()
                .filter(move |block| block.intersect_y(clip))
                .flat_map(|block| block.fragments().iter().map(|fragment| (fragment, block.y))),
        )
    }

    pub fn as_slice(&self) -> &[StyledFragment] {
        self.as_vec().as_slice()
    }

    fn as_vec(&self) -> &Vec<StyledFragment> {
        match &self.0 {
            Storage::Flat(fragments) => fragments,
            Storage::Paragraphs(paragraphs) => paragraphs.flat.get_or_init(|| {
                let mut flat = Vec::with_capacity(self.len());
                flat.extend(self.placed().map(|(fragment, y)| {
                    let mut fragment = fragment.clone();
                    fragment.bounds.y += y;
                    fragment
                }));
                flat
            }),
        }
    }

    pub(crate) fn into_vec(self) -> Vec<StyledFragment> {
        match self.0 {
            Storage::Flat(fragments) => fragments,
            Storage::Paragraphs(_) => self.as_slice().to_vec(),
        }
    }
}

impl From<Vec<StyledFragment>> for StyledFragments {
    fn from(fragments: Vec<StyledFragment>) -> Self {
        Self(Storage::Flat(fragments))
    }
}

impl Default for StyledFragments {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl Deref for StyledFragments {
    type Target = Vec<StyledFragment>;

    fn deref(&self) -> &Self::Target {
        self.as_vec()
    }
}

impl DerefMut for StyledFragments {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if matches!(self.0, Storage::Paragraphs(_)) {
            self.0 = Storage::Flat(self.as_slice().to_vec());
        }
        match &mut self.0 {
            Storage::Flat(fragments) => fragments,
            Storage::Paragraphs(_) => unreachable!(),
        }
    }
}

impl fmt::Debug for StyledFragments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(formatter)
    }
}

impl PartialEq for StyledFragments {
    fn eq(&self, other: &Self) -> bool {
        if let (Storage::Flat(left), Storage::Flat(right)) = (&self.0, &other.0) {
            return left == right;
        }
        if let (Storage::Paragraphs(left), Storage::Paragraphs(right)) = (&self.0, &other.0)
            && Arc::ptr_eq(left, right)
        {
            return true;
        }
        let mut left = self.placed();
        let mut right = other.placed();
        loop {
            match (left.next(), right.next()) {
                (None, None) => return true,
                (Some((left, y)), Some((right, other_y))) if left.eq_placed(y, right, other_y) => {}
                _ => return false,
            }
        }
    }
}

impl<'a> IntoIterator for &'a StyledFragments {
    type Item = &'a StyledFragment;
    type IntoIter = slice::Iter<'a, StyledFragment>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a> IntoIterator for &'a mut StyledFragments {
    type Item = &'a mut StyledFragment;
    type IntoIter = slice::IterMut<'a, StyledFragment>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl IntoIterator for StyledFragments {
    type Item = StyledFragment;
    type IntoIter = std::vec::IntoIter<StyledFragment>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Size, StyledTextLayout};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use xrack_runtime::{Node, StyledText};

    #[test]
    fn paragraph_placement_and_slice_edits_keep_earlier_snapshots_independent() {
        let mut node = Node::new("StyledText", None);
        let document = StyledText::from_plain_text("first");
        node.properties.insert("text", document.clone().into());
        let plan =
            StyledTextLayout::new(&node, &document, Size::new(80.0, 40.0), 12.0, |_, _| 24.0);
        let fragments = Arc::new(plan.fragments.into_vec());
        let height = fragments[0].bounds.height;
        let blocks = StyledFragments::from_blocks(vec![
            StyledFragmentBlock::new(fragments.clone(), 1, 12.0, Some([0.0, height])),
            StyledFragmentBlock::new(fragments.clone(), 1, 84.0, Some([0.0, height])),
        ]);
        let offsets: Vec<_> = blocks
            .placed_in_y_range(Some((80.0, 130.0)))
            .map(|(_, y)| y)
            .collect();
        assert_eq!(offsets, [84.0]);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].bounds.y, 12.0);
        let mut edited = blocks.clone();
        edited[0].bounds.y = 48.0;
        assert_eq!(edited[0].bounds.y, 48.0);
        assert_eq!(blocks[0].bounds.y, 12.0);
        assert_eq!(fragments[0].bounds.y, 0.0);
    }

    #[test]
    fn visible_traversal_only_loads_intersecting_paragraphs() {
        struct CountedSource {
            loads: Arc<AtomicUsize>,
            fragments: OnceLock<Vec<StyledFragment>>,
        }
        impl StyledFragmentSource for CountedSource {
            fn fragments(&self) -> &[StyledFragment] {
                self.fragments.get_or_init(|| {
                    self.loads.fetch_add(1, Ordering::Relaxed);
                    Vec::new()
                })
            }
        }
        let loads = Arc::new(AtomicUsize::new(0));
        let blocks = [0.0, 80.0, 160.0]
            .into_iter()
            .map(|y| {
                let source = Arc::new(CountedSource {
                    loads: loads.clone(),
                    fragments: OnceLock::new(),
                });
                StyledFragmentBlock::deferred(source, y, f32::INFINITY, Some([0.0, 20.0]))
            })
            .collect();
        let fragments = StyledFragments::from_blocks(blocks);
        assert!(fragments.has_shaped_runs());
        assert_eq!(loads.load(Ordering::Relaxed), 0);
        for _ in 0..2 {
            assert_eq!(fragments.placed_in_y_range(Some((81.0, 99.0))).count(), 0);
        }
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        assert_eq!(fragments.len(), 0);
        assert_eq!(loads.load(Ordering::Relaxed), 3);
    }
}
