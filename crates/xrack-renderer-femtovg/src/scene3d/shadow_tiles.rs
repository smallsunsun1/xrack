//! Square pages shared by differently sized shadow views.
pub(super) const MIN_SHADOW_SIZE: u32 = 128;
const MAX_CELLS: usize = 4096 / MIN_SHADOW_SIZE as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ShadowTile {
    pub layer: u32,
    pub origin: [u32; 2],
    pub size: u32,
}

#[derive(Clone, Copy)]
pub(super) struct ShadowPage {
    pub size: u32,
    cell: u32,
}

impl ShadowPage {
    pub fn new(sizes: &[u32]) -> Self {
        let size = sizes.iter().copied().max().unwrap_or(MIN_SHADOW_SIZE);
        if sizes.iter().all(|&requested| requested == size) {
            Self { size, cell: size }
        } else {
            Self {
                size: size.next_power_of_two(),
                cell: MIN_SHADOW_SIZE,
            }
        }
    }

    pub fn pack(
        self,
        sizes: &[u32],
        previous: &[Option<ShadowTile>],
        capacity: usize,
    ) -> Option<Vec<Option<ShadowTile>>> {
        let mut occupied = vec![[0_u32; MAX_CELLS]; capacity];
        let mut assigned = vec![None; sizes.len()];
        for (index, old) in previous.iter().copied().enumerate() {
            if let Some(tile) = old
                && tile.size == sizes[index]
                && self.claim(&mut occupied, tile)
            {
                assigned[index] = Some(tile);
            }
        }
        let mut remaining: Vec<_> = sizes
            .iter()
            .copied()
            .enumerate()
            .filter(|&(i, size)| size != 0 && assigned[i].is_none())
            .collect();
        remaining.sort_by_key(|&(_, size)| std::cmp::Reverse(size));
        for (index, size) in remaining {
            let step = self.cells(size) * self.cell;
            let mut candidates = self.candidates(capacity, size, step);
            assigned[index] = Some(candidates.find(|&tile| self.claim(&mut occupied, tile))?);
        }
        Some(assigned)
    }

    fn candidates(self, layers: usize, size: u32, step: u32) -> impl Iterator<Item = ShadowTile> {
        (0..layers as u32).flat_map(move |layer| {
            (0..self.size).step_by(step as usize).flat_map(move |y| {
                (0..self.size)
                    .step_by(step as usize)
                    .map(move |x| ShadowTile {
                        layer,
                        origin: [x, y],
                        size,
                    })
            })
        })
    }

    fn cells(self, size: u32) -> u32 {
        size.div_ceil(self.cell).next_power_of_two()
    }

    fn claim(self, occupied: &mut [[u32; MAX_CELLS]], tile: ShadowTile) -> bool {
        let Some(page) = occupied.get_mut(tile.layer as usize) else {
            return false;
        };
        let [x, y] = tile.origin.map(|coordinate| coordinate / self.cell);
        let count = self.cells(tile.size);
        let side = self.size / self.cell;
        if x + count > side
            || y + count > side
            || !tile
                .origin
                .iter()
                .all(|value| value.is_multiple_of(count * self.cell))
        {
            return false;
        }
        let mask = (u32::MAX >> (32 - count)) << x;
        let rows = &mut page[y as usize..(y + count) as usize];
        if rows.iter().any(|row| row & mask != 0) {
            return false;
        }
        for row in rows {
            *row |= mask;
        }
        true
    }
}

pub(super) fn used_layers(tiles: &[Option<ShadowTile>]) -> u32 {
    tiles
        .iter()
        .flatten()
        .map(|tile| tile.layer + 1)
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_small_faces_together_and_retains_their_offsets() {
        let sizes = [512, 128, 128, 256, 256, 128, 128, 128, 128];
        let page = ShadowPage::new(&sizes);
        let tiles = page.pack(&sizes, &[], sizes.len()).unwrap();
        assert_eq!(used_layers(&tiles), 2);
        let mut resized = sizes;
        resized[3] = 128;
        let updated = page.pack(&resized, &tiles, 2).unwrap();
        for (i, (&before, &after)) in tiles.iter().zip(&updated).enumerate() {
            if i != 3 {
                assert_eq!(after, before);
            }
        }
        assert_eq!(updated[3].unwrap().size, 128);
    }
}
