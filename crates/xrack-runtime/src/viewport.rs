use crate::{Length, Orientation, Property};
use std::{
    cell::{OnceCell, RefCell},
    fmt,
    rc::Rc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollAlignment {
    Beginning,
    Center,
    End,
    Visible,
    #[default]
    Contain,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollAction {
    To {
        x: Length,
        y: Length,
    },
    ToIndex {
        index: i64,
        alignment: ScrollAlignment,
    },
}

/// A viewport's stable identity and reactive geometry, owned by its component instance.
#[derive(Clone, Default)]
pub struct ViewportHandle(Rc<ViewportProperties>);

#[derive(Default)]
struct ViewportProperties {
    scroll_notifications: bool,
    content_x: Property<Length>,
    content_y: Property<Length>,
    visible_width: Property<Length>,
    visible_height: Property<Length>,
    horizontal: OnceCell<OffsetBinding>,
    vertical: OnceCell<OffsetBinding>,
}

struct OffsetBinding {
    read: Box<dyn Fn() -> Length>,
    write: Box<dyn Fn(Length)>,
}

impl PartialEq for ViewportHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ViewportHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ViewportHandle")
            .finish_non_exhaustive()
    }
}

impl ViewportHandle {
    #[doc(hidden)]
    pub fn with_scroll_notifications() -> Self {
        Self(Rc::new(ViewportProperties {
            scroll_notifications: true,
            ..ViewportProperties::default()
        }))
    }

    #[doc(hidden)]
    pub fn observes_user_scroll(&self) -> bool {
        self.0.scroll_notifications
    }

    fn offset_binding(&self, axis: Orientation) -> &OnceCell<OffsetBinding> {
        match axis {
            Orientation::Horizontal => &self.0.horizontal,
            Orientation::Vertical => &self.0.vertical,
        }
    }

    #[doc(hidden)]
    pub fn bind_offset(
        &self,
        axis: Orientation,
        read: impl Fn() -> Length + 'static,
        write: impl Fn(Length) + 'static,
    ) {
        let binding = OffsetBinding {
            read: Box::new(read),
            write: Box::new(write),
        };
        assert!(
            self.offset_binding(axis).set(binding).is_ok(),
            "viewport offset already bound"
        );
    }

    #[doc(hidden)]
    pub fn has_bound_offset(&self, axis: Orientation) -> bool {
        self.offset_binding(axis).get().is_some()
    }

    #[doc(hidden)]
    pub fn bound_offset(&self, axis: Orientation) -> Option<f32> {
        let binding = self.offset_binding(axis).get()?;
        Some((-(binding.read)().0).max(0.0).min(f64::from(f32::MAX)) as f32)
    }

    #[doc(hidden)]
    pub fn write_bound_offset(&self, axis: Orientation, offset: f32) {
        if let Some(binding) = self.offset_binding(axis).get() {
            (binding.write)(Length(-f64::from(offset)));
        }
    }

    pub fn content_x(&self) -> Length {
        self.0.content_x.get(Length::default)
    }
    pub fn content_y(&self) -> Length {
        self.0.content_y.get(Length::default)
    }
    pub fn visible_width(&self) -> Length {
        self.0.visible_width.get(Length::default)
    }
    pub fn visible_height(&self) -> Length {
        self.0.visible_height.get(Length::default)
    }

    #[doc(hidden)]
    pub fn publish(&self, x: Length, y: Length, width: Length, height: Length) {
        self.0.content_x.set_if_changed(x);
        self.0.content_y.set_if_changed(y);
        self.0.visible_width.set_if_changed(width);
        self.0.visible_height.set_if_changed(height);
    }

    pub fn request(&self, action: ScrollAction) {
        ACTIVE.with(|active| {
            if let Some(requests) = &*active.borrow() {
                requests.0.borrow_mut().push(ScrollRequest {
                    target: self.clone(),
                    action,
                });
            }
        });
    }
}

pub struct ScrollRequest {
    pub target: ViewportHandle,
    pub action: ScrollAction,
}

/// Commands are resolved against the window's newly prepared scene.
#[derive(Clone, Default)]
pub struct ScrollRequests(Rc<RefCell<Vec<ScrollRequest>>>);

thread_local! {
    static ACTIVE: RefCell<Option<ScrollRequests>> = const { RefCell::new(None) };
}

pub struct ScrollContext(Option<ScrollRequests>);

impl ScrollRequests {
    pub fn enter(&self) -> ScrollContext {
        ScrollContext(ACTIVE.with(|active| active.replace(Some(self.clone()))))
    }

    pub fn drain(&self) -> Vec<ScrollRequest> {
        std::mem::take(&mut *self.0.borrow_mut())
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

impl Drop for ScrollContext {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.replace(self.0.take()));
    }
}
