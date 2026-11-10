use crate::{Length, Property};
use std::{cell::Cell, fmt, rc::Rc};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SwipeSample {
    pub pressed_x: Length,
    pub pressed_y: Length,
    pub current_x: Length,
    pub current_y: Length,
}

#[derive(Clone, Default)]
pub struct SwipeHandle(Rc<Properties>);

#[derive(Default)]
struct Properties {
    swiping: Property<bool>,
    pressed_x: Property<Length>,
    pressed_y: Property<Length>,
    current_x: Property<Length>,
    current_y: Property<Length>,
    cancel: Cell<bool>,
}

impl fmt::Debug for SwipeHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SwipeHandle")
            .finish_non_exhaustive()
    }
}

impl PartialEq for SwipeHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl SwipeHandle {
    pub fn swiping(&self) -> bool {
        self.0.swiping.get(|| false)
    }
    pub fn pressed_x(&self) -> Length {
        self.0.pressed_x.get(Length::default)
    }
    pub fn pressed_y(&self) -> Length {
        self.0.pressed_y.get(Length::default)
    }
    pub fn current_x(&self) -> Length {
        self.0.current_x.get(Length::default)
    }
    pub fn current_y(&self) -> Length {
        self.0.current_y.get(Length::default)
    }

    pub fn cancel(&self) {
        if self.swiping() {
            self.0.cancel.set(true);
        }
    }

    #[doc(hidden)]
    pub fn cancel_requested(&self) -> bool {
        self.0.cancel.get()
    }

    #[doc(hidden)]
    pub fn publish(&self, sample: SwipeSample, swiping: bool) {
        if !swiping {
            self.0.cancel.set(false);
        }
        self.0.pressed_x.set_if_changed(sample.pressed_x);
        self.0.pressed_y.set_if_changed(sample.pressed_y);
        self.0.current_x.set_if_changed(sample.current_x);
        self.0.current_y.set_if_changed(sample.current_y);
        self.0.swiping.set_if_changed(swiping);
    }
}
