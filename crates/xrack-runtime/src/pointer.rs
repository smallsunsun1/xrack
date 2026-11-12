use crate::{KeyModifiers, Length, ListModel, Property, RowKey, RowLifetime, VecModel};
use std::{
    cell::{Cell, OnceCell},
    fmt,
    ops::Range,
    rc::Rc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerEventKind {
    Down,
    Move,
    Up,
    #[default]
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerButton {
    #[default]
    Primary,
    Secondary,
    Middle,
}

/// A set of buttons accepted by a TouchArea.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerButtons(u8);

impl PointerButtons {
    pub fn contains(self, button: PointerButton) -> bool {
        self.0 & (1 << button as u8) != 0
    }
}

impl<const N: usize> From<[PointerButton; N]> for PointerButtons {
    fn from(buttons: [PointerButton; N]) -> Self {
        buttons.into_iter().collect()
    }
}

impl FromIterator<PointerButton> for PointerButtons {
    fn from_iter<T: IntoIterator<Item = PointerButton>>(buttons: T) -> Self {
        Self(
            buttons
                .into_iter()
                .fold(0, |mask, button| mask | (1 << button as u8)),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerEvent {
    pub kind: PointerEventKind,
    pub button: PointerButton,
    pub x: Length,
    pub y: Length,
    pub inside: bool,
    pub is_touch: bool,
    /// Positive while a touch contact is active. Mouse events use zero.
    pub touch_finger_id: i64,
    pub modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TouchPoint {
    pub id: i64,
    pub x: Length,
    pub y: Length,
    pub start_x: Length,
    pub start_y: Length,
    pub inside: bool,
}

/// Read-only, reactive rows in contact order. Cloning shares the same rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TouchPoints(VecModel<TouchPoint>);

impl From<Vec<TouchPoint>> for TouchPoints {
    fn from(points: Vec<TouchPoint>) -> Self {
        Self(points.into())
    }
}

impl TouchPoints {
    pub fn snapshot(&self) -> Vec<TouchPoint> {
        self.0.snapshot()
    }

    #[doc(hidden)]
    pub fn values_eq(&self, other: &Self) -> bool {
        self.0.values_eq(&other.0)
    }
}

impl ListModel for TouchPoints {
    type Item = TouchPoint;

    fn row_count(&self) -> usize {
        self.0.row_count()
    }

    fn item(&self, row: usize) -> TouchPoint {
        self.0.item(row)
    }

    fn row_key(&self, row: usize) -> RowKey {
        ListModel::row_key(&self.0, row)
    }

    fn row_lifetime(&self, key: RowKey) -> RowLifetime {
        self.0.row_lifetime(key)
    }

    fn structure_stamp(&self) -> (u64, u64) {
        self.0.structure_stamp()
    }

    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        self.0.index_of_key(key)
    }

    fn visit_items(&self, visit: impl FnMut(usize, RowKey, &TouchPoint)) {
        self.0.visit_items(visit);
    }

    fn visit_row_keys(&self, range: Range<usize>, visit: impl FnMut(RowKey)) {
        self.0.visit_row_keys(range, visit);
    }
}

#[derive(Default)]
struct PointerState {
    pressed: Property<bool>,
    contains_press: Property<bool>,
    has_hover: Property<bool>,
    mouse_x: Property<Length>,
    mouse_y: Property<Length>,
    pressed_x: Property<Length>,
    pressed_y: Property<Length>,
    mouse_press: Cell<Option<bool>>,
    touch_points: OnceCell<VecModel<TouchPoint>>,
}

/// Reactive state and identity of a TouchArea in one component instance.
#[derive(Clone, Default)]
#[doc(hidden)]
pub struct PointerHandle(Rc<PointerState>);

impl fmt::Debug for PointerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointerHandle").finish_non_exhaustive()
    }
}

impl PartialEq for PointerHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl PointerHandle {
    pub fn pressed(&self) -> bool {
        self.0.pressed.get(|| false)
    }

    pub fn contains_press(&self) -> bool {
        self.0.contains_press.get(|| false)
    }

    pub fn has_hover(&self) -> bool {
        self.0.has_hover.get(|| false)
    }

    pub fn mouse_x(&self) -> Length {
        self.0.mouse_x.get(Length::default)
    }

    pub fn mouse_y(&self) -> Length {
        self.0.mouse_y.get(Length::default)
    }

    pub fn pressed_x(&self) -> Length {
        self.0.pressed_x.get(Length::default)
    }

    pub fn pressed_y(&self) -> Length {
        self.0.pressed_y.get(Length::default)
    }

    pub fn active_touch_count(&self) -> i64 {
        self.touch_model().len() as i64
    }

    pub fn touch_points(&self) -> TouchPoints {
        TouchPoints(self.touch_model().clone())
    }

    fn touch_model(&self) -> &VecModel<TouchPoint> {
        self.0.touch_points.get_or_init(VecModel::default)
    }

    pub fn publish_hover(&self, position: Option<(Length, Length)>) {
        if let Some((x, y)) = position {
            self.0.mouse_x.set_if_changed(x);
            self.0.mouse_y.set_if_changed(y);
        }
        self.0.has_hover.set_if_changed(position.is_some());
    }

    pub fn publish_event(&self, event: &PointerEvent) {
        use PointerEventKind::{Cancel, Down, Move, Up};

        if event.kind != Cancel {
            self.0.mouse_x.set_if_changed(event.x);
            self.0.mouse_y.set_if_changed(event.y);
        }
        if event.kind == Down {
            self.0.pressed_x.set_if_changed(event.x);
            self.0.pressed_y.set_if_changed(event.y);
        }
        if event.touch_finger_id != 0 {
            let points = self.touch_model();
            let mut row = None;
            points.visit_rows_untracked(usize::MAX, |index, _, point| {
                if point.id == event.touch_finger_id {
                    row = Some((index, *point));
                }
            });
            match event.kind {
                Down | Move => {
                    if let Some((index, point)) = row {
                        points.set_row(
                            index,
                            TouchPoint {
                                x: event.x,
                                y: event.y,
                                inside: event.inside,
                                ..point
                            },
                        );
                    } else if event.kind == Down {
                        points.push(TouchPoint {
                            id: event.touch_finger_id,
                            x: event.x,
                            y: event.y,
                            start_x: event.x,
                            start_y: event.y,
                            inside: event.inside,
                        });
                    }
                }
                Up | Cancel => {
                    if let Some((index, _)) = row {
                        points.remove(index);
                    }
                }
            }
        } else {
            match event.kind {
                Down => self.0.mouse_press.set(Some(event.inside)),
                Move if self.0.mouse_press.get().is_some() => {
                    self.0.mouse_press.set(Some(event.inside))
                }
                Up | Cancel => self.0.mouse_press.set(None),
                Move => {}
            }
        }
        let mouse = self.0.mouse_press.get();
        let mut inside = mouse.unwrap_or(false);
        let mut pressed = mouse.is_some();
        if let Some(points) = self.0.touch_points.get() {
            points.visit_rows_untracked(usize::MAX, |_, _, point| {
                pressed = true;
                inside |= point.inside;
            });
        }
        self.0.pressed.set_if_changed(pressed);
        self.0.contains_press.set_if_changed(inside);
    }
}
