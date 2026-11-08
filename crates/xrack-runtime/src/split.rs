use crate::{Node, Value, is_modal_overlay};
use std::sync::Arc;

impl Node {
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
                    && !is_modal_overlay(pane.kind)
                    && !matches!(pane.kind, "Menu" | "Tooltip")
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
        if self.kind != "SplitView"
            || weights.len() < 2
            || self
                .split_panes()
                .map(|(index, _)| index)
                .ne(weights.iter().map(|(index, _)| *index))
            || weights
                .iter()
                .any(|(_, value)| !value.is_finite() || *value < 0.0)
        {
            return None;
        }
        let sum: f64 = weights.iter().map(|(_, value)| f64::from(*value)).sum();
        if sum <= 0.0 {
            return None;
        }
        let mut next = self
            .split_weights
            .as_deref()
            .map_or_else(Vec::new, <[_]>::to_vec);
        next.resize(self.split_pane_count(), None);
        for &(index, weight) in weights {
            next[index] = Some(weight);
        }
        (self.split_weights.as_deref() != Some(next.as_slice())).then(|| Arc::from(next))
    }
}
