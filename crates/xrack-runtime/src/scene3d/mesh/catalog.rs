use std::collections::{BTreeMap, HashSet};

use super::data::Mesh3D;

pub(super) fn material_slots(meshes: &[Mesh3D]) -> Box<[String]> {
    // Imported rigs share the same slot table among their meshes. Visit each
    // table once, and keep source identity independent of render batches.
    let mut visited = HashSet::new();
    let mut material_names = BTreeMap::<Option<u32>, Option<&str>>::new();
    for mesh in meshes {
        let slots = mesh.material_slots();
        if slots.is_empty() {
            for index in 0..mesh.materials().len() as u32 {
                material_names.entry(Some(index)).or_default();
            }
        } else if visited.insert((slots.as_ptr(), slots.len())) {
            for slot in slots {
                material_names
                    .entry(slot.source_index)
                    .or_insert(slot.name.as_deref());
            }
        }
    }
    let mut counts = BTreeMap::new();
    for name in material_names.values().flatten() {
        *counts.entry(*name).or_insert(0usize) += 1;
    }
    material_names
        .into_iter()
        .map(|(index, name)| match index {
            None => "#default".into(),
            Some(index) => name
                .filter(|name| !name.is_empty() && counts.get(name) == Some(&1))
                .map_or_else(
                    || format!("#{index}"),
                    |name| {
                        if name.starts_with('#') || name.starts_with("name:") {
                            format!("name:{name}")
                        } else {
                            name.to_owned()
                        }
                    },
                ),
        })
        .collect()
}

pub(super) fn indexed_selectors<'a>(
    entries: impl Iterator<Item = (usize, Option<&'a str>)> + Clone,
    reserved: impl Fn(&str) -> bool,
) -> Box<[String]> {
    let mut counts = BTreeMap::new();
    for (_, name) in entries.clone() {
        if let Some(name) = name {
            *counts.entry(name).or_insert(0usize) += 1;
        }
    }
    entries
        .map(|(index, name)| {
            name.filter(|name| !name.is_empty() && !reserved(name) && counts.get(name) == Some(&1))
                .map_or_else(|| format!("#{index}"), str::to_owned)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::indexed_selectors;

    #[test]
    fn ambiguous_and_reserved_names_keep_their_sparse_source_indices() {
        let names = [
            (0, Some("Idle")),
            (2, Some("Idle")),
            (4, Some("#7")),
            (7, None),
            (9, Some("Run")),
        ];
        let selectors = indexed_selectors(names.into_iter(), |name| name.starts_with('#'));
        assert_eq!(&*selectors, ["#0", "#2", "#4", "#7", "Run"]);
    }

    #[test]
    fn editing_a_returned_model_preserves_the_cached_catalog() {
        use crate::scene3d::{Mesh3D, MeshAsset};

        let asset =
            MeshAsset::Static(Mesh3D::Box).with_material_variants(vec![("Blue".into(), None)]);
        let edited = asset.material_variant_selectors();
        edited.push("temporary edit".into());
        assert_eq!(edited.len(), 2);
        assert_eq!(asset.material_variant_selectors().snapshot(), ["Blue"]);
    }
}
