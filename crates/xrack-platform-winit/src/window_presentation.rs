use std::time::{Duration, Instant};
use winit::window::{Fullscreen, Window};
use xrack_runtime::{WindowCommand, WindowStatus};

const FULL_SCREEN_SETTLE: Duration = Duration::from_secs(4);

#[derive(Default)]
pub(crate) struct WindowPresentation {
    observed: WindowStatus,
    full_screen_request: Option<(bool, Instant)>,
    maximize_after_exit: Option<bool>,
    #[cfg(target_os = "macos")]
    zoom_sample: Option<(
        objc2_foundation::NSRect,
        objc2_app_kit::NSWindowStyleMask,
        bool,
    )>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Actions {
    minimized: Option<bool>,
    maximized: Option<bool>,
    full_screen: Option<bool>,
}

impl Actions {
    fn apply(self, window: &Window) {
        if self.minimized == Some(false) {
            window.set_minimized(false);
        }
        if self.full_screen == Some(false) {
            window.set_fullscreen(None);
        }
        if let Some(maximized) = self.maximized {
            window.set_maximized(maximized);
        }
        if self.full_screen == Some(true) {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        if self.minimized == Some(true) {
            window.set_minimized(true);
        }
    }
}

impl WindowPresentation {
    pub fn observed(&self) -> WindowStatus {
        self.observed
    }

    pub fn sample(&mut self, window: &Window) -> WindowStatus {
        #[cfg(target_os = "macos")]
        let (full_screen, maximized) = self.sample_macos_modes(window);
        #[cfg(not(target_os = "macos"))]
        let (full_screen, maximized) = {
            let full_screen = window.fullscreen().is_some();
            (full_screen, !full_screen && window.is_maximized())
        };
        self.observed = WindowStatus {
            active: window.has_focus(),
            minimized: window.is_minimized(),
            maximized,
            full_screen,
        };
        self.observed
    }

    #[cfg(target_os = "macos")]
    fn sample_macos_modes(&mut self, window: &Window) -> (bool, bool) {
        use objc2_app_kit::NSWindowStyleMask;
        let Ok(native) = crate::window_owner::native_window(window) else {
            let full_screen = window.fullscreen().is_some();
            return (full_screen, !full_screen && window.is_maximized());
        };
        let style = native.styleMask();
        if style.contains(NSWindowStyleMask::FullScreen) {
            self.zoom_sample = None;
            return (true, false);
        }
        if native.inLiveResize() {
            return (false, self.observed.maximized);
        }
        let frame = native.frame();
        if let Some((previous_frame, previous_style, zoomed)) = self.zoom_sample
            && frame == previous_frame
            && style == previous_style
        {
            return (false, zoomed);
        }
        // On a frameless window winit temporarily changes AppKit's style mask
        // to query zoom. Avoid repeating that side effect on idle polls or mouse moves.
        let zoomed = window.is_maximized();
        self.zoom_sample = Some((native.frame(), native.styleMask(), zoomed));
        (false, zoomed)
    }

    pub fn apply(
        &mut self,
        window: &Window,
        commands: Vec<WindowCommand>,
        now: Instant,
    ) -> WindowStatus {
        let status = self.sample(window);
        let actions = self.plan(status, window.fullscreen().is_some(), commands, now);
        if actions == Actions::default() {
            return status;
        }
        actions.apply(window);
        #[cfg(target_os = "macos")]
        {
            self.zoom_sample = None;
        }
        self.sample(window)
    }

    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        self.full_screen_request
            .map(|(_, deadline)| deadline.min(now + Duration::from_millis(80)))
    }

    pub fn is_switching_full_screen(&self) -> bool {
        self.full_screen_request.is_some()
    }

    fn plan(
        &mut self,
        status: WindowStatus,
        native_full_screen: bool,
        commands: impl IntoIterator<Item = WindowCommand>,
        now: Instant,
    ) -> Actions {
        if let Some((target, deadline)) = self.full_screen_request {
            if target == status.full_screen && target == native_full_screen {
                self.full_screen_request = None;
            } else if now >= deadline {
                self.full_screen_request = None;
                self.maximize_after_exit = None;
            }
        }
        let previous = self
            .full_screen_request
            .map_or(native_full_screen, |(target, _)| target);
        let mut actions = Actions {
            maximized: self.maximize_after_exit.take(),
            ..Actions::default()
        };
        for command in commands {
            use WindowCommand::*;
            match command {
                Minimize => actions.minimized = Some(true),
                Maximize | Restore | ToggleMaximized => {
                    let maximize = match command {
                        Maximize => true,
                        Restore => false,
                        _ => {
                            actions.full_screen.unwrap_or(previous)
                                || !actions.maximized.unwrap_or(status.maximized)
                        }
                    };
                    actions.minimized = Some(false);
                    actions.full_screen = Some(false);
                    actions.maximized = Some(maximize);
                }
                SetFullScreen(_) | ToggleFullScreen => {
                    let enable = match command {
                        SetFullScreen(enable) => enable,
                        _ => !actions.full_screen.unwrap_or(previous),
                    };
                    actions.full_screen = Some(enable);
                    if enable {
                        actions.minimized = Some(false);
                        actions.maximized = None;
                    }
                }
            }
        }
        let target = actions.full_screen.unwrap_or(previous);
        actions.full_screen = actions.full_screen.filter(|&enabled| {
            enabled != previous
                || self.full_screen_request.is_none() && enabled != status.full_screen
        });
        if let Some(enabled) = actions.full_screen {
            self.full_screen_request = Some((enabled, now + FULL_SCREEN_SETTLE));
        }
        if target || status.full_screen || native_full_screen {
            self.maximize_after_exit = actions.maximized.take();
        }
        actions.maximized = actions.maximized.filter(|&value| value != status.maximized);
        actions.minimized = actions
            .minimized
            .filter(|&value| Some(value) != status.minimized);
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_full_screen_toggles_follow_requested_state_until_the_host_catches_up() {
        let mut presentation = WindowPresentation::default();
        let now = Instant::now();
        let status = WindowStatus {
            minimized: Some(false),
            ..WindowStatus::default()
        };
        let toggle = [WindowCommand::ToggleFullScreen];
        assert_eq!(
            presentation.plan(status, false, [toggle[0]; 2], now),
            Actions::default()
        );
        assert_eq!(
            presentation.plan(status, false, toggle, now).full_screen,
            Some(true)
        );
        assert_eq!(
            presentation.plan(status, true, toggle, now).full_screen,
            Some(false)
        );
        assert_eq!(
            presentation.plan(status, true, toggle, now).full_screen,
            Some(true)
        );
        let entered = WindowStatus {
            full_screen: true,
            ..status
        };
        assert_eq!(
            presentation.plan(entered, true, [], now),
            Actions::default()
        );
        assert!(presentation.deadline(now).is_none());
    }

    #[test]
    fn restoring_from_full_screen_defers_zoom_until_exit_is_observed() {
        let mut presentation = WindowPresentation::default();
        let now = Instant::now();
        let status = WindowStatus {
            full_screen: true,
            minimized: Some(false),
            ..WindowStatus::default()
        };
        let leave = presentation.plan(status, true, [WindowCommand::Restore], now);
        assert_eq!(
            leave,
            Actions {
                full_screen: Some(false),
                ..Actions::default()
            }
        );
        assert_eq!(
            presentation.plan(status, false, [], now),
            Actions::default()
        );
        let exited_zoomed = WindowStatus {
            full_screen: false,
            maximized: true,
            ..status
        };
        assert_eq!(
            presentation.plan(exited_zoomed, false, [], now),
            Actions {
                maximized: Some(false),
                ..Actions::default()
            }
        );
        assert!(presentation.deadline(now).is_none());
    }

    #[test]
    fn a_failed_full_screen_exit_does_not_leave_a_late_zoom_request() {
        let mut presentation = WindowPresentation::default();
        let now = Instant::now();
        let status = WindowStatus {
            full_screen: true,
            ..WindowStatus::default()
        };
        assert_eq!(
            presentation
                .plan(status, true, [WindowCommand::Maximize], now)
                .full_screen,
            Some(false)
        );
        assert_eq!(
            presentation.plan(status, true, [], now + FULL_SCREEN_SETTLE),
            Actions::default()
        );
        assert!(presentation.deadline(now + FULL_SCREEN_SETTLE).is_none());
        assert_eq!(
            presentation.plan(WindowStatus::default(), false, [], now + FULL_SCREEN_SETTLE),
            Actions::default()
        );
    }
}
