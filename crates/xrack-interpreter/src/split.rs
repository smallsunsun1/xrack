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
            let error = std::cell::Cell::new(None);
            state.handle.decorate(node, |input| {
                self.render_split_handle(template, input)
                    .unwrap_or_else(|failure| {
                        error.set(Some(failure));
                        Node::hidden()
                    })
            });
            if let Some(error) = error.into_inner() {
                return Err(error);
            }
        }
        Ok(())
    }

    pub(crate) fn split_resizing(&self, key: &str) -> Result<bool> {
        let (owner, element) = self.element_owner(key)?;
        Ok(owner.split_state(&element.key).handle.resizing())
    }

    pub(crate) fn save_split(&self, key: &str) -> Result<String> {
        let (owner, element) = self.element_owner(key)?;
        let definition = element
            .split
            .as_ref()
            .ok_or_else(|| Error::Invalid("invalid SplitView target".into()))?;
        let names = element.pane_names();
        let names: Vec<_> = names.iter().map(|name| name.as_ref()).collect();
        let values = owner.split_state(&element.key).values.get(|| None);
        Ok(if definition.fill.is_some() {
            encode_split_sizes(&names, values.as_deref())
        } else {
            encode_split_state(&names, values.as_deref())
        })
    }

    pub(crate) fn restore_split(&self, key: &str, serialized: &str) -> Result<()> {
        let (owner, element) = self.element_owner(key)?;
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
        if let Some(values) = decoded {
            owner.update_split(
                element,
                values.iter().any(Option::is_some).then_some(values),
            )?;
        }
        Ok(())
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
        values: &[(usize, f32)],
        sized: bool,
    ) -> Result<bool> {
        let Some(node) = scene.interactive_at_path(path) else {
            return Ok(false);
        };
        let Some((owner, element)) = self.try_target(scene, path)? else {
            return Ok(false);
        };
        let Some(definition) = &element.split else {
            return Ok(false);
        };
        if sized != definition.fill.is_some() {
            return Ok(false);
        }
        let snapshot = if sized {
            &node.split_sizes
        } else {
            &node.split_weights
        };
        if owner
            .split_state(&element.key)
            .values
            .get(|| None)
            .as_deref()
            != snapshot.as_deref()
        {
            return Ok(false);
        }
        let next = if sized {
            node.resized_split_sizes(values)
        } else {
            node.resized_split_weights(values)
        };
        match next {
            Some(next) => owner.update_split(element, Some(next)),
            None => Ok(false),
        }
    }
}
