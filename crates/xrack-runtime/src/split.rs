use crate::{Node, Orientation, Property, Value, is_window_overlay};
use std::{cell::RefCell, collections::BTreeMap, fmt, fmt::Write, rc::Rc, sync::Arc};

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitHandleInput {
    pub index: i64,
    pub orientation: Orientation,
    pub hovered: bool,
    pub pressed: bool,
}

#[derive(Default)]
struct SplitHandleVisual {
    input: Property<SplitHandleInput>,
    scene: Property<Node>,
}

#[derive(Default)]
struct SplitSignals {
    pressed: Property<Option<(usize, usize)>>,
    hovered: Property<Option<usize>>,
    visuals: RefCell<BTreeMap<usize, SplitHandleVisual>>,
}

/// The reactive drag state of a SplitView instance.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct SplitResizeHandle(Rc<SplitSignals>);

impl fmt::Debug for SplitResizeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SplitResizeHandle").finish_non_exhaustive()
    }
}

impl PartialEq for SplitResizeHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl SplitResizeHandle {
    pub fn resizing(&self) -> bool {
        self.0.pressed.get(|| None).is_some()
    }

    pub fn publish_pressed(&self, divider: Option<(usize, usize)>) {
        self.0.pressed.set_if_changed(divider);
    }

    pub fn publish_hovered(&self, slot: Option<usize>) {
        self.0.hovered.set_if_changed(slot);
    }

    pub fn decorate(&self, view: &mut Node, render: impl Fn(SplitHandleInput) -> Node) {
        if !view.is_visible() {
            return;
        }
        let first = view.split_pane_count();
        let orientation = view.orientation();
        let panes: Vec<_> = view.split_panes().map(|(index, _)| index).collect();
        let hovered = self.0.hovered.get(|| None);
        let pressed = self.0.pressed.get(|| None);
        let mut visuals = self.0.visuals.borrow_mut();
        for pair in panes.windows(2) {
            let index = pair[0];
            let slot = first + index;
            let visual = visuals.entry(index).or_default();
            let input = SplitHandleInput {
                index: index as i64,
                orientation,
                hovered: hovered == Some(slot),
                pressed: pressed == Some((slot, pair[1])),
            };
            visual.input.set_if_changed(input);
            let decoration = visual.scene.get(|| render(visual.input.get(|| input)));
            view.children[slot].children.push(decoration);
        }
    }
}

impl Node {
    pub fn has_split_fill(&self) -> bool {
        self.split_fill_index.is_some() || self.split_fill_pane.is_some()
    }

    #[doc(hidden)]
    pub fn split_fill_slot(&self) -> Option<usize> {
        self.split_fill_index.or_else(|| {
            let target = self.split_fill_pane?;
            self.children[..self.split_pane_count()]
                .iter()
                .position(|pane| pane.element_id() == Some(target))
        })
    }

    pub fn split_pane_count(&self) -> usize {
        self.children
            .iter()
            .position(|node| node.kind == "SplitHandle")
            .unwrap_or(self.children.len())
    }

    pub fn split_panes(&self) -> impl Iterator<Item = (usize, &Node)> {
        self.children[..self.split_pane_count()]
            .iter()
            .enumerate()
            .filter(|(_, pane)| {
                pane.is_visible()
                    && !is_window_overlay(pane.kind)
                    && !matches!(pane.kind, "Menu" | "Tooltip" | "Shortcut")
            })
    }

    #[doc(hidden)]
    pub fn add_split_handles(&mut self) {
        let orientation = self.orientation();
        let color = self.color("handle-color");
        let count = self.children.len().saturating_sub(1);
        self.children.extend((0..count).map(|_| {
            let mut handle = Node::new("SplitHandle", None);
            handle
                .properties
                .insert("orientation", Value::Orientation(orientation));
            if let Some(color) = color {
                handle.properties.insert("color", Value::Color(color));
            }
            handle
        }));
    }

    #[doc(hidden)]
    pub fn resized_split_weights(&self, weights: &[(usize, f32)]) -> Option<Arc<[Option<f32>]>> {
        if self.has_split_fill()
            || self
                .split_panes()
                .map(|(index, _)| index)
                .ne(weights.iter().map(|(index, _)| *index))
        {
            return None;
        }
        self.resized_split_values(weights, self.split_weights.as_deref())
    }

    #[doc(hidden)]
    pub fn resized_split_sizes(&self, sizes: &[(usize, f32)]) -> Option<Arc<[Option<f32>]>> {
        if !self.has_split_fill() {
            return None;
        }
        if self
            .split_panes()
            .map(|(index, _)| index)
            .ne(sizes.iter().map(|(index, _)| *index))
        {
            let [before, after] = sizes else {
                return None;
            };
            if self
                .split_panes()
                .map(|(index, _)| index)
                .skip_while(|&index| index != before.0)
                .nth(1)
                != Some(after.0)
            {
                return None;
            }
        }
        self.resized_split_values(sizes, self.split_sizes.as_deref())
    }

    fn resized_split_values(
        &self,
        values: &[(usize, f32)],
        previous: Option<&[Option<f32>]>,
    ) -> Option<Arc<[Option<f32>]>> {
        if self.kind != "SplitView"
            || values.len() < 2
            || values
                .iter()
                .any(|(_, value)| !value.is_finite() || *value < 0.0)
        {
            return None;
        }
        let sum: f64 = values.iter().map(|(_, value)| f64::from(*value)).sum();
        if sum <= 0.0 {
            return None;
        }
        let mut next = previous.map_or_else(Vec::new, <[_]>::to_vec);
        next.resize(self.split_pane_count(), None);
        for &(index, value) in values {
            next[index] = Some(value);
        }
        (previous != Some(next.as_slice())).then(|| Arc::from(next))
    }
}

const SPLIT_STATE_VERSION: &str = "xrack-split/1";
const SPLIT_SIZES_VERSION: &str = "xrack-split/2";

#[doc(hidden)]
pub fn encode_split_state(panes: &[&str], weights: Option<&[Option<f32>]>) -> String {
    encode_split_values(SPLIT_STATE_VERSION, panes, weights)
}

#[doc(hidden)]
pub fn encode_split_sizes(panes: &[&str], sizes: Option<&[Option<f32>]>) -> String {
    encode_split_values(SPLIT_SIZES_VERSION, panes, sizes)
}

fn encode_split_values(version: &str, panes: &[&str], weights: Option<&[Option<f32>]>) -> String {
    let mut state = version.to_owned();
    for (index, pane) in panes.iter().enumerate() {
        let value = weights
            .and_then(|weights| weights.get(index))
            .copied()
            .flatten();
        if let Some(value) = value {
            write!(state, ";{pane}={value}").expect("writing to a String");
        } else {
            write!(state, ";{pane}=-").expect("writing to a String");
        }
    }
    state
}

#[doc(hidden)]
pub fn decode_split_state(panes: &[&str], state: &str) -> Option<Arc<[Option<f32>]>> {
    decode_split_values(SPLIT_STATE_VERSION, panes, state)
}

#[doc(hidden)]
pub fn decode_split_sizes(panes: &[&str], state: &str) -> Option<Arc<[Option<f32>]>> {
    decode_split_values(SPLIT_SIZES_VERSION, panes, state)
}

fn decode_split_values(version: &str, panes: &[&str], state: &str) -> Option<Arc<[Option<f32>]>> {
    let mut entries = state.split(';');
    if entries.next()? != version {
        return None;
    }
    let mut values = BTreeMap::new();
    for entry in entries {
        let (pane, encoded) = entry.split_once('=')?;
        if values.len() == panes.len() || !panes.contains(&pane) {
            return None;
        }
        let weight = if encoded == "-" {
            None
        } else {
            let value = encoded.parse::<f32>().ok()?;
            if !value.is_finite() || value < 0.0 {
                return None;
            }
            Some(value)
        };
        if values.insert(pane, weight).is_some() {
            return None;
        }
    }
    let weights = panes
        .iter()
        .map(|pane| values.get(pane).copied())
        .collect::<Option<Vec<_>>>()?;
    if weights.iter().any(Option::is_some) && !weights.iter().flatten().any(|weight| *weight > 0.0)
    {
        return None;
    }
    Some(weights.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn divider_templates_only_recompute_for_changed_inputs() {
        let state = SplitResizeHandle::default();
        let calls = Cell::new([0; 3]);
        let render = || {
            let mut view = Node::new("SplitView", None);
            view.children = (0..4).map(|_| Node::new("Rectangle", None)).collect();
            view.add_split_handles();
            state.decorate(&mut view, |input| {
                let mut next = calls.get();
                next[input.index as usize] += 1;
                calls.set(next);
                let mut node = Node::new("Rectangle", None);
                node.properties.insert("visible", (!input.pressed).into());
                node
            });
            view
        };
        render();
        assert_eq!(calls.get(), [1, 1, 1]);
        state.publish_hovered(Some(4));
        render();
        assert_eq!(calls.get(), [2, 1, 1]);
        state.publish_hovered(Some(5));
        render();
        assert_eq!(calls.get(), [3, 2, 1]);
        state.publish_pressed(Some((5, 2)));
        let view = render();
        assert_eq!(calls.get(), [3, 3, 1]);
        assert!(view.children[4].children[0].is_visible());
        assert!(!view.children[5].children[0].is_visible());
    }

    #[test]
    fn state_restores_by_pane_id_and_rejects_an_invalid_record_set() {
        let panes = ["source", "preview", "@2"];
        let weights = [Some(0.2), None, Some(0.8)];
        let saved = encode_split_state(&panes, Some(&weights));
        assert_eq!(
            decode_split_state(&["preview", "source", "@2"], &saved)
                .unwrap()
                .as_ref(),
            [None, Some(0.2), Some(0.8)],
        );
        assert_eq!(
            decode_split_state(&panes, &encode_split_state(&panes, None))
                .unwrap()
                .as_ref(),
            [None; 3]
        );
        for invalid in [
            "xrack-split/2;source=0.2;preview=-;@2=0.8",
            "xrack-split/1;source=0.2;source=-;@2=0.8",
            "xrack-split/1;source=0.2;@2=0.8",
            "xrack-split/1;source=NaN;preview=-;@2=0.8",
            "xrack-split/1;source=-0.1;preview=-;@2=0.8",
            "xrack-split/1;source=0;preview=-;@2=0",
        ] {
            assert!(decode_split_state(&panes, invalid).is_none(), "{invalid}");
        }
    }
}
