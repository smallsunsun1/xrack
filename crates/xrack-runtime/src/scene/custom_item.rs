use crate::{Color, EventResult, FocusEvent, KeyEvent, KeyPhase, Node, PointerEvent, WheelEvent};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};

/// Logical-pixel geometry shared by native controls and the host.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CustomItemRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl CustomItemRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, [x, y]: [f32; 2]) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

/// The persistent identity of one mounted CustomItem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CustomItemId(pub(crate) usize);

#[derive(Clone, Copy)]
pub struct CustomItemData<'a> {
    pub id: Option<CustomItemId>,
    pub data: &'a str,
    pub revision: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CustomItemPlacement {
    pub bounds: CustomItemRect,
    pub window_bounds: CustomItemRect,
    /// Conservative visible rectangle in the item's local coordinates.
    pub visible_bounds: CustomItemRect,
    pub visible: bool,
    pub enabled: bool,
}

#[derive(Clone, Copy)]
pub struct CustomItemContext<'a> {
    pub state: CustomItemData<'a>,
    pub placement: CustomItemPlacement,
    handle: Option<&'a crate::GeometryHandle>,
}

impl<'a> CustomItemContext<'a> {
    #[doc(hidden)]
    pub fn for_node(node: &'a Node, placement: CustomItemPlacement) -> Self {
        Self {
            state: node.custom_item_data(),
            placement,
            handle: node.geometry_handle.as_ref(),
        }
    }

    /// A weak handle for scheduling layout or paint from the owning UI thread.
    pub fn updater(self) -> Option<crate::CustomItemUpdater> {
        self.handle.map(crate::GeometryHandle::custom_item_updater)
    }

    pub fn request_update(self, update: CustomItemUpdate) {
        if let Some(handle) = self.handle {
            handle.request_custom_item_update(update);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomItemUpdate {
    /// Layout, baseline and painting can depend on the data change.
    Layout,
    /// Reuse layout. Hit testing and painting read the current data.
    Paint,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CustomItemChange {
    Mounted,
    Placement { previous: CustomItemPlacement },
    Unmounted,
}

pub enum CustomItemEvent<'a> {
    Pointer(&'a PointerEvent),
    Key(KeyPhase, &'a KeyEvent),
    Wheel(&'a WheelEvent),
    Focus(FocusEvent),
    Activate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CustomItemRole {
    #[default]
    Button,
    Image,
    Group,
}

/// Drawing commands in the same local coordinates passed to contains().
pub enum CustomItemDrawing {
    Fill {
        bounds: CustomItemRect,
        color: Color,
    },
    Polyline {
        points: Vec<[f32; 2]>,
        width: f32,
        color: Color,
    },
    Polygon {
        points: Vec<[f32; 2]>,
        color: Color,
    },
    Image {
        bounds: CustomItemRect,
        image: crate::ImageAsset,
    },
    Text {
        origin: [f32; 2],
        text: String,
        size: f32,
        color: Color,
    },
}

/// A native control definition. Callbacks run on the UI thread. Data and
/// revision are supplied by the XRack component and can be bound reactively.
pub trait CustomItem {
    fn measure(&self, _data: CustomItemData<'_>, _available: [f32; 2]) -> [f32; 2] {
        [0.0; 2]
    }

    fn baseline(&self, _data: CustomItemData<'_>, _size: [f32; 2]) -> f32 {
        0.0
    }

    fn arrange(&self, _context: CustomItemContext<'_>, _children: &mut [CustomItemRect]) {}

    fn contains(&self, context: CustomItemContext<'_>, point: [f32; 2]) -> bool {
        context.placement.bounds.contains(point)
    }

    fn paint(&self, context: CustomItemContext<'_>, drawings: &mut Vec<CustomItemDrawing>);

    fn event(&self, _context: CustomItemContext<'_>, _event: CustomItemEvent<'_>) -> EventResult {
        EventResult::Reject
    }

    fn changed(&self, _context: CustomItemContext<'_>, _change: CustomItemChange) {}

    fn update_effects(
        &self,
        _old: CustomItemData<'_>,
        _new: CustomItemData<'_>,
    ) -> CustomItemUpdate {
        CustomItemUpdate::Layout
    }

    fn role(&self, _data: CustomItemData<'_>) -> CustomItemRole {
        CustomItemRole::Button
    }

    /// Capture a drag before an ancestor scroll container takes over.
    fn reserves_pointer_drag(&self, _data: CustomItemData<'_>) -> bool {
        false
    }

    fn focusable(&self, data: CustomItemData<'_>) -> bool {
        self.role(data) == CustomItemRole::Button
    }
}

thread_local! {
    static ITEMS: RefCell<BTreeMap<String, Weak<dyn CustomItem>>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// Keeps a type registered on the current UI thread.
#[must_use]
pub struct CustomItemRegistration {
    name: String,
    definition: Rc<dyn CustomItem>,
}

impl Drop for CustomItemRegistration {
    fn drop(&mut self) {
        let _ = ITEMS.try_with(|items| {
            let mut items = items.borrow_mut();
            if items
                .get(&self.name)
                .is_some_and(|entry| entry.ptr_eq(&Rc::downgrade(&self.definition)))
            {
                items.remove(&self.name);
            }
        });
    }
}

pub fn register_custom_item(
    name: impl Into<String>,
    definition: impl CustomItem + 'static,
) -> Result<CustomItemRegistration, String> {
    let name = name.into();
    if name.trim().is_empty() {
        return Err("a custom item needs a name".into());
    }
    ITEMS.with(|items| {
        let mut items = items.borrow_mut();
        if items.get(&name).and_then(Weak::upgrade).is_some() {
            return Err(format!("custom item {name:?} is already registered"));
        }
        let definition: Rc<dyn CustomItem> = Rc::new(definition);
        items.insert(name.clone(), Rc::downgrade(&definition));
        Ok(CustomItemRegistration { name, definition })
    })
}

impl Node {
    #[doc(hidden)]
    pub fn custom_item(&self) -> Option<Rc<dyn CustomItem>> {
        if self.kind != "CustomItem" {
            return None;
        }
        let name = self.text("implementation")?;
        ITEMS.with(|items| items.borrow().get(name).and_then(Weak::upgrade))
    }

    #[doc(hidden)]
    pub fn custom_item_data(&self) -> CustomItemData<'_> {
        CustomItemData {
            id: self
                .geometry_handle
                .as_ref()
                .map(|handle| CustomItemId(handle.identity())),
            data: self.text("data").unwrap_or_default(),
            revision: self.int("revision").unwrap_or_default(),
        }
    }

    pub(crate) fn custom_item_event(&self, event: CustomItemEvent<'_>) -> EventResult {
        let Some(item) = self.custom_item() else {
            return EventResult::Reject;
        };
        let placement = self
            .geometry_handle
            .as_ref()
            .map_or_else(CustomItemPlacement::default, |handle| {
                handle.custom_item_placement()
            });
        item.event(CustomItemContext::for_node(self, placement), event)
    }
}
