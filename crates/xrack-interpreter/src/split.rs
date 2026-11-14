use crate::*;
use std::sync::Arc;
use xrack_runtime::{
    Property, SplitHandleInput, SplitResizeHandle, decode_split_sizes, decode_split_state,
    encode_split_sizes, encode_split_state,
};

pub(crate) struct Definition {
    pub(crate) fill: Option<usize>,
    pub(crate) handle: Option<Box<Element>>,
}

type Panes = Arc<[Option<f32>]>;

#[derive(Default)]
pub(crate) struct State {
    values: Property<Option<Panes>>,
    handle: SplitResizeHandle,
}

#[derive(Clone, Copy)]
pub(crate) enum Parameter {
    Index,
    Orientation,
    Hovered,
    Pressed,
}

impl Parameter {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "handle-index" => Self::Index,
            "handle-orientation" => Self::Orientation,
            "handle-hovered" => Self::Hovered,
            "handle-pressed" => Self::Pressed,
            _ => return None,
        })
    }

    pub(crate) fn value(self, input: SplitHandleInput) -> Value {
        match self {
            Self::Index => input.index.into(),
            Self::Orientation => input.orientation.into(),
            Self::Hovered => input.hovered.into(),
            Self::Pressed => input.pressed.into(),
        }
    }
}

impl Element {
    fn pane_names(&self) -> Vec<std::borrow::Cow<'_, str>> {
        self.children
            .iter()
            .enumerate()
            .map(|(index, pane)| {
                if pane.id.is_empty() {
                    format!("@{index}").into()
                } else {
                    pane.id.as_ref().into()
                }
            })
            .collect()
    }
}

impl Instance {
    pub(crate) fn populate_split(
        &self,
        element: &Element,
        definition: &Definition,
        node: &mut Node,
    ) -> Result<()> {
        let state = self.split_state(&element.key);
        let values = state.values.get(|| None);
        if let Some(fill) = definition.fill {
            node.split_fill_index = Some(fill);
            node.split_sizes = values;
        } else {
            node.split_weights = values;
        }
        node.split_resize_handle = Some(state.handle.clone());
        node.add_split_handles();
        if let Some(template) = &definition.handle {
            state
                .handle
                .try_decorate(node, |input| self.render_split_handle(template, input))?;
        }
        Ok(())
    }

    pub(crate) fn split_resizing(&self, key: &str) -> Result<bool> {
        let (owner, element) = self.element_owner(key)?;
        Ok(owner.split_state(&element.key).handle.resizing())
    }

    /// Serialize a split view's pane weights or fixed sizes.
    pub fn save_split_state(&self, id: &str) -> Result<String> {
        let element = self.host_element(id, |element| element.split.is_some())?;
        self.save_split_state_for(element)
    }

    pub(crate) fn save_split_state_by_key(&self, key: &str) -> Result<String> {
        let (owner, element) = self.element_owner(key)?;
        owner.save_split_state_for(element)
    }

    fn save_split_state_for(&self, element: &Element) -> Result<String> {
        let definition = element
            .split
            .as_ref()
            .ok_or_else(|| Error::Invalid("invalid SplitView target".into()))?;
        let names = element.pane_names();
        let names: Vec<_> = names.iter().map(|name| name.as_ref()).collect();
        let values = self.split_state(&element.key).values.get(|| None);
        Ok(if definition.fill.is_some() {
            encode_split_sizes(&names, values.as_deref())
        } else {
            encode_split_state(&names, values.as_deref())
        })
    }

    /// Restore pane state. Returns false for an invalid payload and true for a valid no-op.
    pub fn restore_split_state(&self, id: &str, serialized: &str) -> Result<bool> {
        let element = self.host_element(id, |element| element.split.is_some())?;
        self.restore_split_state_for(element, serialized)
    }

    pub(crate) fn restore_split_state_by_key(&self, key: &str, serialized: &str) -> Result<bool> {
        let (owner, element) = self.element_owner(key)?;
        owner.restore_split_state_for(element, serialized)
    }

    fn restore_split_state_for(&self, element: &Element, serialized: &str) -> Result<bool> {
        let definition = element
            .split
            .as_ref()
            .ok_or_else(|| Error::Invalid("invalid SplitView target".into()))?;
        let names = element.pane_names();
        let names: Vec<_> = names.iter().map(|name| name.as_ref()).collect();
        let decoded = if definition.fill.is_some() {
            decode_split_sizes(&names, serialized)
        } else {
            decode_split_state(&names, serialized)
        };
        let Some(values) = decoded else {
            return Ok(false);
        };
        self.update_split(
            element,
            values.iter().any(Option::is_some).then_some(values),
        )?;
        Ok(true)
    }

    fn update_split(&self, element: &Element, next: Option<Panes>) -> Result<bool> {
        let state = self.split_state(&element.key);
        if state.values.get(|| None) == next {
            return Ok(false);
        }
        state.values.set(next);
        self.handler(element, "resized", &[])?;
        Ok(true)
    }

    pub(crate) fn resize_split(
        &self,
        scene: &Node,
        path: &[usize],
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let Some(node) = scene.at_path(path) else {
            return Ok(false);
        };
        let Some((owner, element, input)) = self.input_route_target(scene, path, input)? else {
            return Ok(false);
        };
        if element.split.is_none() {
            return Ok(false);
        }
        let current = owner.split_state(&element.key).values.get(|| None);
        let next = node.prepare_split_resize(input, current.as_deref());
        match next {
            Some(next) => owner.update_split(element, Some(next)),
            None => Ok(false),
        }
    }
}
