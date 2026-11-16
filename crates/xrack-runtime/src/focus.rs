use crate::{Component, InputRoute, Node, Property, RoutedEvent};
use std::{
    cell::{Cell, RefCell},
    fmt,
    ops::Range,
    rc::Rc,
    time::{Duration, Instant},
};

/// Identity of a focus target in one component instance.
#[derive(Clone, Default)]
pub struct FocusHandle(Rc<FocusSignals>);

/// Reactive press state for one observed push button.
#[derive(Clone, Default)]
#[doc(hidden)]
pub struct ButtonPressHandle(Rc<ButtonPressState>);

#[derive(Default)]
struct ButtonPressState {
    pressed: Property<bool>,
    physical: Cell<bool>,
    synthetic: Cell<bool>,
    clicking: Cell<bool>,
    animation: Option<Box<Cell<Option<Instant>>>>,
}

struct ButtonClick<'a>(&'a ButtonPressState);

impl ButtonClick<'_> {
    fn release(&self) {
        self.0.synthetic.set(false);
        self.0.pressed.set_if_changed(self.0.physical.get());
    }
}

impl Drop for ButtonClick<'_> {
    fn drop(&mut self) {
        if self
            .0
            .animation
            .as_ref()
            .is_none_or(|deadline| deadline.get().is_none())
        {
            self.release();
        }
        self.0.clicking.set(false);
    }
}

impl fmt::Debug for ButtonPressHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ButtonPressHandle").finish_non_exhaustive()
    }
}

impl PartialEq for ButtonPressHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl ButtonPressHandle {
    pub fn animated() -> Self {
        Self(Rc::new(ButtonPressState {
            animation: Some(Box::default()),
            ..ButtonPressState::default()
        }))
    }

    pub fn pressed(&self) -> bool {
        self.0.pressed.get(|| false)
    }

    pub fn publish(&self, pressed: bool) {
        self.0.physical.set(pressed);
        self.0
            .pressed
            .set_if_changed(pressed || self.0.synthetic.get());
    }

    pub fn is_synthetic_pressed(&self) -> bool {
        self.0.synthetic.get()
    }

    pub fn supports_animation(&self) -> bool {
        self.0.animation.is_some()
    }

    fn deadline(&self) -> Option<Instant> {
        self.0.animation.as_ref()?.get()
    }

    pub fn cancel_animation(&self) -> bool {
        if self
            .0
            .animation
            .as_ref()
            .and_then(|deadline| deadline.take())
            .is_none()
        {
            return false;
        }
        self.0.synthetic.set(false);
        self.0.pressed.set_if_changed(self.0.physical.get());
        true
    }

    /// Activate a mounted button using the component's ordinary input route.
    pub fn try_click<E>(
        &self,
        render: impl FnMut() -> Rc<Node>,
        route: impl FnMut(&Node, &[usize], InputRoute<'_>) -> Result<bool, E>,
    ) -> Result<(), E> {
        self.try_click_with_delay(render, route, None)
    }

    /// Press now and let the window scheduler deliver the release.
    pub fn try_animate_click<E>(
        &self,
        render: impl FnMut() -> Rc<Node>,
        route: impl FnMut(&Node, &[usize], InputRoute<'_>) -> Result<bool, E>,
    ) -> Result<(), E> {
        let Some(deadline) = self.0.animation.as_deref() else {
            return Ok(());
        };
        self.try_click_with_delay(render, route, Some(deadline))
    }

    fn try_click_with_delay<E>(
        &self,
        mut render: impl FnMut() -> Rc<Node>,
        mut route: impl FnMut(&Node, &[usize], InputRoute<'_>) -> Result<bool, E>,
        deadline: Option<&Cell<Option<Instant>>>,
    ) -> Result<(), E> {
        if self.0.clicking.get() || self.0.physical.get() {
            return Ok(());
        }
        let next_deadline = || Some(Instant::now() + Duration::from_millis(100));
        if self.deadline().is_some() {
            if let Some(deadline) = deadline {
                let scene = render();
                if self
                    .path_in(&scene)
                    .is_some_and(|path| self.is_available(&scene, &path))
                {
                    deadline.set(next_deadline());
                }
            }
            return Ok(());
        }
        // Retain the guard through clicked(), even after the press has been released.
        self.0.clicking.set(true);
        let click = ButtonClick(&self.0);
        let scene = render();
        let Some(path) = self.path_in(&scene) else {
            return Ok(());
        };
        let Some(node) = scene.interactive_at_path(&path) else {
            return Ok(());
        };
        self.0.synthetic.set(true);
        self.0.pressed.set_if_changed(true);
        if node.boolean("focus-on-click", true)
            && let Some(handle) = &node.focus_handle
        {
            handle.request(FocusAction::Focus);
        }
        let mut send = |scene: &Node, event| {
            if !scene
                .at_path(&path)
                .is_some_and(|node| node.button_press_handle.as_ref() == Some(self))
            {
                return Ok(false);
            }
            RoutedEvent::Dispatch(event)
                .with_input(scene, &path, |input| route(scene, &path, input))
                .unwrap_or(Ok(false))
        };
        send(&scene, "pressed")?;
        if let Some(deadline) = deadline {
            let scene = render();
            if self.is_available(&scene, &path) {
                deadline.set(next_deadline());
            } else {
                click.release();
                send(&scene, "canceled")?;
            }
            return Ok(());
        }
        click.release();
        let scene = render();
        if !self.is_available(&scene, &path) {
            send(&scene, "canceled")?;
        } else {
            send(&scene, "released")?;
            send(&render(), "clicked")?;
        }
        Ok(())
    }

    fn is_available(&self, scene: &Node, path: &[usize]) -> bool {
        scene
            .interactive_at_path(path)
            .is_some_and(|node| node.button_press_handle.as_ref() == Some(self))
    }

    fn path_in(&self, scene: &Node) -> Option<Vec<usize>> {
        fn find(node: &Node, handle: &ButtonPressHandle, path: &mut Vec<usize>) -> bool {
            if node.button_press_handle.as_ref() == Some(handle) {
                return true;
            }
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                if find(child, handle, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        find(scene, self, &mut path).then_some(path)
    }
}

/// Mounted buttons with animated activations, owned by the window scheduler.
#[doc(hidden)]
#[derive(Default)]
pub struct ButtonAnimations(Vec<(ButtonPressHandle, bool)>);

impl ButtonAnimations {
    pub fn sync<'a>(&mut self, handles: impl IntoIterator<Item = &'a ButtonPressHandle>) {
        let next: Vec<_> = handles
            .into_iter()
            .map(|handle| {
                let was_pressed = self
                    .0
                    .iter()
                    .find(|entry| &entry.0 == handle)
                    .is_some_and(|entry| entry.1);
                (handle.clone(), was_pressed)
            })
            .collect();
        for (old, _) in &self.0 {
            if !next.iter().any(|entry| &entry.0 == old) {
                old.cancel_animation();
            }
        }
        self.0 = next;
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.0.iter().filter_map(|entry| entry.0.deadline()).min()
    }

    pub fn take_redraw(&mut self) -> bool {
        let mut changed = false;
        for (handle, previous) in &mut self.0 {
            changed |= std::mem::replace(previous, handle.is_synthetic_pressed()) != *previous;
        }
        changed
    }

    pub fn next(&self, scene: &Node, now: Instant) -> Option<ButtonAnimationDelivery> {
        for (handle, _) in &self.0 {
            let Some(deadline) = handle.deadline() else {
                continue;
            };
            let Some(path) = handle.path_in(scene) else {
                handle.cancel_animation();
                continue;
            };
            let release = handle.is_available(scene, &path);
            if !release || now >= deadline {
                handle.cancel_animation();
                return Some(ButtonAnimationDelivery {
                    handle: handle.clone(),
                    path,
                    release,
                });
            }
        }
        None
    }
}

impl Drop for ButtonAnimations {
    fn drop(&mut self) {
        for (handle, _) in &self.0 {
            handle.cancel_animation();
        }
    }
}

#[doc(hidden)]
pub struct ButtonAnimationDelivery {
    handle: ButtonPressHandle,
    path: Vec<usize>,
    release: bool,
}

impl ButtonAnimationDelivery {
    pub fn apply(self, ui: &mut impl Component, scene: &Node) {
        self.handle.0.clicking.set(true);
        let _click = ButtonClick(&self.handle.0);
        if self.release {
            RoutedEvent::Dispatch("released").deliver_input(ui, scene, &self.path);
            RoutedEvent::Dispatch("clicked").deliver_input(ui, scene, &self.path);
        } else {
            RoutedEvent::Dispatch("canceled").deliver_input(ui, scene, &self.path);
        }
    }
}

#[derive(Default)]
struct FocusSignals {
    direct: Property<bool>,
    within: Property<bool>,
    text: Option<Box<TextSelectionSignals>>,
    commands: Option<Box<TextCommandSignals>>,
}

#[derive(Default)]
struct TextSelectionSignals {
    offsets: [Property<i64>; 3],
    selected: Property<Rc<str>>,
}

#[derive(Default)]
struct TextCommandSignals {
    undo: Property<bool>,
    redo: Property<bool>,
    cut: Property<bool>,
    copy: Property<bool>,
}

/// Availability of commands on the active text editor.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCommandAvailability {
    pub undo: bool,
    pub redo: bool,
    pub cut: bool,
    pub copy: bool,
}

impl fmt::Debug for FocusHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusHandle").finish_non_exhaustive()
    }
}

impl PartialEq for FocusHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusAction {
    Focus,
    Clear,
    OpenChoice,
    CloseChoice,
    Text(TextCommand),
    EditTableCell { row: i64, column: i64 },
    CancelTableEdit,
}

/// A programmatic edit using the same selection, validation and history as keyboard input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextCommand {
    SelectAll,
    ClearSelection,
    SetSelection { anchor: i64, focus: i64 },
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
}

/// Commands lowered from XR focus and editor methods.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum FocusCommandMethod {
    Focus,
    Clear,
    OpenChoice,
    CloseChoice,
    SelectAll,
    ClearSelection,
    SetSelection,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    EditTableCell,
    CancelTableEdit,
}

impl FocusCommandMethod {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "focus" => Self::Focus,
            "clear" => Self::Clear,
            "open-popup" => Self::OpenChoice,
            "close-popup" => Self::CloseChoice,
            "select-all" => Self::SelectAll,
            "clear-selection" => Self::ClearSelection,
            "set-selection-offsets" => Self::SetSelection,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            "cut" => Self::Cut,
            "copy" => Self::Copy,
            "paste" => Self::Paste,
            "edit-cell" => Self::EditTableCell,
            "cancel-edit" => Self::CancelTableEdit,
            _ => return None,
        })
    }

    pub fn action(self, operands: &[i64]) -> Option<FocusAction> {
        let text = match (self, operands) {
            (Self::Focus, []) => return Some(FocusAction::Focus),
            (Self::Clear, []) => return Some(FocusAction::Clear),
            (Self::OpenChoice, []) => return Some(FocusAction::OpenChoice),
            (Self::CloseChoice, []) => return Some(FocusAction::CloseChoice),
            (Self::SelectAll, []) => TextCommand::SelectAll,
            (Self::ClearSelection, []) => TextCommand::ClearSelection,
            (Self::SetSelection, &[anchor, focus]) => TextCommand::SetSelection { anchor, focus },
            (Self::Undo, []) => TextCommand::Undo,
            (Self::Redo, []) => TextCommand::Redo,
            (Self::Cut, []) => TextCommand::Cut,
            (Self::Copy, []) => TextCommand::Copy,
            (Self::Paste, []) => TextCommand::Paste,
            (Self::EditTableCell, &[row, column]) => {
                return Some(FocusAction::EditTableCell { row, column });
            }
            (Self::CancelTableEdit, []) => return Some(FocusAction::CancelTableEdit),
            _ => return None,
        };
        Some(FocusAction::Text(text))
    }
}

#[doc(hidden)]
pub fn is_focus_handler(name: &str) -> bool {
    FocusHandler::ALL
        .iter()
        .any(|handler| handler.name() == name)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusReason {
    #[default]
    Programmatic,
    TabNavigation,
    PointerClick,
    PopupActivation,
    WindowActivation,
    Accessibility,
    LayoutChange,
    KeyboardNavigation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusEvent {
    Gained(FocusReason),
    Lost(FocusReason),
    Within { focused: bool, reason: FocusReason },
}

#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum FocusHandler {
    Gained,
    Lost,
    Changed,
    Within,
}

impl FocusHandler {
    pub const ALL: [Self; 4] = [Self::Gained, Self::Lost, Self::Changed, Self::Within];

    pub fn name(self) -> &'static str {
        match self {
            Self::Gained => "focus-gained",
            Self::Lost => "focus-lost",
            Self::Changed => "focus-changed-event",
            Self::Within => "focus-within-changed",
        }
    }
}

impl FocusEvent {
    #[doc(hidden)]
    pub fn is_loss(self) -> bool {
        matches!(self, Self::Lost(_) | Self::Within { focused: false, .. })
    }

    #[doc(hidden)]
    pub fn try_deliver<E>(
        self,
        mut notify: impl FnMut(FocusHandler, bool, FocusReason) -> Result<bool, E>,
    ) -> Result<bool, E> {
        let (handler, focused, reason) = match self {
            Self::Gained(reason) => (FocusHandler::Gained, true, reason),
            Self::Lost(reason) => (FocusHandler::Lost, false, reason),
            Self::Within { focused, reason } => {
                return notify(FocusHandler::Within, focused, reason);
            }
        };
        let handled = notify(handler, focused, reason)?;
        Ok(notify(FocusHandler::Changed, focused, reason)? || handled)
    }

    #[doc(hidden)]
    pub fn deliver(self, mut notify: impl FnMut(FocusHandler, bool, FocusReason) -> bool) -> bool {
        self.try_deliver(|handler, focused, reason| {
            Ok::<_, std::convert::Infallible>(notify(handler, focused, reason))
        })
        .unwrap_or_else(|error| match error {})
    }
}

pub struct FocusRequest {
    pub target: FocusHandle,
    pub action: FocusAction,
}

/// A window's commands, drained against the scene prepared after the callback.
#[derive(Clone, Default)]
pub struct FocusRequests(Rc<RefCell<Vec<FocusRequest>>>);

thread_local! {
    static ACTIVE: RefCell<Option<FocusRequests>> = const { RefCell::new(None) };
}

pub struct FocusContext(Option<FocusRequests>);

impl FocusRequests {
    pub fn enter(&self) -> FocusContext {
        FocusContext(ACTIVE.with(|active| active.replace(Some(self.clone()))))
    }

    pub fn drain(&self) -> Vec<FocusRequest> {
        std::mem::take(&mut *self.0.borrow_mut())
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

impl Drop for FocusContext {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.replace(self.0.take()));
    }
}

impl FocusHandle {
    #[doc(hidden)]
    pub fn with_text_selection() -> Self {
        Self::with_text_state(true, false)
    }

    #[doc(hidden)]
    pub fn with_text_state(selection: bool, commands: bool) -> Self {
        Self(Rc::new(FocusSignals {
            text: selection.then(Box::default),
            commands: commands.then(Box::default),
            ..FocusSignals::default()
        }))
    }

    #[doc(hidden)]
    pub fn observes_text_commands(&self) -> bool {
        self.0.commands.is_some()
    }

    #[doc(hidden)]
    pub fn publish_text_commands(&self, commands: TextCommandAvailability) {
        if let Some(signals) = &self.0.commands {
            signals.undo.set_if_changed(commands.undo);
            signals.redo.set_if_changed(commands.redo);
            signals.cut.set_if_changed(commands.cut);
            signals.copy.set_if_changed(commands.copy);
        }
    }

    #[doc(hidden)]
    pub fn clear_text_state(&self) {
        self.publish_text_selection(0, 0..0, "");
        self.publish_text_commands(TextCommandAvailability::default());
    }

    pub fn can_undo(&self) -> bool {
        self.0
            .commands
            .as_ref()
            .is_some_and(|signals| signals.undo.get(|| false))
    }

    pub fn can_redo(&self) -> bool {
        self.0
            .commands
            .as_ref()
            .is_some_and(|signals| signals.redo.get(|| false))
    }

    pub fn can_cut(&self) -> bool {
        self.0
            .commands
            .as_ref()
            .is_some_and(|signals| signals.cut.get(|| false))
    }

    pub fn can_copy(&self) -> bool {
        self.0
            .commands
            .as_ref()
            .is_some_and(|signals| signals.copy.get(|| false))
    }

    #[doc(hidden)]
    pub fn observes_text_selection(&self) -> bool {
        self.0.text.is_some()
    }

    #[doc(hidden)]
    pub fn publish_text_selection(&self, caret: usize, selection: Range<usize>, text: &str) {
        if let Some(signals) = &self.0.text {
            for (offset, value) in
                signals
                    .offsets
                    .iter()
                    .zip([caret, selection.start, selection.end])
            {
                offset.set_if_changed(value as i64);
            }
            if signals.selected.explicit_value().as_deref() != Some(text) {
                signals.selected.set(Rc::from(text));
            }
        }
    }

    fn text_offset(&self, index: usize) -> i64 {
        self.0
            .text
            .as_ref()
            .map_or(0, |signals| signals.offsets[index].get(|| 0))
    }

    pub fn cursor_offset(&self) -> i64 {
        self.text_offset(0)
    }

    pub fn selection_start_offset(&self) -> i64 {
        self.text_offset(1)
    }

    pub fn selection_end_offset(&self) -> i64 {
        self.text_offset(2)
    }

    pub fn selected_text(&self) -> String {
        self.0.text.as_ref().map_or_else(String::new, |signals| {
            signals.selected.get(|| Rc::from("")).to_string()
        })
    }

    pub fn has_focus(&self) -> bool {
        self.0.direct.get(|| false)
    }

    pub fn focus_within(&self) -> bool {
        self.0.within.get(|| false)
    }

    #[doc(hidden)]
    pub fn publish(&self, direct: bool, within: bool) {
        self.0.direct.set_if_changed(direct);
        self.0.within.set_if_changed(within);
    }

    pub fn request(&self, action: FocusAction) {
        ACTIVE.with(|active| {
            if let Some(requests) = &*active.borrow() {
                requests.0.borrow_mut().push(FocusRequest {
                    target: self.clone(),
                    action,
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_start_and_selected_text_track_independently() {
        let handle = FocusHandle::with_text_selection();
        let start = Property::default();
        let selected = Property::default();
        handle.publish_text_selection(3, 1..3, "bc");
        assert_eq!(start.get(|| handle.selection_start_offset()), 1);
        assert_eq!(selected.get(|| handle.selected_text()), "bc");

        handle.publish_text_selection(4, 1..4, "bcd");
        assert!(!start.is_dirty());
        assert!(selected.is_dirty());
        assert_eq!(selected.get(|| handle.selected_text()), "bcd");

        handle.publish_text_selection(1, 1..4, "bcd");
        assert!(!start.is_dirty());
        assert!(!selected.is_dirty());
    }

    #[test]
    fn direct_and_within_focus_have_independent_dependencies() {
        let handle = FocusHandle::default();
        let direct = Property::default();
        let within = Property::default();
        assert!(!direct.get(|| handle.has_focus()));
        assert!(!within.get(|| handle.focus_within()));
        handle.publish(false, true);
        assert!(!direct.is_dirty());
        assert!(within.is_dirty());
        assert!(within.get(|| handle.focus_within()));
        handle.publish(true, true);
        assert!(direct.is_dirty());
        assert!(!within.is_dirty());
    }
}
