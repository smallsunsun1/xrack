use crate::{
    AccessibilityEscape, UserEvent, desktop::WindowEvents, ios_accessibility_actions::NativeActions,
};
use accesskit::{Action, ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_ios::SubclassingAdapter;
use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol},
};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSNotificationCenter, NSOperationQueue, NSString,
};
use objc2_ui_kit::{
    UIAccessibilityIsReduceMotionEnabled, UIAccessibilityPageScrolledNotification,
    UIAccessibilityPostNotification, UIAccessibilityReduceMotionStatusDidChangeNotification,
    UIContentSizeCategoryDidChangeNotification, UIFontMetrics, UIView, UIViewAutoresizing,
};
use std::{cell::Cell, ptr::NonNull};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use xrack_layout::{LayoutNode, Rect, ScrollLayout, StackTransitions, Target};
use xrack_runtime::{Node, Orientation, PageIdentity, group_index, group_item};

pub(super) struct ScrollAnnouncement {
    pub(super) target: Target,
    position: ScrollPosition,
}

enum ScrollPosition {
    Page(PageIdentity),
    Viewport { axis: Orientation, previous: f32 },
}

impl ScrollAnnouncement {
    pub(super) fn page(target: Target, view: &Node, index: usize) -> Option<Self> {
        Some(Self {
            target,
            position: ScrollPosition::Page(PageIdentity::of(group_item(view, index)?, index)),
        })
    }

    pub(super) fn viewport(target: &Target, item: &LayoutNode, action: Action) -> Option<Self> {
        let axis = match action {
            Action::ScrollUp | Action::ScrollDown => Orientation::Vertical,
            Action::ScrollLeft | Action::ScrollRight => Orientation::Horizontal,
            _ => return None,
        };
        Some(Self {
            target: target.clone(),
            position: ScrollPosition::Viewport {
                axis,
                previous: scroll_position(item, axis)?.offset,
            },
        })
    }

    pub(super) fn ready(&self, transitions: &StackTransitions) -> bool {
        !matches!(self.position, ScrollPosition::Page(_))
            || transitions.at(&self.target.path).is_none()
    }

    pub(super) fn announce(self, scene: &Node, layout: &LayoutNode) {
        let Some(status) = self.status(scene, layout) else {
            return;
        };
        let status = NSString::from_str(&status);
        // SAFETY: The completed frame and its accessibility notification run on the UIKit thread.
        unsafe {
            UIAccessibilityPostNotification(UIAccessibilityPageScrolledNotification, Some(&status))
        };
    }

    fn status(&self, scene: &Node, layout: &LayoutNode) -> Option<String> {
        match &self.position {
            ScrollPosition::Page(identity) => {
                let view = self.target.node_in(scene)?;
                let selected = group_index(view)?;
                let page = group_item(view, selected)?;
                if PageIdentity::of(page, selected) != *identity {
                    return None;
                }
                let mut pages =
                    (0..view.children.len()).filter(|&index| group_item(view, index).is_some());
                let position = pages.position(|page| page == selected)? + 1;
                let count = position + pages.count();
                Some(format!("Page {position} of {count}"))
            }
            ScrollPosition::Viewport { axis, previous } => {
                let item = layout.at_path(&self.target.path)?;
                let scroll = scroll_position(item, *axis)?;
                if scroll.offset == *previous || scroll.max_offset <= 0.0 {
                    return None;
                }
                let percent = 100.0 * scroll.offset / scroll.max_offset;
                Some(format!("{percent:.1}% scrolled"))
            }
        }
    }
}

fn scroll_position(item: &LayoutNode, axis: Orientation) -> Option<ScrollLayout> {
    item.attached_scroll_geometry()
        .filter(|bar| bar.orientation == axis)
        .map(|bar| bar.scroll)
        .or_else(|| item.scroll_on(axis))
}

pub(super) struct Accessibility {
    adapter: SubclassingAdapter,
    host: Retained<AccessibilityView>,
    actions: NativeActions,
}

impl Accessibility {
    pub(super) fn new<H>(window: &Window, events: WindowEvents, handler: H) -> Option<Self>
    where
        H: ActivationHandler + ActionHandler + DeactivationHandler + Clone + 'static,
    {
        let parent = view(window)?;
        let mtm = MainThreadMarker::new()?;
        let allocated = mtm
            .alloc::<AccessibilityView>()
            .set_ivars(AccessibilityViewState {
                events: events.clone(),
                escape: Cell::new(None),
            });
        // SAFETY: The window and accessibility view are owned by the UIKit main thread.
        let host: Retained<AccessibilityView> = unsafe { msg_send_id![super(allocated), init] };
        let adapter = unsafe {
            let adapter = SubclassingAdapter::new(
                Retained::as_ptr(&host).cast_mut().cast(),
                handler.clone(),
                handler.clone(),
                handler,
            );
            host.setFrame(parent.bounds());
            host.setContentScaleFactor(window.scale_factor());
            host.setAutoresizingMask(
                UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
            );
            host.setUserInteractionEnabled(false);
            parent.addSubview(&host);
            adapter
        };
        Some(Self {
            adapter,
            host,
            actions: NativeActions::new(window.id(), events),
        })
    }

    pub(super) fn update_if_active(&mut self, update: impl FnOnce() -> TreeUpdate) {
        let mut actions = None;
        let events = self.adapter.update_if_active(|| {
            let mut tree = update();
            actions = Some(NativeActions::project(&mut tree));
            tree
        });
        if let Some(actions) = actions {
            self.actions.update(&self.host, actions);
        }
        if let Some(events) = events {
            events.raise();
        }
    }

    pub(super) fn clear_actions(&mut self) {
        self.actions.clear();
    }

    pub(super) fn prepare(&self, scale_factor: f64, escape: Option<AccessibilityEscape>) {
        if self.host.contentScaleFactor() != scale_factor {
            self.host.setContentScaleFactor(scale_factor);
        }
        self.host.ivars().escape.set(escape);
    }
}

impl Drop for Accessibility {
    fn drop(&mut self) {
        // SAFETY: The accessibility view is owned and dropped on the UIKit main thread.
        unsafe { self.host.removeFromSuperview() };
    }
}

struct AccessibilityViewState {
    events: WindowEvents,
    escape: Cell<Option<AccessibilityEscape>>,
}

declare_class!(
    struct AccessibilityView;

    unsafe impl ClassType for AccessibilityView {
        type Super = UIView;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackAccessibilityView";
    }

    impl DeclaredClass for AccessibilityView {
        type Ivars = AccessibilityViewState;
    }

    unsafe impl NSObjectProtocol for AccessibilityView {}

    unsafe impl AccessibilityView {
        #[method(accessibilityPerformEscape)]
        fn perform_escape(&self) -> Bool {
            let Some(request) = self.ivars().escape.take() else {
                return Bool::NO;
            };
            Bool::new(self.ivars().events.post(UserEvent::IosAccessibilityEscape(request)))
        }
    }
);

/// Physical touch positions and the Metal layer use the full window. UIKit
/// reports the safe content rectangle separately through winit's inner bounds.
pub(super) fn content_geometry(
    window: &Window,
    editor: Option<&crate::ios_text::TextInput>,
) -> ([f64; 4], Option<Rect>) {
    let Ok(inner) = window.inner_position() else {
        return ([0.0; 4], None);
    };
    let Ok(outer) = window.outer_position() else {
        return ([0.0; 4], None);
    };
    let left = inner.x.saturating_sub(outer.x).max(0) as u32;
    let top = inner.y.saturating_sub(outer.y).max(0) as u32;
    let size = window.outer_size();
    let content = window.inner_size();
    let right = size
        .width
        .saturating_sub(content.width.saturating_add(left));
    let bottom = size
        .height
        .saturating_sub(content.height.saturating_add(top));
    let mut insets =
        [left, top, right, bottom].map(|pixels| f64::from(pixels) / window.scale_factor());
    let mut floating = None;
    if let Some(view) = view(window) {
        // SAFETY: view() verifies the UIKit main thread and keeps the window borrowed.
        let keyboard = unsafe { view.keyboardLayoutGuide().layoutFrame() };
        let bounds = view.bounds();
        let bottom = bounds.origin.y + bounds.size.height;
        if keyboard.size.width >= bounds.size.width - 1.0
            && keyboard.size.height > 0.0
            && keyboard.origin.y + keyboard.size.height >= bottom - insets[3] - 1.0
        {
            let top = editor
                .and_then(|editor| editor.accessory_frame(view))
                .filter(|accessory| {
                    accessory.size.height > 0.0
                        && accessory.size.width >= bounds.size.width - 1.0
                        && accessory.origin.y <= keyboard.origin.y
                        && accessory.origin.y + accessory.size.height >= keyboard.origin.y - 1.0
                })
                .map_or(keyboard.origin.y, |accessory| accessory.origin.y);
            insets[3] = insets[3].max(bottom - top);
        } else if keyboard.origin.y < bottom - insets[3] - 1.0 {
            let window = Rect::new(
                0.0,
                0.0,
                bounds.size.width as f32,
                bounds.size.height as f32,
            );
            let keyboard = Rect::new(
                (keyboard.origin.x - bounds.origin.x) as f32,
                (keyboard.origin.y - bounds.origin.y) as f32,
                keyboard.size.width as f32,
                keyboard.size.height as f32,
            )
            .intersect(window);
            if keyboard.width > 0.0 && keyboard.height > 0.0 {
                floating = Some(keyboard);
            }
        }
    }
    (insets, floating)
}

pub(super) fn view(window: &Window) -> Option<&UIView> {
    MainThreadMarker::new()?;
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: The borrowed winit window owns this UIView, and the caller is on its main thread.
    Some(unsafe { handle.ui_view.cast::<UIView>().as_ref() })
}

pub(super) fn reduce_motion() -> bool {
    // SAFETY: The UIKit setting is read on the winit UI thread.
    unsafe { UIAccessibilityIsReduceMotionEnabled().as_bool() }
}

pub(super) fn text_scale() -> f32 {
    // SAFETY: UIFontMetrics reads the user's current Dynamic Type category.
    unsafe { UIFontMetrics::defaultMetrics().scaledValueForValue(1.0) as f32 }
}

pub(super) struct AccessibilityObserver {
    center: Retained<NSNotificationCenter>,
    tokens: [Retained<NSObject>; 2],
}

impl AccessibilityObserver {
    pub(super) fn new(events: WindowEvents) -> Self {
        let callback = RcBlock::new(move |_: NonNull<NSNotification>| {
            events.post(UserEvent::IosAccessibilityPreferencesChanged);
        });
        // SAFETY: Install the observer on the main queue for the life of the window.
        unsafe {
            let center = NSNotificationCenter::defaultCenter();
            let queue = NSOperationQueue::mainQueue();
            let tokens = [
                UIAccessibilityReduceMotionStatusDidChangeNotification,
                UIContentSizeCategoryDidChangeNotification,
            ]
            .map(|name| {
                center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    None,
                    Some(&queue),
                    &callback,
                )
            });
            Self { center, tokens }
        }
    }
}

impl Drop for AccessibilityObserver {
    fn drop(&mut self) {
        // SAFETY: The window owns the observer and removes it on the UIKit main thread.
        for token in &self.tokens {
            unsafe { self.center.removeObserver(token) };
        }
    }
}
