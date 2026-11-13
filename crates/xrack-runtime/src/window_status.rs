use crate::Property;
use std::cell::{Cell, RefCell};

/// Native state reported by the host. Some window managers do not report minimization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowStatus {
    pub active: bool,
    pub minimized: Option<bool>,
    pub maximized: bool,
    pub full_screen: bool,
}

/// An ordered request from a component to the host of its native window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    Minimize,
    Maximize,
    Restore,
    ToggleMaximized,
    SetFullScreen(bool),
    ToggleFullScreen,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommandMethod {
    Minimize,
    Maximize,
    Restore,
    ToggleMaximized,
    SetFullScreen,
    ToggleFullScreen,
}

impl WindowCommandMethod {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "minimize" => Self::Minimize,
            "maximize" => Self::Maximize,
            "restore" => Self::Restore,
            "toggle-maximized" => Self::ToggleMaximized,
            "set-full-screen" => Self::SetFullScreen,
            "toggle-full-screen" => Self::ToggleFullScreen,
            _ => return None,
        })
    }

    pub fn action(self, enabled: Option<bool>) -> Option<WindowCommand> {
        Some(match (self, enabled) {
            (Self::Minimize, None) => WindowCommand::Minimize,
            (Self::Maximize, None) => WindowCommand::Maximize,
            (Self::Restore, None) => WindowCommand::Restore,
            (Self::ToggleMaximized, None) => WindowCommand::ToggleMaximized,
            (Self::SetFullScreen, Some(enabled)) => WindowCommand::SetFullScreen(enabled),
            (Self::ToggleFullScreen, None) => WindowCommand::ToggleFullScreen,
            _ => return None,
        })
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum WindowMetric {
    Active,
    Minimized,
    MinimizedKnown,
    Maximized,
    FullScreen,
}

impl WindowMetric {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "window-active" => Self::Active,
            "minimized" => Self::Minimized,
            "minimized-known" => Self::MinimizedKnown,
            "maximized" => Self::Maximized,
            "full-screen" => Self::FullScreen,
            _ => return None,
        })
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct WindowStatusBinding([Property<bool>; 5]);

impl WindowStatusBinding {
    pub fn get(&self, metric: WindowMetric) -> bool {
        self.0[metric as usize].get(|| false)
    }

    pub fn publish(&self, status: WindowStatus) {
        for (property, value) in self.0.iter().zip([
            status.active,
            status.minimized.unwrap_or(false),
            status.minimized.is_some(),
            status.maximized,
            status.full_screen,
        ]) {
            property.set_if_changed(value);
        }
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct WindowLifecycle {
    status: WindowStatusBinding,
    close: Cell<bool>,
    commands: RefCell<Vec<WindowCommand>>,
}

impl WindowLifecycle {
    pub fn get(&self, metric: WindowMetric) -> bool {
        self.status.get(metric)
    }

    pub fn publish(&self, status: WindowStatus) {
        self.status.publish(status);
    }

    pub fn request_close(&self) {
        self.close.set(true);
    }

    pub fn take_close_request(&self) -> bool {
        self.close.replace(false)
    }

    pub fn request(&self, command: WindowCommand) {
        self.commands.borrow_mut().push(command);
    }

    pub fn take_commands(&self) -> Vec<WindowCommand> {
        self.commands.take()
    }

    pub fn has_commands(&self) -> bool {
        !self.commands.borrow().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_minimization_status_does_not_invalidate_unchanged_flags() {
        let status = WindowStatusBinding::default();
        let minimized = Property::default();
        let known = Property::default();
        let read_minimized = || minimized.get(|| status.get(WindowMetric::Minimized));
        let read_known = || known.get(|| status.get(WindowMetric::MinimizedKnown));
        assert!(!read_minimized());
        assert!(!read_known());
        status.publish(WindowStatus {
            active: true,
            minimized: Some(false),
            ..WindowStatus::default()
        });
        assert!(!minimized.is_dirty());
        assert!(known.is_dirty());
        assert!(read_known());
        status.publish(WindowStatus {
            minimized: Some(true),
            ..WindowStatus::default()
        });
        assert!(!known.is_dirty());
        assert!(read_minimized());
    }
}
