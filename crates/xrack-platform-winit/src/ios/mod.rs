mod accessibility_actions;
pub(super) mod back;
pub(super) mod directory;
pub(super) mod edit_menu;
pub(super) mod file_drop;
pub(super) mod keyboard;
pub(super) mod native_dialog;
pub(super) mod pointer;
pub(super) mod text;

use self::accessibility_actions::NativeActions;
use crate::{AccessibilityEscape, UserEvent, desktop::WindowEvents};
use accesskit::{Action, ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_ios::SubclassingAdapter;
use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass, class, declare_class, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{
    CGPoint, CGRect, CGSize, MainThreadMarker, NSArray, NSNotification, NSNotificationCenter,
    NSNumber, NSNumberFormatter, NSNumberFormatterStyle, NSOperationQueue, NSString,
};
use objc2_ui_kit::{
    NSObjectUIAccessibility, UIAccessibilityBoldTextStatusDidChangeNotification,
    UIAccessibilityButtonShapesEnabled,
    UIAccessibilityButtonShapesEnabledStatusDidChangeNotification, UIAccessibilityContrast,
    UIAccessibilityIsBoldTextEnabled, UIAccessibilityIsOnOffSwitchLabelsEnabled,
    UIAccessibilityIsReduceMotionEnabled, UIAccessibilityOnOffSwitchLabelsDidChangeNotification,
    UIAccessibilityPageScrolledNotification, UIAccessibilityPostNotification,
    UIAccessibilityReduceMotionStatusDidChangeNotification,
    UIAccessibilityShouldDifferentiateWithoutColor,
    UIAccessibilityShouldDifferentiateWithoutColorDidChangeNotification,
    UIContentSizeCategoryDidChangeNotification, UIFontMetrics, UIScrollView,
    UIScrollViewContentInsetAdjustmentBehavior, UIScrollViewDelegate, UITraitChangeHandler,
    UITraitCollection, UITraitEnvironment, UITraitEnvironmentLayoutDirection, UIUserInterfaceStyle,
    UIView, UIViewAutoresizing,
};
use std::{cell::Cell, ptr::NonNull};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use xrack_layout::{LayoutNode, Rect, ScrollLayout, StackTransitions, Target};
use xrack_runtime::{
    ColorScheme, LayoutDirection, Node, Orientation, PageIdentity, group_index, group_item,
};

declare_class!(
    struct StatusBarScrollView;

    unsafe impl ClassType for StatusBarScrollView {
        type Super = UIScrollView;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackStatusBarScrollView";
    }

    impl DeclaredClass for StatusBarScrollView {
        type Ivars = ();
    }

    unsafe impl NSObjectProtocol for StatusBarScrollView {}

    unsafe impl StatusBarScrollView {
        // UIKit finds this view for the status bar. XRack owns touch hit testing.
        #[method(pointInside:withEvent:)]
        fn point_inside(&self, _: CGPoint, _: Option<&AnyObject>) -> bool {
            false
        }
    }
);

declare_class!(
    struct ScrollToTopDelegate;

    unsafe impl ClassType for ScrollToTopDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackScrollToTopDelegate";
    }

    impl DeclaredClass for ScrollToTopDelegate {
        type Ivars = WindowEvents;
    }

    unsafe impl NSObjectProtocol for ScrollToTopDelegate {}

    unsafe impl UIScrollViewDelegate for ScrollToTopDelegate {
        #[method(scrollViewShouldScrollToTop:)]
        fn should_scroll_to_top(&self, _: &UIScrollView) -> bool {
            self.ivars().post(UserEvent::IosScrollToTop);
            false
        }
    }
);

pub(super) struct ScrollToTop {
    scroll: Retained<StatusBarScrollView>,
    _delegate: Retained<ScrollToTopDelegate>,
}

impl ScrollToTop {
    pub(super) fn new(window: &Window, events: WindowEvents) -> Option<Self> {
        let parent = view(window)?;
        let mtm = MainThreadMarker::new()?;
        let delegate = mtm.alloc::<ScrollToTopDelegate>().set_ivars(events);
        let scroll = mtm.alloc::<StatusBarScrollView>().set_ivars(());
        // SAFETY: The parent, scroll view and delegate are owned by the UIKit main thread.
        unsafe {
            let delegate: Retained<ScrollToTopDelegate> = msg_send_id![super(delegate), init];
            let frame = parent.bounds();
            let scroll: Retained<StatusBarScrollView> =
                msg_send_id![super(scroll), initWithFrame: frame];
            scroll.setAutoresizingMask(UIViewAutoresizing::FlexibleWidth);
            scroll.setContentInsetAdjustmentBehavior(
                UIScrollViewContentInsetAdjustmentBehavior::UIScrollViewContentInsetAdjustmentNever,
            );
            scroll.setShowsVerticalScrollIndicator(false);
            scroll.setAccessibilityElementsHidden(true);
            scroll.setScrollsToTop(true);
            // UIKit ignores a scroll view already at its top. XRack owns the real offset.
            scroll.setContentSize(CGSize::new(frame.size.width, frame.size.height + 64.0));
            scroll.setContentOffset(CGPoint::new(0.0, 32.0));
            scroll.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            parent.addSubview(&scroll);
            Some(Self {
                scroll,
                _delegate: delegate,
            })
        }
    }
}

impl Drop for ScrollToTop {
    fn drop(&mut self) {
        // SAFETY: The host drops the bridge on the UIKit main thread.
        unsafe { self.scroll.removeFromSuperview() };
    }
}

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
        announce_accessibility_scroll(&status);
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
                let percent_at = |offset| 100.0 * f64::from(offset) / f64::from(scroll.max_offset);
                let percent = percent_at(scroll.offset);
                let difference = (percent - percent_at(*previous)).abs();
                // Long virtual views can move by less than one percentage point per gesture.
                let digits = (-difference.log10()).ceil().clamp(0.0, 6.0) as usize;
                Some(format!("{percent:.digits$}% scrolled"))
            }
        }
    }
}

pub(super) fn announce_accessibility_scroll(status: &str) {
    let status = NSString::from_str(status);
    // SAFETY: Accessibility scroll updates and notifications run on the UIKit thread.
    unsafe {
        UIAccessibilityPostNotification(UIAccessibilityPageScrolledNotification, Some(&status))
    };
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
            Bool::new(self.ivars().events.post(UserEvent::IosBackRequested(request)))
        }
    }
);

#[derive(Clone, Copy)]
pub(super) enum KeyboardOcclusion {
    None,
    Docked,
    Floating(Rect),
}

impl KeyboardOcclusion {
    pub(super) fn floating(self) -> Option<Rect> {
        match self {
            Self::Floating(bounds) => Some(bounds),
            Self::Docked | Self::None => None,
        }
    }
}

/// Physical touch positions and the Metal layer use the full window. UIKit
/// reports the safe content rectangle separately through winit's inner bounds.
pub(super) fn content_geometry(
    window: &Window,
    editor: Option<&text::TextInput>,
) -> ([f64; 4], KeyboardOcclusion) {
    let Ok(inner) = window.inner_position() else {
        return ([0.0; 4], KeyboardOcclusion::None);
    };
    let Ok(outer) = window.outer_position() else {
        return ([0.0; 4], KeyboardOcclusion::None);
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
    let mut occlusion = KeyboardOcclusion::None;
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
            if bottom - top > insets[3] + 1.0 {
                occlusion = KeyboardOcclusion::Docked;
            }
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
                occlusion = KeyboardOcclusion::Floating(keyboard);
            }
        }
    }
    (insets, occlusion)
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

pub(super) fn software_keyboard_visible(window: &Window, editor: &text::TextInput) -> bool {
    view(window).is_some_and(|view| {
        let rect = |frame: CGRect| {
            Rect::new(
                frame.origin.x as f32,
                frame.origin.y as f32,
                frame.size.width as f32,
                frame.size.height as f32,
            )
        };
        // SAFETY: The window's view and layout guide are used on the UIKit main thread.
        let occupied = unsafe {
            let mut content = rect(view.bounds());
            content.height = (content.height - view.safeAreaInsets().bottom as f32).max(0.0);
            rect(view.keyboardLayoutGuide().layoutFrame()).intersect(content)
        };
        if occupied.width <= 1.0 || occupied.height <= 1.0 {
            return false;
        }
        // UIKit can report only our input accessory when a hardware keyboard is active.
        editor.accessory_frame(view).is_none_or(|accessory| {
            let overlap = occupied.intersect(rect(accessory));
            occupied.width - overlap.width > 1.0 || occupied.height - overlap.height > 1.0
        })
    })
}

pub(super) struct SystemAppearance {
    pub(super) scheme: ColorScheme,
    pub(super) increased_contrast: bool,
    pub(super) layout_direction: LayoutDirection,
}

pub(super) fn system_appearance(window: &Window) -> SystemAppearance {
    // SAFETY: view() verifies that traits are read on the UIKit main thread.
    unsafe {
        let Some(view) = view(window) else {
            return SystemAppearance {
                scheme: ColorScheme::System,
                increased_contrast: false,
                layout_direction: LayoutDirection::LeftToRight,
            };
        };
        let traits = view.traitCollection();
        let scheme = match traits.userInterfaceStyle() {
            UIUserInterfaceStyle::Light => ColorScheme::Light,
            UIUserInterfaceStyle::Dark => ColorScheme::Dark,
            _ => ColorScheme::System,
        };
        SystemAppearance {
            scheme,
            increased_contrast: traits.accessibilityContrast() == UIAccessibilityContrast::High,
            layout_direction: if traits.layoutDirection()
                == UITraitEnvironmentLayoutDirection::RightToLeft
            {
                LayoutDirection::RightToLeft
            } else {
                LayoutDirection::LeftToRight
            },
        }
    }
}

pub(super) fn observe_system_appearance(window: &Window, events: WindowEvents) {
    let Some(view) = view(window) else {
        return;
    };
    let handler = RcBlock::new(
        move |_: NonNull<ProtocolObject<dyn UITraitEnvironment>>, _: NonNull<UITraitCollection>| {
            events.post(UserEvent::IosAppearanceChanged);
        },
    );
    let handler: UITraitChangeHandler = std::ptr::from_ref(&*handler).cast_mut();
    // SAFETY: These public iOS 17 selectors are missing from the generated bindings.
    // UIKit copies the block and keeps the registration active for the view's lifetime.
    unsafe {
        let color_traits: Retained<NSArray<NSObject>> = msg_send_id![
            UITraitCollection::class(),
            systemTraitsAffectingColorAppearance
        ];
        let traits: Retained<NSArray<NSObject>> =
            msg_send_id![&*color_traits, arrayByAddingObject: class!(UITraitLayoutDirection)];
        let _registration: Retained<NSObject> =
            msg_send_id![view, registerForTraitChanges: &*traits withHandler: handler];
    }
}

pub(super) fn reduce_motion() -> bool {
    // SAFETY: The UIKit setting is read on the winit UI thread.
    unsafe { UIAccessibilityIsReduceMotionEnabled().as_bool() }
}

pub(super) fn text_scale() -> f32 {
    // SAFETY: UIFontMetrics reads the user's current Dynamic Type category.
    unsafe { UIFontMetrics::defaultMetrics().scaledValueForValue(1.0) as f32 }
}

pub(super) fn progress_value(position: f64) -> String {
    let number = NSNumber::numberWithDouble(position);
    // SAFETY: Foundation receives a live NSNumber and a defined formatter style.
    unsafe {
        NSNumberFormatter::localizedStringFromNumber_numberStyle(
            &number,
            NSNumberFormatterStyle::NSNumberFormatterPercentStyle,
        )
    }
    .to_string()
}

pub(super) fn bold_text() -> bool {
    // SAFETY: The UIKit setting is read on the winit UI thread.
    unsafe { UIAccessibilityIsBoldTextEnabled().as_bool() }
}

pub(super) fn switch_labels() -> bool {
    // SAFETY: The UIKit setting is read on the winit UI thread.
    unsafe { UIAccessibilityIsOnOffSwitchLabelsEnabled().as_bool() }
}

pub(super) fn button_shapes() -> bool {
    // SAFETY: The UIKit setting is read on the UI thread. This API is available on iOS 18.
    unsafe { UIAccessibilityButtonShapesEnabled().as_bool() }
}

pub(super) fn differentiate_without_color() -> bool {
    // SAFETY: The UIKit setting is read on the winit UI thread.
    unsafe { UIAccessibilityShouldDifferentiateWithoutColor().as_bool() }
}

pub(super) struct AccessibilityObserver {
    center: Retained<NSNotificationCenter>,
    tokens: [Retained<NSObject>; 6],
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
                UIAccessibilityBoldTextStatusDidChangeNotification,
                UIAccessibilityOnOffSwitchLabelsDidChangeNotification,
                UIAccessibilityButtonShapesEnabledStatusDidChangeNotification,
                UIAccessibilityShouldDifferentiateWithoutColorDidChangeNotification,
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
