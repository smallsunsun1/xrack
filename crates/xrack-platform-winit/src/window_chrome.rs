use winit::{
    dpi::LogicalSize,
    window::{ResizeDirection, Window, WindowAttributes},
};
use xrack_layout::WindowResizeEdge;
use xrack_runtime::Node;

const MAX_WINDOW_EXTENT: f64 = 16384.0;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct WindowChrome {
    pub decorated: bool,
    pub resizable: bool,
    min: Option<LogicalSize<f64>>,
    max: Option<LogicalSize<f64>>,
}

fn window_length(scene: &Node, name: &str) -> Option<f64> {
    scene
        .length(name)
        .filter(|value| value.is_finite())
        .map(|value| f64::from(value).clamp(1.0, MAX_WINDOW_EXTENT))
}

impl WindowChrome {
    pub fn read(scene: &Node) -> Self {
        if scene.kind != "Window" {
            return Self {
                decorated: true,
                resizable: true,
                min: None,
                max: None,
            };
        }
        let min_width = window_length(scene, "min-width");
        let min_height = window_length(scene, "min-height");
        let max_width = window_length(scene, "max-width");
        let max_height = window_length(scene, "max-height");
        let min = LogicalSize::new(min_width.unwrap_or(1.0), min_height.unwrap_or(1.0));
        let max = LogicalSize::new(
            max_width.unwrap_or(MAX_WINDOW_EXTENT).max(min.width),
            max_height.unwrap_or(MAX_WINDOW_EXTENT).max(min.height),
        );
        Self {
            decorated: !scene.boolean("no-frame", false),
            resizable: scene.boolean("resizable", true),
            min: (min_width.is_some() || min_height.is_some()).then_some(min),
            max: (max_width.is_some() || max_height.is_some()).then_some(max),
        }
    }

    pub fn constrain(self, size: LogicalSize<f64>) -> LogicalSize<f64> {
        let min = self.min.unwrap_or(LogicalSize::new(1.0, 1.0));
        let max = self
            .max
            .unwrap_or(LogicalSize::new(MAX_WINDOW_EXTENT, MAX_WINDOW_EXTENT));
        LogicalSize::new(
            size.width.clamp(min.width, max.width),
            size.height.clamp(min.height, max.height),
        )
    }

    pub fn attributes(self, attrs: WindowAttributes, scene: &Node) -> WindowAttributes {
        let initial = |size, preferred, fallback| {
            window_length(scene, size)
                .or_else(|| window_length(scene, preferred))
                .unwrap_or(fallback)
        };
        let mut attrs = attrs
            .with_decorations(self.decorated)
            .with_resizable(self.resizable)
            .with_inner_size(self.constrain(LogicalSize::new(
                initial("width", "preferred-width", 640.0),
                initial("height", "preferred-height", 480.0),
            )));
        if let Some(size) = self.min {
            attrs = attrs.with_min_inner_size(size);
        }
        if let Some(size) = self.max {
            attrs = attrs.with_max_inner_size(size);
        }
        attrs
    }

    pub fn apply(self, previous: Self, window: &Window) {
        if self.decorated != previous.decorated {
            window.set_decorations(self.decorated);
        }
        // Both constraints can cross on different axes. Clear the old upper
        // bound before publishing the new lower bound in that case.
        if self.min != previous.min && self.max != previous.max {
            window.set_max_inner_size(None::<LogicalSize<f64>>);
        }
        if self.min != previous.min {
            window.set_min_inner_size(self.min);
        }
        if self.max != previous.max {
            window.set_max_inner_size(self.max);
        }
        if self.resizable != previous.resizable {
            window.set_resizable(self.resizable);
        }
    }
}

pub(crate) fn resize_direction(edge: WindowResizeEdge) -> ResizeDirection {
    match edge {
        WindowResizeEdge::North => ResizeDirection::North,
        WindowResizeEdge::South => ResizeDirection::South,
        WindowResizeEdge::East => ResizeDirection::East,
        WindowResizeEdge::West => ResizeDirection::West,
        WindowResizeEdge::NorthEast => ResizeDirection::NorthEast,
        WindowResizeEdge::NorthWest => ResizeDirection::NorthWest,
        WindowResizeEdge::SouthEast => ResizeDirection::SouthEast,
        WindowResizeEdge::SouthWest => ResizeDirection::SouthWest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::Size;
    use xrack_runtime::Length;

    #[test]
    fn window_chrome_applies_preferred_size_and_normalizes_inverted_limits() {
        let mut scene = Node::new("Window", None);
        scene.properties.insert("no-frame", true.into());
        scene.properties.insert("resizable", false.into());
        scene
            .properties
            .insert("preferred-width", Length(600.0).into());
        scene.properties.insert("min-width", Length(360.0).into());
        scene.properties.insert("max-width", Length(280.0).into());
        let chrome = WindowChrome::read(&scene);
        let attrs = chrome.attributes(Window::default_attributes(), &scene);
        assert!(!attrs.decorations);
        assert!(!attrs.resizable);
        assert_eq!(
            attrs.inner_size,
            Some(Size::Logical(LogicalSize::new(360.0, 480.0)))
        );
        assert_eq!(
            attrs.min_inner_size,
            Some(Size::Logical(LogicalSize::new(360.0, 1.0)))
        );
        assert_eq!(
            attrs.max_inner_size,
            Some(Size::Logical(LogicalSize::new(360.0, MAX_WINDOW_EXTENT)))
        );
    }
}

#[cfg(target_os = "macos")]
pub(crate) use macos::ResizeDrag;

#[cfg(target_os = "macos")]
mod macos {
    use super::{MAX_WINDOW_EXTENT, WindowResizeEdge};
    use crate::window_owner::native_window;
    use objc2::rc::Retained;
    use objc2_app_kit::NSWindow;
    use objc2_foundation::{NSPoint, NSRect};
    use winit::window::Window;

    pub(crate) struct ResizeDrag {
        window: Retained<NSWindow>,
        frame: NSRect,
        pointer: NSPoint,
        edge: WindowResizeEdge,
    }

    impl ResizeDrag {
        pub fn start(window: &Window, edge: WindowResizeEdge) -> Option<Self> {
            let window = native_window(window).ok()?;
            Some(Self {
                frame: window.frame(),
                pointer: screen_pointer(&window),
                window,
                edge,
            })
        }

        pub fn edge(&self) -> WindowResizeEdge {
            self.edge
        }

        pub fn update(&self) {
            use WindowResizeEdge::*;
            let pointer = screen_pointer(&self.window);
            let min = self.window.minSize();
            let max = self.window.maxSize();
            let mut frame = self.frame;
            let horizontal = match self.edge {
                West | NorthWest | SouthWest => -1,
                East | NorthEast | SouthEast => 1,
                _ => 0,
            };
            let vertical = match self.edge {
                South | SouthWest | SouthEast => -1,
                North | NorthWest | NorthEast => 1,
                _ => 0,
            };
            (frame.origin.x, frame.size.width) = resize_axis(
                frame.origin.x,
                frame.size.width,
                pointer.x - self.pointer.x,
                min.width,
                max.width,
                horizontal,
            );
            (frame.origin.y, frame.size.height) = resize_axis(
                frame.origin.y,
                frame.size.height,
                pointer.y - self.pointer.y,
                min.height,
                max.height,
                vertical,
            );
            if frame != self.window.frame() {
                self.window.setFrame_display(frame, true);
            }
        }
    }

    fn screen_pointer(window: &NSWindow) -> NSPoint {
        window.convertPointToScreen(window.mouseLocationOutsideOfEventStream())
    }

    fn resize_axis(
        origin: f64,
        extent: f64,
        delta: f64,
        min: f64,
        max: f64,
        direction: i8,
    ) -> (f64, f64) {
        if direction == 0 {
            return (origin, extent);
        }
        let min = min.clamp(1.0, MAX_WINDOW_EXTENT);
        let max = max.min(MAX_WINDOW_EXTENT).max(min);
        let size = (extent + delta * f64::from(direction)).clamp(min, max);
        if direction < 0 {
            (origin + extent - size, size)
        } else {
            (origin, size)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::resize_axis;

        #[test]
        fn resizing_clamps_the_dragged_edge_and_keeps_the_opposite_edge_fixed() {
            assert_eq!(
                resize_axis(40.0, 200.0, 180.0, 120.0, 400.0, -1),
                (120.0, 120.0)
            );
            assert_eq!(
                resize_axis(40.0, 200.0, 300.0, 120.0, 400.0, 1),
                (40.0, 400.0)
            );
            assert_eq!(
                resize_axis(40.0, 200.0, -36.0, 120.0, 400.0, -1),
                (4.0, 236.0)
            );
            assert_eq!(
                resize_axis(40.0, 200.0, 180.0, 120.0, 400.0, 0),
                (40.0, 200.0)
            );
        }
    }
}
