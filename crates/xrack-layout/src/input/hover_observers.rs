use crate::{Interaction, LayoutNode, Point};
use xrack_runtime::{Component, CursorShape, HoverHandle, Length, Node, PointerHandle, RowKey};

pub(crate) struct HoverEntry {
    pub path: Vec<usize>,
    pub row_key: Option<RowKey>,
    pub handle: HoverHandle,
    pub position: (Length, Length),
    pub cursor_shape: CursorShape,
}

pub struct HoverDelivery {
    path: Vec<usize>,
    row_key: Option<RowKey>,
    handle: HoverHandle,
    event: &'static str,
}

impl HoverDelivery {
    fn new(entry: &HoverEntry, event: &'static str) -> Self {
        Self {
            path: entry.path.clone(),
            row_key: entry.row_key,
            handle: entry.handle.clone(),
            event,
        }
    }

    pub fn apply(self, component: &mut impl Component, scene: &Node) -> bool {
        scene.row_key_at(&self.path) == self.row_key
            && scene
                .at_path(&self.path)
                .is_some_and(|node| node.hover_handle.as_ref() == Some(&self.handle))
            && component.dispatch_at(scene, &self.path, self.event)
    }
}

/// Resolve hover in a prepared frame and publish all state before delivering events.
#[derive(Default)]
pub struct HoverObservers {
    entries: Vec<HoverEntry>,
    touch_area: Option<PointerHandle>,
    touch_area_cursor: CursorShape,
}

impl HoverObservers {
    pub fn cursor_shape(&self) -> CursorShape {
        std::iter::once(self.touch_area_cursor)
            .chain(self.entries.iter().map(|entry| entry.cursor_shape))
            .find(|shape| *shape != CursorShape::Auto)
            .unwrap_or_default()
    }

    pub fn sync(
        &mut self,
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        pointer: Option<Point>,
    ) -> Vec<HoverDelivery> {
        let touch_area = pointer.and_then(|point| input.touch_area_hover(scene, layout, point));
        self.touch_area_cursor =
            touch_area.map_or(CursorShape::Auto, |(node, _)| node.cursor_shape());
        let touch_area =
            touch_area.and_then(|(node, position)| Some((node.pointer_handle.clone()?, position)));
        let previous = self.touch_area.take();
        if let Some(previous) = previous
            && touch_area
                .as_ref()
                .is_none_or(|(handle, _)| handle != &previous)
        {
            previous.publish_hover(None);
        }
        self.touch_area = touch_area.map(|(handle, position)| {
            handle.publish_hover(Some(position));
            handle
        });
        let next = pointer.map_or_else(Vec::new, |point| input.hover_entries(scene, layout, point));
        let previous = std::mem::replace(&mut self.entries, next);
        let mut events = Vec::new();
        for old in &previous {
            if !self.entries.iter().any(|entry| entry.handle == old.handle) {
                old.handle.publish(None);
                events.push(HoverDelivery::new(old, "exited"));
            }
        }
        for entry in &self.entries {
            let old = previous.iter().find(|old| old.handle == entry.handle);
            entry.handle.publish(Some(entry.position));
            if old.is_none() {
                events.push(HoverDelivery::new(entry, "entered"));
            } else if old.is_some_and(|old| old.position != entry.position) {
                events.push(HoverDelivery::new(entry, "moved"));
            }
        }
        events
    }

    pub fn clear(&mut self) {
        self.touch_area_cursor = CursorShape::Auto;
        if let Some(handle) = self.touch_area.take() {
            handle.publish_hover(None);
        }
        for entry in self.entries.drain(..) {
            entry.handle.publish(None);
        }
    }
}
