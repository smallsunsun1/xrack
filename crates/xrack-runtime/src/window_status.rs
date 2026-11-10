use crate::Property;

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
#[derive(Clone, Copy)]
pub enum WindowMetric {
    Active,
    Minimized,
    MinimizedKnown,
    Maximized,
    FullScreen,
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
