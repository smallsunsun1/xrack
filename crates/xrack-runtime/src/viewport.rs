use crate::{Length, ListModel, Orientation, Property, RowKey, Value};
use std::{
    cell::{Cell, OnceCell, RefCell},
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
    CancelFlick,
    ReturnToBounds,
    To {
        x: Length,
        y: Length,
    },
    ToIndex {
        index: i64,
        alignment: ScrollAlignment,
    },
    /// A source record captured before the view is laid out.
    ToRowKey {
        key: crate::RowKey,
        source_index: usize,
        alignment: ScrollAlignment,
    },
    /// A declaration-order column. Frozen columns keep their horizontal position.
    ToColumn {
        column: i64,
        alignment: ScrollAlignment,
    },
    /// A displayed row and a declaration-order column, resolved after layout.
    ToCell {
        row: i64,
        column: i64,
        horizontal: ScrollAlignment,
        vertical: ScrollAlignment,
    },
    /// A captured source record and a declaration-order table column.
    ToCellAtRowKey {
        key: crate::RowKey,
        source_index: usize,
        column: i64,
        horizontal: ScrollAlignment,
        vertical: ScrollAlignment,
    },
}

/// Method names used by the XR viewport operations.
#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum ScrollCommandMethod {
    To,
    Index,
    Column,
    Cell,
    SourceIndex,
    SourceCell,
    CancelFlick,
    ReturnToBounds,
}

impl ScrollCommandMethod {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "scroll-to" => Self::To,
            "position-view-at-index" | "position-view-at-row" => Self::Index,
            "position-view-at-column" => Self::Column,
            "position-view-at-cell" => Self::Cell,
            "position-view-at-source-index" => Self::SourceIndex,
            "position-view-at-source-cell" => Self::SourceCell,
            "cancel-flick" => Self::CancelFlick,
            "return-to-bounds" => Self::ReturnToBounds,
            _ => return None,
        })
    }

    pub fn uses_source_row(self) -> bool {
        matches!(self, Self::SourceIndex | Self::SourceCell)
    }

    pub fn command(self, operands: &[Value]) -> Option<ScrollCommand> {
        use Value::{Int, Length, ScrollAlignment};
        let action = match (self, operands) {
            (Self::CancelFlick, []) => ScrollAction::CancelFlick,
            (Self::ReturnToBounds, []) => ScrollAction::ReturnToBounds,
            (Self::To, &[Length(x), Length(y)]) => ScrollAction::To { x, y },
            (Self::Index, &[Int(index), ScrollAlignment(alignment)]) => {
                ScrollAction::ToIndex { index, alignment }
            }
            (Self::Column, &[Int(column), ScrollAlignment(alignment)]) => {
                ScrollAction::ToColumn { column, alignment }
            }
            (
                Self::Cell,
                &[
                    Int(row),
                    Int(column),
                    ScrollAlignment(horizontal),
                    ScrollAlignment(vertical),
                ],
            ) => ScrollAction::ToCell {
                row,
                column,
                horizontal,
                vertical,
            },
            (Self::SourceIndex, &[Int(row), ScrollAlignment(alignment)]) => {
                return Some(ScrollCommand::Source(row, SourceScroll::Row(alignment)));
            }
            (
                Self::SourceCell,
                &[
                    Int(row),
                    Int(column),
                    ScrollAlignment(horizontal),
                    ScrollAlignment(vertical),
                ],
            ) => {
                return Some(ScrollCommand::Source(
                    row,
                    SourceScroll::Cell {
                        column,
                        horizontal,
                        vertical,
                    },
                ));
            }
            _ => return None,
        };
        Some(ScrollCommand::Direct(action))
    }
}

#[doc(hidden)]
pub enum ScrollCommand {
    Direct(ScrollAction),
    Source(i64, SourceScroll),
}

impl ScrollCommand {
    pub fn try_resolve<E>(
        self,
        key_at: impl FnOnce(usize) -> Result<Option<RowKey>, E>,
    ) -> Result<Option<ScrollAction>, E> {
        match self {
            Self::Direct(action) => Ok(Some(action)),
            Self::Source(row, command) => command.try_resolve(row, key_at),
        }
    }

    pub fn resolve(self, key_at: impl FnOnce(usize) -> Option<RowKey>) -> Option<ScrollAction> {
        self.try_resolve(|index| Ok::<_, std::convert::Infallible>(key_at(index)))
            .unwrap_or_else(|error| match error {})
    }
}

/// Source positions are converted to stable row keys when the command is queued.
#[derive(Clone, Copy)]
#[doc(hidden)]
pub enum SourceScroll {
    Row(ScrollAlignment),
    Cell {
        column: i64,
        horizontal: ScrollAlignment,
        vertical: ScrollAlignment,
    },
}

impl SourceScroll {
    pub fn try_resolve<E>(
        self,
        row: i64,
        key_at: impl FnOnce(usize) -> Result<Option<RowKey>, E>,
    ) -> Result<Option<ScrollAction>, E> {
        let Ok(source_index) = usize::try_from(row) else {
            return Ok(None);
        };
        if matches!(self, Self::Cell { column, .. } if usize::try_from(column).is_err()) {
            return Ok(None);
        }
        let Some(key) = key_at(source_index)? else {
            return Ok(None);
        };
        Ok(Some(match self {
            Self::Row(alignment) => ScrollAction::ToRowKey {
                key,
                source_index,
                alignment,
            },
            Self::Cell {
                column,
                horizontal,
                vertical,
            } => ScrollAction::ToCellAtRowKey {
                key,
                source_index,
                column,
                horizontal,
                vertical,
            },
        }))
    }

    pub fn resolve<M: ListModel>(
        self,
        row: i64,
        model: impl FnOnce() -> Rc<M>,
    ) -> Option<ScrollAction> {
        self.try_resolve(row, |index| {
            let model = model();
            Ok::<_, std::convert::Infallible>(model.key_at(index))
        })
        .unwrap_or_else(|error| match error {})
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewportMotion {
    #[default]
    Idle,
    Dragging,
    Flicking,
}

/// A viewport's stable identity and reactive geometry, owned by its component instance.
#[derive(Clone, Default)]
pub struct ViewportHandle(Rc<ViewportProperties>);

#[derive(Default)]
struct ViewportProperties {
    scroll_notifications: bool,
    motion: Option<Box<MotionProperties>>,
    content_x: Property<Length>,
    content_y: Property<Length>,
    visible_width: Property<Length>,
    visible_height: Property<Length>,
    ranges: [ViewportRange; 2],
    horizontal: OnceCell<OffsetBinding>,
    vertical: OnceCell<OffsetBinding>,
}

#[derive(Default)]
struct MotionProperties {
    phase: Cell<ViewportMotion>,
    dragging: Property<bool>,
    flicking: Property<bool>,
    moving: Property<bool>,
    overshoot: [RangeMetric; 2],
    returning: Cell<[bool; 2]>,
    rebounding: Property<bool>,
}

#[derive(Default)]
struct ViewportRange {
    position: RangeMetric,
    size: RangeMetric,
}

/// Keep the latest layout sample and allocate a dependency only when it is read.
#[derive(Default)]
struct RangeMetric {
    sample: Cell<f64>,
    observed: OnceCell<Box<Property<f64>>>,
}

impl RangeMetric {
    fn get(&self) -> f64 {
        self.observed
            .get_or_init(Default::default)
            .get(|| self.sample.get())
    }

    fn publish(&self, value: f64) {
        self.sample.set(value);
        if let Some(property) = self.observed.get() {
            property.set_if_changed(value);
        }
    }
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
    pub fn identity(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }

    #[doc(hidden)]
    pub fn with_scroll_notifications() -> Self {
        Self(Rc::new(ViewportProperties {
            scroll_notifications: true,
            ..ViewportProperties::default()
        }))
    }

    #[doc(hidden)]
    pub fn for_flickable(scroll_notifications: bool) -> Self {
        Self(Rc::new(ViewportProperties {
            scroll_notifications,
            motion: Some(Box::default()),
            ..ViewportProperties::default()
        }))
    }

    pub fn dragging(&self) -> bool {
        self.0
            .motion
            .as_ref()
            .is_some_and(|state| state.dragging.get(|| false))
    }
    pub fn flicking(&self) -> bool {
        self.0
            .motion
            .as_ref()
            .is_some_and(|state| state.flicking.get(|| false))
    }
    pub fn moving(&self) -> bool {
        self.0
            .motion
            .as_ref()
            .is_some_and(|state| state.moving.get(|| false))
    }

    pub fn rebounding(&self) -> bool {
        self.0
            .motion
            .as_ref()
            .is_some_and(|state| state.rebounding.get(|| false))
    }

    pub fn horizontal_overshoot(&self) -> Length {
        self.overshoot(Orientation::Horizontal)
    }
    pub fn vertical_overshoot(&self) -> Length {
        self.overshoot(Orientation::Vertical)
    }

    fn overshoot(&self, axis: Orientation) -> Length {
        Length(self.0.motion.as_ref().map_or(0.0, |state| {
            state.overshoot[usize::from(axis == Orientation::Vertical)].get()
        }))
    }

    #[doc(hidden)]
    pub fn observes_motion(&self) -> bool {
        self.0.motion.is_some()
    }

    #[doc(hidden)]
    pub fn publish_motion(&self, motion: ViewportMotion) {
        if let Some(state) = &self.0.motion {
            state.phase.set(motion);
            state
                .dragging
                .set_if_changed(motion == ViewportMotion::Dragging);
            state
                .flicking
                .set_if_changed(motion == ViewportMotion::Flicking);
            state.moving.set_if_changed(
                motion != ViewportMotion::Idle || state.returning.get().contains(&true),
            );
        }
    }

    #[doc(hidden)]
    pub fn publish_overshoot(&self, axis: Orientation, value: f32, returning: bool) {
        if let Some(state) = &self.0.motion {
            let index = usize::from(axis == Orientation::Vertical);
            state.overshoot[index].publish(f64::from(value));
            let mut phases = state.returning.get();
            phases[index] = returning;
            state.returning.set(phases);
            let rebounding = phases.contains(&true);
            state.rebounding.set_if_changed(rebounding);
            state
                .moving
                .set_if_changed(state.phase.get() != ViewportMotion::Idle || rebounding);
        }
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

    fn range(&self, axis: Orientation) -> &ViewportRange {
        &self.0.ranges[usize::from(axis == Orientation::Vertical)]
    }

    pub fn scroll_position(&self, axis: Orientation) -> f64 {
        self.range(axis).position.get()
    }

    pub fn scroll_size(&self, axis: Orientation) -> f64 {
        self.range(axis).size.get()
    }

    #[doc(hidden)]
    pub fn publish_scroll_range(&self, axis: Orientation, offset: f32, page: f32, maximum: f32) {
        let page = f64::from(page);
        let extent = page + f64::from(maximum);
        let (position, size) =
            if page > 0.0 && maximum >= 0.0 && extent.is_finite() && offset.is_finite() {
                let size = page / extent;
                ((f64::from(offset) / extent).clamp(0.0, 1.0 - size), size)
            } else {
                (0.0, 0.0)
            };
        let range = self.range(axis);
        range.position.publish(position);
        range.size.publish(size);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_binding_remains_cached_across_drag_to_flick_transition() {
        let viewport = ViewportHandle::for_flickable(false);
        let moving = Property::default();
        let dragging = Property::default();
        viewport.publish_motion(ViewportMotion::Dragging);
        assert!(moving.get(|| viewport.moving()));
        assert!(dragging.get(|| viewport.dragging()));
        viewport.publish_motion(ViewportMotion::Flicking);
        assert!(!moving.is_dirty());
        assert!(dragging.is_dirty());
        assert!(viewport.flicking());
        viewport.publish_motion(ViewportMotion::Idle);
        assert!(moving.is_dirty());
        assert!(!moving.get(|| viewport.moving()));
    }

    #[test]
    fn scroll_range_invalidates_only_the_changed_axis_and_metric() {
        let viewport = ViewportHandle::default();
        viewport.publish_scroll_range(Orientation::Horizontal, 0.0, 100.0, 100.0);
        assert!(
            viewport
                .range(Orientation::Horizontal)
                .size
                .observed
                .get()
                .is_none()
        );
        let size = Property::default();
        let position = Property::default();
        let read_size = || viewport.scroll_size(Orientation::Horizontal);
        let read_position = || viewport.scroll_position(Orientation::Horizontal);
        assert_eq!(size.get(read_size), 0.5);
        assert_eq!(position.get(read_position), 0.0);

        viewport.publish_scroll_range(Orientation::Vertical, 30.0, 20.0, 80.0);
        assert!(
            viewport
                .range(Orientation::Vertical)
                .position
                .observed
                .get()
                .is_none()
        );
        assert!(!position.is_dirty());
        assert!(!size.is_dirty());
        viewport.publish_scroll_range(Orientation::Horizontal, 50.0, 100.0, 100.0);
        assert!(position.is_dirty());
        assert!(!size.is_dirty());
        assert_eq!(position.get(read_position), 0.25);

        viewport.publish_scroll_range(Orientation::Horizontal, 50.0, 100.0, 100.0);
        assert!(!position.is_dirty());
        viewport.publish_scroll_range(Orientation::Horizontal, 0.0, 0.0, 0.0);
        assert!(position.is_dirty());
        assert!(size.is_dirty());
        assert_eq!(size.get(read_size), 0.0);
    }
}
