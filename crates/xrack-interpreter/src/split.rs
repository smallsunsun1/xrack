use crate::*;
use xrack_runtime::SplitHandleInput;

pub(crate) struct Definition {
    pub(crate) fill: Option<usize>,
    pub(crate) panes: Box<[String]>,
    pub(crate) handle: Option<Box<Element>>,
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

impl Instance {
    pub(crate) fn populate_split(
        &self,
        element: &Element,
        definition: &Definition,
        node: &mut Node,
    ) -> Result<()> {
        let state = self.split_state(&element.key);
        state.populate(node, definition.fill);
        if let Some(template) = &definition.handle {
            state
                .handle()
                .try_decorate(node, |input| self.render_split_handle(template, input))?;
        }
        Ok(())
    }

    pub(crate) fn split_resizing(&self, key: &str) -> Result<bool> {
        let (owner, element) = self.element_owner(key)?;
        Ok(owner.split_state(&element.key).handle().resizing())
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
        let names: Vec<_> = definition.panes.iter().map(String::as_str).collect();
        Ok(self
            .split_state(&element.key)
            .save(&names, definition.fill.is_some()))
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
        let names: Vec<_> = definition.panes.iter().map(String::as_str).collect();
        let Some(changed) =
            self.split_state(&element.key)
                .restore(&names, definition.fill.is_some(), serialized)
        else {
            return Ok(false);
        };
        if changed {
            self.handler(element, "resized", &[])?;
        }
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
        if !owner.split_state(&element.key).resize(node, input) {
            return Ok(false);
        }
        owner.handler(element, "resized", &[])?;
        Ok(true)
    }
}
