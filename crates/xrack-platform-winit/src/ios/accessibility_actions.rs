use crate::{UserEvent, desktop::WindowEvents};
use accesskit::{Action, ActionData, ActionRequest, CustomAction, NodeId, TreeId, TreeUpdate};
use block2::RcBlock;
use objc2::{
    ClassType, msg_send_id,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObject, NSObjectProtocol},
    sel,
};
use objc2_foundation::{MainThreadMarker, NSArray, NSString};
use objc2_ui_kit::{
    NSObjectUIAccessibilityAction, NSObjectUIAccessibilityContainer, UIAccessibilityCustomAction,
};
use std::{cell::Cell, collections::BTreeMap, ptr::NonNull, rc::Rc};
use winit::window::WindowId;

const NODE_PREFIX: &str = "xrack-action-";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NativeAction {
    ContextMenu,
    Expand,
    Collapse,
    Custom(CustomAction),
}

impl NativeAction {
    fn name(&self) -> &str {
        match self {
            Self::ContextMenu => "Show context menu",
            Self::Expand => "Expand",
            Self::Collapse => "Collapse",
            Self::Custom(action) => &action.description,
        }
    }

    fn request(&self, node: NodeId) -> ActionRequest {
        let (action, data) = match self {
            Self::ContextMenu => (Action::ShowContextMenu, None),
            Self::Expand => (Action::Expand, None),
            Self::Collapse => (Action::Collapse, None),
            Self::Custom(action) => (
                Action::CustomAction,
                Some(ActionData::CustomAction(action.id)),
            ),
        };
        ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: node,
            data,
        }
    }
}

struct AttachedActions {
    element: Retained<NSObject>,
    actions: Vec<NativeAction>,
    active: Rc<Cell<bool>>,
}

impl Drop for AttachedActions {
    fn drop(&mut self) {
        self.active.set(false);
        // SAFETY: This object and its actions are installed and released on the UIKit thread.
        unsafe { self.element.setAccessibilityCustomActions(None) };
    }
}

pub(crate) struct NativeActions {
    window_id: WindowId,
    events: WindowEvents,
    attached: BTreeMap<NodeId, AttachedActions>,
}

impl NativeActions {
    pub(crate) fn new(window_id: WindowId, events: WindowEvents) -> Self {
        Self {
            window_id,
            events,
            attached: BTreeMap::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.attached.clear();
    }

    pub(crate) fn project(update: &mut TreeUpdate) -> BTreeMap<NodeId, Vec<NativeAction>> {
        let mut projected = BTreeMap::new();
        for (id, node) in &mut update.nodes {
            if node.is_disabled() {
                continue;
            }
            let mut actions = Vec::new();
            if node.supports_action(Action::ShowContextMenu) {
                actions.push(NativeAction::ContextMenu);
            }
            if matches!(
                node.role(),
                accesskit::Role::TreeItem | accesskit::Role::DisclosureTriangle
            ) {
                if node.supports_action(Action::Expand) {
                    actions.push(NativeAction::Expand);
                }
                if node.supports_action(Action::Collapse) {
                    actions.push(NativeAction::Collapse);
                }
            }
            if node.supports_action(Action::CustomAction) {
                actions.extend(
                    node.custom_actions()
                        .iter()
                        .cloned()
                        .map(NativeAction::Custom),
                );
            }
            if !actions.is_empty() {
                // The iOS adapter publishes this as the UIKit accessibility identifier.
                node.set_author_id(format!("{NODE_PREFIX}{}", id.0));
                projected.insert(*id, actions);
            }
        }
        projected
    }

    pub(crate) fn update(
        &mut self,
        host: &NSObject,
        mut pending: BTreeMap<NodeId, Vec<NativeAction>>,
    ) {
        self.attached.retain(|id, attached| {
            if pending.get(id) == Some(&attached.actions) {
                pending.remove(id);
                true
            } else {
                false
            }
        });
        if let Some(mtm) = MainThreadMarker::new() {
            self.attach_in(host, mtm, &mut pending);
        }
    }

    fn attach_in(
        &mut self,
        container: &NSObject,
        mtm: MainThreadMarker,
        pending: &mut BTreeMap<NodeId, Vec<NativeAction>>,
    ) {
        if pending.is_empty() {
            return;
        }
        // SAFETY: AccessKit's exported UIKit objects support the accessibility container API.
        let Some(children) = (unsafe { container.accessibilityElements(mtm) }) else {
            return;
        };
        for element in &children {
            let ancestors =
                std::iter::successors(Some(element.class()), |class| class.superclass());
            if !ancestors
                .into_iter()
                .any(|class| std::ptr::eq(class, NSObject::class()))
            {
                continue;
            }
            // SAFETY: The runtime class check verifies this cast for the borrowed object.
            let element = unsafe { &*std::ptr::from_ref::<AnyObject>(element).cast::<NSObject>() };
            if let Some(id) = node_id(element)
                && let Some(actions) = pending.remove(&id)
                // SAFETY: The array keeps the element alive while it is retained.
                && let Some(retained) = unsafe { Retained::retain(std::ptr::from_ref(element).cast_mut()) }
            {
                self.attach(retained, id, actions);
            }
            self.attach_in(element, mtm, pending);
        }
    }

    fn attach(&mut self, element: Retained<NSObject>, id: NodeId, actions: Vec<NativeAction>) {
        let active = Rc::new(Cell::new(true));
        let window_id = self.window_id;
        let native = actions
            .iter()
            .map(|action| {
                let request = action.request(id);
                let active = active.clone();
                let events = self.events.clone();
                let callback = RcBlock::new(move |_: NonNull<UIAccessibilityCustomAction>| {
                    Bool::new(
                        active.get()
                            && events.post(UserEvent::Accessibility(accesskit_winit::Event {
                                window_id,
                                window_event: accesskit_winit::WindowEvent::ActionRequested(
                                    request.clone(),
                                ),
                            })),
                    )
                });
                // SAFETY: UIKit copies the block when creating the action.
                unsafe {
                    UIAccessibilityCustomAction::initWithName_actionHandler(
                        UIAccessibilityCustomAction::alloc(),
                        &NSString::from_str(action.name()),
                        std::ptr::from_ref(&*callback).cast_mut(),
                    )
                }
            })
            .collect();
        // SAFETY: AccessKit's exported UIKit objects store custom actions on the main thread.
        unsafe { element.setAccessibilityCustomActions(Some(&NSArray::from_vec(native))) };
        self.attached.insert(
            id,
            AttachedActions {
                element,
                actions,
                active,
            },
        );
    }
}

fn node_id(element: &NSObject) -> Option<NodeId> {
    if !element.respondsToSelector(sel!(accessibilityIdentifier)) {
        return None;
    }
    // SAFETY: The accessibility identifier is a nullable NSString.
    let identifier: Option<Retained<NSString>> =
        unsafe { msg_send_id![element, accessibilityIdentifier] };
    identifier?
        .to_string()
        .strip_prefix(NODE_PREFIX)?
        .parse()
        .ok()
        .map(NodeId)
}
