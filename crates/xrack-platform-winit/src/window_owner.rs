use winit::window::{Window, WindowAttributes};

pub(crate) fn attributes(
    attributes: WindowAttributes,
    owner: Option<&Window>,
) -> Result<WindowAttributes, String> {
    #[cfg(target_os = "windows")]
    if let Some(owner) = owner {
        use winit::{
            platform::windows::WindowAttributesExtWindows,
            raw_window_handle::{HasWindowHandle, RawWindowHandle},
        };
        let handle = owner.window_handle().map_err(|error| error.to_string())?;
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            return Ok(attributes.with_owner_window(handle.hwnd.get()));
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = owner;
    Ok(attributes)
}

#[cfg(target_os = "macos")]
pub(crate) use macos::{WindowOwner, native_window};

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSView, NSWindow, NSWindowOrderingMode};
    use winit::{
        raw_window_handle::{HasWindowHandle, RawWindowHandle},
        window::Window,
    };

    pub(crate) struct WindowOwner(Option<(Retained<NSWindow>, Retained<NSWindow>)>);

    impl WindowOwner {
        pub(crate) fn new(window: &Window, owner: Option<&Window>) -> Result<Self, String> {
            Ok(Self(
                owner
                    .map(|owner| Ok::<_, String>((native_window(owner)?, native_window(window)?)))
                    .transpose()?,
            ))
        }

        pub(crate) fn attach(&self) {
            if let Some((owner, child)) = &self.0
                && child.parentWindow().is_none()
            {
                // The logical ownership tree guarantees a distinct, acyclic pair.
                unsafe { owner.addChildWindow_ordered(child, NSWindowOrderingMode::Above) };
            }
        }

        pub(crate) fn detach(&self) {
            if let Some((owner, child)) = &self.0
                && child.parentWindow().is_some()
            {
                owner.removeChildWindow(child);
            }
        }
    }

    impl Drop for WindowOwner {
        fn drop(&mut self) {
            self.detach();
        }
    }

    pub(crate) fn native_window(window: &Window) -> Result<Retained<NSWindow>, String> {
        let handle = window.window_handle().map_err(|error| error.to_string())?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return Err("expected an AppKit window handle".into());
        };
        // Winit supplies a live NSView for this borrowed handle on the UI thread.
        let view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        view.window()
            .ok_or_else(|| "the AppKit view has no window".into())
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) struct WindowOwner;

#[cfg(not(target_os = "macos"))]
impl WindowOwner {
    pub(crate) fn new(_: &Window, _: Option<&Window>) -> Result<Self, String> {
        Ok(Self)
    }
    pub(crate) fn attach(&self) {}
    pub(crate) fn detach(&self) {}
}
