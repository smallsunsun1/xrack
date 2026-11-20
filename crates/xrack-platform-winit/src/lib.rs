//! Native desktop host. The event loop sleeps until input, resize, or OS redraw.
mod accessibility;
mod accessibility_text;
#[cfg(target_os = "android")]
mod android_accessibility;
#[cfg(target_os = "android")]
mod android_directory;
#[cfg(target_os = "android")]
mod android_emoji;
#[cfg(target_os = "android")]
mod android_host;
#[cfg(any(target_os = "android", test))]
mod android_keyboard;
#[cfg(target_os = "android")]
mod android_text;
#[cfg(any(target_os = "android", test))]
mod android_text_traversal;
mod clipboard;
mod desktop;
#[cfg(target_os = "macos")]
mod file_drop;
mod frame;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
mod ios_accessibility_actions;
#[cfg(target_os = "ios")]
mod ios_back;
#[cfg(target_os = "ios")]
mod ios_directory;
#[cfg(target_os = "ios")]
mod ios_edit_menu;
#[cfg(target_os = "ios")]
mod ios_file_drop;
#[cfg(target_os = "ios")]
use ios_file_drop as file_drop;
#[cfg(target_os = "ios")]
mod ios_keyboard;
#[cfg(target_os = "ios")]
mod ios_pointer;
#[cfg(target_os = "ios")]
mod ios_text;
mod keyboard;
mod local_task;
mod native_dialog;
mod queue;
mod scale_rotate;
mod system_tray;
#[cfg(target_os = "linux")]
mod system_tray_linux;
#[cfg(target_os = "macos")]
mod system_tray_macos;
#[cfg(target_os = "windows")]
mod system_tray_windows;
#[cfg(test)]
mod tests;
#[cfg(any(target_os = "android", target_os = "ios", test))]
mod text_indices;
#[cfg(any(not(target_os = "android"), test))]
mod window_chrome;
mod window_owner;
mod window_presentation;

use accessibility::{Projection, Route};
use accesskit::{Action as AccessibleAction, ActionData, ActionRequest, TreeId};
#[cfg(target_os = "android")]
pub use android_directory::{AndroidDirectory, AndroidDocument};
#[cfg(target_os = "android")]
use android_host::{ActiveEventLoop, AndroidEvent, DeviceId, Touch, Window};
use clipboard::{ClipboardPort, ClipboardValue, Reply as ClipboardReply};
pub use desktop::{Desktop, DesktopTray, DesktopWindow, run_desktop};
use desktop::{DesktopGraphics, HostedWindow, WindowEvents};
use frame::FrameCache;
#[cfg(target_os = "ios")]
pub use ios_directory::IosDirectory;
use keyboard::KeyEvent;
use local_task::LocalExecutor;
pub use local_task::{LocalTask, SpawnLocalError, TaskCancelled, spawn_local};
#[cfg(target_os = "ios")]
pub use native_dialog::{IosPhotoSelection, IosPhotoThumbnail};
#[cfg(not(target_os = "android"))]
use std::cell::Cell;
use std::{
    borrow::Cow,
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(not(target_os = "android"))]
use window_chrome::WindowChrome;
#[cfg(any(not(target_os = "android"), test))]
use window_chrome::resize_direction;
use window_owner::WindowOwner;
use window_presentation::WindowPresentation;
#[cfg(not(target_os = "android"))]
use winit::dpi::{LogicalPosition, LogicalSize};
#[cfg(any(not(target_os = "android"), test))]
use winit::window::CursorIcon;
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Theme as NativeTheme, WindowId},
};
#[cfg(not(target_os = "android"))]
use winit::{
    event::{DeviceId, Touch},
    event_loop::ActiveEventLoop,
    window::{ImePurpose, Window},
};
use xrack_layout::{
    ActivationKey, ButtonClick, ChoiceKey, ControlInput, DragUpdate, EditAction, EditSource,
    FocusObservers, HoverObservers, Interaction, LayoutNode, MenuKey, MenuShortcut, NavigationKey,
    Point, PointerDelivery, PointerSequence, RangeInput, ScrollBarUpdate, ScrollGestureKind,
    ShortcutKey, ShortcutModifiers, ShortcutOutcome, Size, SplitResize, StackTransitions,
    TableEditCommit, TableEditTarget, TablePasteDestination, Target, TextAreaMotion, TouchTooltip,
    TumblerScroll, ViewportObservers, WheelAdjustment, WindowEffects,
};
#[cfg(any(not(target_os = "android"), test))]
use xrack_layout::{ComboGeometry, SpinGeometry};
use xrack_renderer_femtovg::{PointerCaret, Renderer};
#[cfg(any(not(target_os = "android"), test))]
use xrack_runtime::CursorShape;
use xrack_runtime::{
    ColorScheme, Component, ControlAction, DragAction, EventResult, FocusRequests,
    ImagePlaybackQueue, KeyPhase, NativeDialogQueue, NativeDialogResult, Node, Orientation,
    PointerButton, PointerEvent, PointerEventKind, PointerHandle, RangeAction, ReturnKey, RowKey,
    RowSelection, SceneTransition, ScrollRequests, SecondaryWindowRegistration, SpinAction,
    SpinDecimal, TablePaste, TextCommand, TimerQueue, combo_editable, decimal_separator,
    is_calendar_control, is_multiline_text_input, is_spin_box, spin_editable,
};
pub use xrack_runtime::{EventLoopError, invoke_from_event_loop};
#[cfg(target_os = "android")]
pub use {android_activity::AndroidApp, desktop::run_android};

#[cfg(feature = "media")]
pub use xrack_media::install as install_media;
#[cfg(feature = "media")]
pub use xrack_media::voice;

#[cfg(target_os = "ios")]
type WindowAccessibility = ios::Accessibility;
#[cfg(target_os = "android")]
type WindowAccessibility = android_accessibility::Accessibility;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
type WindowAccessibility = accesskit_winit::Adapter;

#[derive(PartialEq, Eq)]
enum CaretFollow {
    None,
    Editor,
    Viewports,
}

struct WindowState {
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    config: wgpu::SurfaceConfiguration,
    accessibility: WindowAccessibility,
    projection: Projection,
    accessibility_active: bool,
    #[cfg(not(target_os = "android"))]
    ime_purpose: ImePurpose,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    file_drop: Option<file_drop::FileDropBridge>,
    #[cfg(target_os = "macos")]
    resize_drag: Option<window_chrome::ResizeDrag>,
    owner: WindowOwner,
    window: Arc<Window>,
    #[cfg(target_os = "android")]
    android_app: AndroidApp,
    #[cfg(target_os = "android")]
    android_text: Option<android_text::TextInput>,
    #[cfg(target_os = "android")]
    android_spin_step_focus: Option<Target>,
    #[cfg(target_os = "android")]
    android_ime_top: Option<u32>,
    #[cfg(not(target_os = "android"))]
    applied_cursor: Cell<Option<CursorIcon>>,
    frame: FrameCache,
    presentation: WindowPresentation,
    inspection_revision: Option<u64>,
    #[cfg(any(target_os = "ios", target_os = "android"))]
    safe_area: [f64; 4],
    #[cfg(target_os = "ios")]
    ios_layout_direction: xrack_runtime::LayoutDirection,
    #[cfg(target_os = "ios")]
    ios_modal_queue: native_dialog::ModalQueue,
    #[cfg(target_os = "ios")]
    ios_imported_folders: Vec<tempfile::TempDir>,
    #[cfg(target_os = "ios")]
    ios_text: Option<ios_text::TextInput>,
    #[cfg(target_os = "ios")]
    ios_edit_menu: Option<ios_edit_menu::EditMenu>,
    #[cfg(target_os = "ios")]
    ios_keyboard_occlusion: ios::KeyboardOcclusion,
    #[cfg(target_os = "ios")]
    ios_keyboard: Option<ios_keyboard::KeyboardInput>,
    #[cfg(target_os = "ios")]
    ios_pointer: Option<ios_pointer::PointerInput>,
    #[cfg(target_os = "ios")]
    _ios_accessibility_observer: Option<ios::AccessibilityObserver>,
    #[cfg(target_os = "ios")]
    _ios_scroll_to_top: Option<ios::ScrollToTop>,
    #[cfg(target_os = "ios")]
    ios_edge_back: Option<ios_back::EdgeBack>,
    #[cfg(target_os = "ios")]
    ios_spin_step_focus: Option<Target>,
    #[cfg(target_os = "ios")]
    ios_keyboard_dismissed_focus: Option<Target>,
    #[cfg(target_os = "ios")]
    ios_scroll_announcement: Option<ios::ScrollAnnouncement>,
}

fn activation_key_value(key: &Key) -> Option<ActivationKey> {
    match key {
        Key::Named(NamedKey::Enter) => Some(ActivationKey::Enter),
        Key::Named(NamedKey::Space) => Some(ActivationKey::Space),
        Key::Character(text) if text == " " => Some(ActivationKey::Space),
        _ => None,
    }
}

#[cfg(any(not(target_os = "android"), test))]
fn native_cursor_shape(shape: CursorShape) -> Option<CursorIcon> {
    Some(match shape {
        CursorShape::Auto => return None,
        CursorShape::Default => CursorIcon::Default,
        CursorShape::Pointer => CursorIcon::Pointer,
        CursorShape::Text => CursorIcon::Text,
        CursorShape::Crosshair => CursorIcon::Crosshair,
        CursorShape::Move => CursorIcon::Move,
        CursorShape::Grab => CursorIcon::Grab,
        CursorShape::Grabbing => CursorIcon::Grabbing,
        CursorShape::NotAllowed => CursorIcon::NotAllowed,
        CursorShape::Wait => CursorIcon::Wait,
        CursorShape::Help => CursorIcon::Help,
        CursorShape::ResizeHorizontal => CursorIcon::ColResize,
        CursorShape::ResizeVertical => CursorIcon::RowResize,
        CursorShape::ZoomIn => CursorIcon::ZoomIn,
        CursorShape::ZoomOut => CursorIcon::ZoomOut,
    })
}

fn navigation_key_value(key: &Key) -> Option<NavigationKey> {
    Some(match key {
        Key::Named(NamedKey::ArrowLeft) => NavigationKey::Left,
        Key::Named(NamedKey::ArrowRight) => NavigationKey::Right,
        Key::Named(NamedKey::ArrowUp) => NavigationKey::Up,
        Key::Named(NamedKey::ArrowDown) => NavigationKey::Down,
        Key::Named(NamedKey::Home) => NavigationKey::Home,
        Key::Named(NamedKey::End) => NavigationKey::End,
        Key::Named(NamedKey::PageUp) => NavigationKey::PageUp,
        Key::Named(NamedKey::PageDown) => NavigationKey::PageDown,
        _ => return None,
    })
}

fn color_scheme(theme: Option<NativeTheme>) -> ColorScheme {
    match theme {
        Some(NativeTheme::Light) => ColorScheme::Light,
        Some(NativeTheme::Dark) => ColorScheme::Dark,
        None => ColorScheme::System,
    }
}

#[cfg(target_os = "android")]
fn android_color_scheme(app: &AndroidApp) -> Option<ColorScheme> {
    use jni::{jni_sig, jni_str};

    const UI_MODE_NIGHT_MASK: i32 = 0x30;
    const UI_MODE_NIGHT_NO: i32 = 0x10;
    const UI_MODE_NIGHT_YES: i32 = 0x20;

    // Use the Activity's resource configuration. The NDK asset configuration can
    // remain unchanged on a live night-mode switch.
    let mode = android_directory::with_activity(app, |env, activity| {
        let resources = env
            .call_method(
                activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let configuration = env
            .call_method(
                resources,
                jni_str!("getConfiguration"),
                jni_sig!("()Landroid/content/res/Configuration;"),
                &[],
            )?
            .l()?;
        env.get_field(configuration, jni_str!("uiMode"), jni_sig!("I"))?
            .i()
    })
    .ok()?;
    match mode & UI_MODE_NIGHT_MASK {
        UI_MODE_NIGHT_NO => Some(ColorScheme::Light),
        UI_MODE_NIGHT_YES => Some(ColorScheme::Dark),
        _ => None,
    }
}

fn drawable_size(window: &Window) -> winit::dpi::PhysicalSize<u32> {
    #[cfg(target_os = "ios")]
    return window.outer_size();
    #[cfg(not(target_os = "ios"))]
    return window.inner_size();
}

#[cfg(target_os = "android")]
fn android_safe_area(window: &Window, ime_top: Option<u32>) -> [f64; 4] {
    let content = window.content_rect();
    let size = drawable_size(window);
    let mut insets = if content.right > content.left && content.bottom > content.top {
        [
            f64::from(content.left),
            f64::from(content.top),
            f64::from(size.width) - f64::from(content.right),
            f64::from(size.height) - f64::from(content.bottom),
        ]
    } else {
        [0.0; 4]
    };
    if let Some(top) = ime_top {
        insets[3] = insets[3].max(f64::from(size.height.saturating_sub(top)));
    }
    insets.map(|pixels| pixels.max(0.0) / window.scale_factor())
}

#[cfg(any(target_os = "ios", target_os = "android"))]
fn inset_content(mut scene: Rc<Node>, insets: [f64; 4]) -> Rc<Node> {
    if insets != [0.0; 4] {
        xrack_layout::set_window_safe_area(Rc::make_mut(&mut scene), insets);
    }
    scene
}

impl WindowState {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    fn content_insets(&self) -> [f64; 4] {
        #[cfg(target_os = "ios")]
        return ios::content_geometry(&self.window, self.ios_text.as_ref()).0;
        #[cfg(target_os = "android")]
        return android_safe_area(&self.window, self.android_ime_top);
    }

    fn new(
        window: Arc<Window>,
        accessibility: WindowAccessibility,
        graphics: &mut DesktopGraphics,
        owner: Option<&Window>,
        #[cfg(target_os = "android")] android_app: AndroidApp,
    ) -> Result<Self, String> {
        let surface = graphics
            .instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let mut renderer = graphics.renderer(&surface)?;
        #[cfg(target_os = "ios")]
        let appearance = ios::system_appearance(&window);
        #[cfg(target_os = "ios")]
        {
            renderer.set_system_color_scheme(appearance.scheme);
            renderer.set_system_increased_contrast(appearance.increased_contrast);
        }
        #[cfg(target_os = "android")]
        {
            if let Some(scheme) = android_color_scheme(&android_app) {
                renderer.set_system_color_scheme(scheme);
            }
            android_emoji::install(&android_app, &mut renderer.text);
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        renderer.set_system_color_scheme(color_scheme(window.theme()));
        let size = drawable_size(&window);
        let mut config = surface
            .get_default_config(&renderer.adapter, size.width.max(1), size.height.max(1))
            .ok_or("no supported surface configuration")?;
        let caps = surface.get_capabilities(&renderer.adapter);
        // FemtoVG produces display-encoded colors; prefer a non-sRGB attachment,
        // matching the upstream WGPU integration, to avoid a second conversion.
        config.format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&renderer.device, &config);
        let owner = WindowOwner::new(&window, owner)?;
        Ok(Self {
            surface,
            renderer,
            config,
            accessibility,
            projection: Projection::default(),
            accessibility_active: false,
            #[cfg(not(target_os = "android"))]
            ime_purpose: ImePurpose::Normal,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            file_drop: None,
            #[cfg(target_os = "macos")]
            resize_drag: None,
            owner,
            window,
            #[cfg(target_os = "android")]
            android_app,
            #[cfg(target_os = "android")]
            android_text: None,
            #[cfg(not(target_os = "android"))]
            applied_cursor: Cell::new(None),
            frame: FrameCache::default(),
            presentation: WindowPresentation::default(),
            inspection_revision: None,
            #[cfg(any(target_os = "ios", target_os = "android"))]
            safe_area: [0.0; 4],
            #[cfg(target_os = "ios")]
            ios_layout_direction: appearance.layout_direction,
            #[cfg(target_os = "ios")]
            ios_modal_queue: native_dialog::ModalQueue::default(),
            #[cfg(target_os = "ios")]
            ios_imported_folders: Vec::new(),
            #[cfg(target_os = "ios")]
            ios_text: None,
            #[cfg(target_os = "ios")]
            ios_edit_menu: None,
            #[cfg(target_os = "ios")]
            ios_keyboard_occlusion: ios::KeyboardOcclusion::None,
            #[cfg(target_os = "ios")]
            ios_keyboard: None,
            #[cfg(target_os = "ios")]
            ios_pointer: None,
            #[cfg(target_os = "ios")]
            _ios_accessibility_observer: None,
            #[cfg(target_os = "ios")]
            _ios_scroll_to_top: None,
            #[cfg(target_os = "ios")]
            ios_edge_back: None,
            #[cfg(target_os = "ios")]
            ios_spin_step_focus: None,
            #[cfg(target_os = "ios")]
            ios_keyboard_dismissed_focus: None,
            #[cfg(target_os = "ios")]
            ios_scroll_announcement: None,
            #[cfg(target_os = "android")]
            android_spin_step_focus: None,
            #[cfg(target_os = "android")]
            android_ime_top: None,
        })
    }

    fn set_visible(&self, visible: bool) {
        if visible {
            self.owner.attach();
        } else {
            self.owner.detach();
        }
        #[cfg(not(target_os = "android"))]
        self.window.set_visible(visible);
    }

    fn begin_client_resize(&mut self, edge: xrack_layout::WindowResizeEdge) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.resize_drag = window_chrome::ResizeDrag::start(&self.window, edge);
            self.resize_drag.is_some()
        }
        #[cfg(not(any(target_os = "macos", target_os = "android")))]
        {
            self.window
                .drag_resize_window(resize_direction(edge))
                .is_ok()
        }
        #[cfg(target_os = "android")]
        {
            let _ = edge;
            false
        }
    }

    #[cfg(target_os = "macos")]
    fn update_client_resize(&mut self, allowed: bool) -> bool {
        if !allowed {
            self.finish_client_resize();
        }
        if let Some(drag) = &self.resize_drag {
            drag.update();
            true
        } else {
            false
        }
    }

    fn finish_client_resize(&mut self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.resize_drag.take().is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn is_native_full_screen(&self) -> bool {
        #[cfg(target_os = "android")]
        return false;
        #[cfg(not(target_os = "android"))]
        self.window.fullscreen().is_some()
    }

    fn sample_window_status(&mut self) -> xrack_runtime::WindowStatus {
        #[cfg(target_os = "macos")]
        if self.resize_drag.is_some() {
            return self.presentation.observed();
        }
        self.presentation.sample(&self.window)
    }
    fn viewport(&self) -> Size {
        let size = drawable_size(&self.window).to_logical::<f32>(self.window.scale_factor());
        Size::new(size.width, size.height)
    }
    fn resize(&mut self) -> Result<(), String> {
        let size = drawable_size(&self.window);
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let limit = self.renderer.device.limits().max_texture_dimension_2d;
        if size.width > limit
            || size.height > limit
            || size.width as u64 * size.height as u64 > 64_000_000
        {
            return Err("window is larger than the supported render extent".into());
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.renderer.device, &self.config);
        Ok(())
    }
}

pub(crate) enum UserEvent {
    PollTasks,
    Clipboard(ClipboardReply),
    #[cfg(target_os = "ios")]
    IosReadClipboard(u64),
    #[cfg(target_os = "ios")]
    IosText(ios_text::NativeEdit),
    #[cfg(target_os = "ios")]
    IosTextMenu(u64, Option<usize>),
    #[cfg(target_os = "ios")]
    IosControlKey(KeyEvent, ModifiersState),
    #[cfg(target_os = "ios")]
    IosKeyCancelled,
    #[cfg(target_os = "ios")]
    IosPointer(ios_pointer::PointerInputEvent),
    #[cfg(target_os = "ios")]
    IosKeyboardGeometry,
    #[cfg(target_os = "ios")]
    IosAccessibilityPreferencesChanged,
    #[cfg(target_os = "ios")]
    IosAppearanceChanged,
    #[cfg(target_os = "ios")]
    IosScrollToTop,
    #[cfg(target_os = "ios")]
    IosBackRequested(AccessibilityEscape),
    #[cfg(target_os = "ios")]
    IosTransfer(native_dialog::TransferRequest),
    #[cfg(target_os = "ios")]
    IosSelectDirectory(native_dialog::DirectoryRequest),
    #[cfg(target_os = "ios")]
    IosSelectPhotos(native_dialog::PhotoRequest),
    #[cfg(target_os = "ios")]
    IosSelectColor(native_dialog::ColorRequest),
    #[cfg(target_os = "android")]
    AndroidSystem(native_dialog::AndroidSystemRequest),
    #[cfg(target_os = "android")]
    AndroidText(android_text::Edit),
    #[cfg(target_os = "android")]
    AndroidImeGeometry {
        token: i64,
        top: Option<u32>,
    },
    NativeDialogFinished {
        id: u64,
        result: NativeDialogResult,
    },
    #[cfg(target_os = "ios")]
    IosFoldersImported {
        id: u64,
        folders: native_dialog::ImportedFolders,
    },
    #[cfg(target_os = "android")]
    AndroidDocumentsImported {
        id: u64,
        files: native_dialog::ImportedFiles,
    },
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    NativeFileDrag {
        window: WindowId,
        input: xrack_layout::FileDragInput,
    },
    #[cfg(target_os = "ios")]
    IosFilesCopied {
        window: WindowId,
        input: xrack_layout::FileDragInput,
        directory: Arc<tempfile::TempDir>,
    },
    #[cfg(target_os = "ios")]
    IosTextDropStarted {
        window: WindowId,
        sequence: u64,
        point: Point,
        target: Target,
    },
    #[cfg(target_os = "ios")]
    IosTextDropLoaded {
        window: WindowId,
        sequence: u64,
        text: Option<String>,
    },
    #[cfg(target_os = "ios")]
    IosTextDragStarted(WindowId),
    #[cfg(target_os = "ios")]
    IosSelectedTextHeld {
        window: WindowId,
        target: Target,
        context: xrack_layout::TextEditContext,
        point: Point,
    },
    #[cfg(not(target_os = "android"))]
    Accessibility(accesskit_winit::Event),
    #[cfg(target_os = "android")]
    AndroidAccessibility(i64, accesskit_winit::Event),
    #[cfg(target_os = "android")]
    AndroidAccessibilitySelection(android_accessibility::SelectionRequest),
}

#[cfg(any(target_os = "ios", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AccessibilityEscape {
    Menu {
        session: u64,
        path: Vec<usize>,
    },
    Choice(Target),
    CellEdit(Target),
    Control {
        target: Target,
        action: ControlAction,
    },
}

enum ClipboardShortcut {
    Copy,
    Cut,
    Paste,
}

enum ClipboardContext {
    Cut(xrack_layout::TextEditContext),
    Paste(xrack_layout::TextEditContext),
    Table {
        destination: TablePasteDestination,
        apply: TablePaste,
    },
}

struct PendingClipboard {
    id: u64,
    context: ClipboardContext,
    deferred_text: Option<String>,
}

impl PendingClipboard {
    fn new(id: u64, context: ClipboardContext) -> Self {
        Self {
            id,
            context,
            deferred_text: None,
        }
    }

    #[cfg(target_os = "ios")]
    fn reads_text(&self) -> bool {
        matches!(
            self.context,
            ClipboardContext::Paste(_) | ClipboardContext::Table { .. }
        )
    }
}

#[derive(Clone, Copy)]
enum TouchRoute {
    Direct,
    Split,
}

#[derive(Clone, Copy)]
enum ScrollContact {
    Touch(DeviceId, u64, TouchRoute),
    MousePan(Point),
    MouseControl(Point),
    MouseViewport(Point),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PointerContact {
    Mouse(PointerButton),
    Touch(DeviceId, u64),
}

impl PointerContact {
    fn for_mouse(button: MouseButton) -> Option<Self> {
        Some(Self::Mouse(match button {
            MouseButton::Left => PointerButton::Primary,
            MouseButton::Right => PointerButton::Secondary,
            MouseButton::Middle => PointerButton::Middle,
            _ => return None,
        }))
    }

    fn for_scroll(contact: ScrollContact) -> Self {
        match contact {
            ScrollContact::Touch(device, id, _) => Self::Touch(device, id),
            ScrollContact::MousePan(_) => Self::Mouse(PointerButton::Middle),
            ScrollContact::MouseControl(_) | ScrollContact::MouseViewport(_) => {
                Self::Mouse(PointerButton::Primary)
            }
        }
    }

    fn payload(self) -> (PointerButton, bool) {
        match self {
            Self::Mouse(button) => (button, false),
            Self::Touch(..) => (PointerButton::Primary, true),
        }
    }
}

struct CapturedPointer {
    contact: PointerContact,
    sequence: PointerSequence,
}

enum TouchHoldKind {
    Editor(Target),
    ContextMenu(Target),
    Tooltip(TouchTooltip),
}

const TOUCH_HOLD_SLOP: f32 = 8.0;

struct TouchHold {
    contact: PointerContact,
    origin: Point,
    deadline: Option<Instant>,
    kind: TouchHoldKind,
}

struct SelectionHandleDrag {
    contact: PointerContact,
    target: Target,
    origin: Point,
    finger_offset: Point,
    value: String,
}

struct PointerClick {
    touch_device: Option<DeviceId>,
    point: Point,
    time: Instant,
}

impl PointerClick {
    fn follows(&self, previous: &Self) -> bool {
        self.touch_device == previous.touch_device
            && xrack_layout::pointer_click_follows(
                (previous.point, previous.time),
                (self.point, self.time),
            )
    }
}

type ViewportHandler<C> = Box<dyn FnMut(&mut C, Size)>;

struct Application<C> {
    component: C,
    viewport_handler: Option<ViewportHandler<C>>,
    base_scene: Rc<Node>,
    scene: Rc<Node>,
    transition: SceneTransition,
    reduce_motion: bool,
    transition_epoch: Instant,
    animation_wake: Option<Instant>,
    timers: TimerQueue,
    button_animations: xrack_runtime::ButtonAnimations,
    image_playback: ImagePlaybackQueue,
    media: xrack_runtime::MediaQueue,
    native_dialogs: NativeDialogQueue,
    stack_transitions: StackTransitions,
    layout: Option<Rc<LayoutNode>>,
    input: Interaction,
    scale_rotate: scale_rotate::Gestures,
    focus_requests: FocusRequests,
    loader_events_scene: std::rc::Weak<Node>,
    loader_frame_scene: std::rc::Weak<Node>,
    loader_redraw: bool,
    focus_observers: FocusObservers,
    hover_observers: HoverObservers,
    hover_pointer: Option<PhysicalPosition<f64>>,
    pointer_capture: Option<CapturedPointer>,
    touch_hold: Option<TouchHold>,
    #[cfg(target_os = "android")]
    input_batch_started_at: Option<Instant>,
    selection_handle_drag: Option<SelectionHandleDrag>,
    additional_touch_captures: Vec<CapturedPointer>,
    next_touch_finger_id: i64,
    last_pointer_click: Option<(PointerHandle, Option<RowKey>, PointerClick)>,
    last_button_click: Option<ButtonClick<Option<DeviceId>>>,
    scroll_requests: ScrollRequests,
    viewport_observers: ViewportObservers,
    focus_active: bool,
    #[cfg(target_os = "ios")]
    ios_application_active: bool,
    #[cfg(target_os = "ios")]
    ios_pressed_button: Option<MouseButton>,
    scroll_contact: Option<ScrollContact>,
    modifiers: ModifiersState,
    #[cfg(target_os = "android")]
    android_keyboard: android_keyboard::Keyboard,
    list_ime_focus: Option<xrack_layout::Target>,
    inspection_press: bool,
    state: Option<WindowState>,
    error: Option<String>,
    tasks: Rc<LocalExecutor>,
    clipboard: Option<Rc<RefCell<ClipboardPort>>>,
    pending_clipboard: Option<PendingClipboard>,
    #[cfg(target_os = "ios")]
    pending_text_drop: Option<(u64, xrack_layout::TextDrop)>,
    events: Option<WindowEvents>,
}

impl<C: Component> Application<C> {
    fn new(component: C, tasks: Rc<LocalExecutor>) -> Self {
        let scene = component.render_shared();
        Self {
            component,
            viewport_handler: None,
            base_scene: scene.clone(),
            scene,
            transition: SceneTransition::default(),
            reduce_motion: false,
            transition_epoch: Instant::now(),
            animation_wake: None,
            timers: TimerQueue::default(),
            button_animations: xrack_runtime::ButtonAnimations::default(),
            image_playback: ImagePlaybackQueue::default(),
            media: xrack_runtime::MediaQueue::default(),
            native_dialogs: NativeDialogQueue::default(),
            stack_transitions: StackTransitions::default(),
            layout: None,
            input: Interaction::default(),
            scale_rotate: scale_rotate::Gestures::default(),
            focus_requests: FocusRequests::default(),
            loader_events_scene: std::rc::Weak::new(),
            loader_frame_scene: std::rc::Weak::new(),
            loader_redraw: false,
            focus_observers: FocusObservers::default(),
            hover_observers: HoverObservers::default(),
            hover_pointer: None,
            pointer_capture: None,
            touch_hold: None,
            #[cfg(target_os = "android")]
            input_batch_started_at: None,
            selection_handle_drag: None,
            additional_touch_captures: Vec::new(),
            next_touch_finger_id: 1,
            last_pointer_click: None,
            last_button_click: None,
            scroll_requests: ScrollRequests::default(),
            viewport_observers: ViewportObservers::default(),
            focus_active: false,
            #[cfg(target_os = "ios")]
            ios_application_active: true,
            #[cfg(target_os = "ios")]
            ios_pressed_button: None,
            scroll_contact: None,
            modifiers: ModifiersState::default(),
            #[cfg(target_os = "android")]
            android_keyboard: android_keyboard::Keyboard::default(),
            list_ime_focus: None,
            inspection_press: false,
            state: None,
            error: None,
            tasks,
            clipboard: None,
            pending_clipboard: None,
            #[cfg(target_os = "ios")]
            pending_text_drop: None,
            events: None,
        }
    }

    fn process_user_event(&mut self, event: UserEvent) {
        let processed = match event {
            UserEvent::PollTasks => self.tasks.poll_ready(),
            UserEvent::Clipboard(reply) => {
                self.finish_clipboard(reply);
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosReadClipboard(id) => {
                self.read_ios_clipboard(id);
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosText(edit) => {
                self.commit_ios_text(edit);
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosTextMenu(session, item) => {
                let menu = self
                    .state
                    .as_ref()
                    .and_then(|state| state.ios_edit_menu.as_ref())
                    .and_then(|menu| menu.take(session));
                if let (Some(menu), Some(layout), Some(item)) = (menu, &self.layout, item)
                    && self.ios_application_active
                    && let Some(action) =
                        self.input
                            .native_text_menu_input(menu, &self.scene, layout, item)
                {
                    self.apply_control(action);
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosControlKey(event, modifiers) => {
                self.modifiers = modifiers;
                self.keyboard_input(&event, false);
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosKeyCancelled => {
                self.modifiers = ModifiersState::empty();
                self.input.cancel_press();
                self.update_editor();
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosPointer(event) => {
                self.ios_pointer_event(event);
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosKeyboardGeometry => {
                self.publish_viewport();
                self.relayout();
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosAccessibilityPreferencesChanged => {
                if self.sync_ios_accessibility_preferences() {
                    self.relayout();
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosAppearanceChanged => {
                self.sync_ios_appearance();
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosScrollToTop => {
                if self.ios_application_active
                    && let Some(layout) = &self.layout
                    && self.input.scroll_to_top(&self.scene, layout)
                {
                    self.relayout_with_caret(false);
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosBackRequested(request) => {
                if self.ios_application_active {
                    self.perform_accessibility_escape(request);
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosTransfer(request) => {
                if let Some(state) = &self.state {
                    let _ = self.tasks.spawn(
                        request.present(state.window.clone(), state.ios_modal_queue.clone()),
                    );
                } else {
                    let _ = request.reply.send(Err("the window is unavailable".into()));
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosSelectDirectory(request) => {
                if let Some(state) = &self.state {
                    let _ = self.tasks.spawn(
                        request.present(state.window.clone(), state.ios_modal_queue.clone()),
                    );
                } else {
                    let _ = request.reply.send(Err("the window is unavailable".into()));
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosSelectPhotos(request) => {
                if let Some(state) = &self.state {
                    let _ = self.tasks.spawn(
                        request.present(state.window.clone(), state.ios_modal_queue.clone()),
                    );
                } else {
                    let _ = request.reply.send(Err("the window is unavailable".into()));
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosSelectColor(request) => {
                if let Some(state) = &self.state {
                    let _ = self.tasks.spawn(
                        request.present(state.window.clone(), state.ios_modal_queue.clone()),
                    );
                } else {
                    let _ = request.reply.send(Err("the window is unavailable".into()));
                }
                false
            }
            UserEvent::NativeDialogFinished { id, result } => {
                self.native_dialogs.complete(id, result)
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosFoldersImported { id, folders } => {
                if let Some(state) = self.state.as_mut() {
                    let live = self.native_dialogs.is_active_request_live(id);
                    let completed = self
                        .native_dialogs
                        .complete(id, NativeDialogResult::Files(folders.paths));
                    if live && completed {
                        state.ios_imported_folders.push(folders.directory);
                    }
                    completed
                } else {
                    self.native_dialogs
                        .complete(id, NativeDialogResult::Abandoned)
                }
            }
            #[cfg(target_os = "android")]
            UserEvent::AndroidDocumentsImported { id, mut files } => {
                let live = self.native_dialogs.is_active_request_live(id);
                let result = NativeDialogResult::Files(std::mem::take(&mut files.paths));
                let completed = self.native_dialogs.complete(id, result);
                if live && completed {
                    files.keep();
                }
                completed
            }
            #[cfg(target_os = "android")]
            UserEvent::AndroidSystem(request) => {
                if let Some(state) = &self.state {
                    let _ = self.tasks.spawn(request.present(state.android_app.clone()));
                }
                false
            }
            #[cfg(target_os = "android")]
            UserEvent::AndroidText(edit) => {
                self.commit_android_text(edit);
                false
            }
            #[cfg(target_os = "android")]
            UserEvent::AndroidImeGeometry { token, top } => {
                if let Some(state) = self.state.as_mut()
                    && state
                        .android_text
                        .as_ref()
                        .is_some_and(|input| input.owns_token(token))
                    && state.android_ime_top != top
                {
                    state.android_ime_top = top;
                    self.publish_viewport();
                    self.relayout();
                }
                false
            }
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            UserEvent::NativeFileDrag { window, input } => {
                self.update_native_file_drag(window, input);
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosFilesCopied {
                window,
                input,
                directory,
            } => {
                if self.update_native_file_drag(window, input)
                    && let Some(bridge) = self
                        .state
                        .as_mut()
                        .and_then(|state| state.file_drop.as_mut())
                {
                    bridge.keep(directory);
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosTextDropStarted {
                window,
                sequence,
                point,
                target,
            } => {
                self.start_ios_text_drop(window, sequence, point, target);
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosTextDropLoaded {
                window,
                sequence,
                text,
            } => {
                self.finish_ios_text_drop(window, sequence, text);
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosTextDragStarted(window) => {
                if let Some(state) = self
                    .state
                    .as_ref()
                    .filter(|state| state.window.id() == window)
                {
                    if let Some(menu) = &state.ios_edit_menu {
                        menu.dismiss();
                    }
                    self.cancel_drag();
                    self.cancel_direct_scroll();
                }
                false
            }
            #[cfg(target_os = "ios")]
            UserEvent::IosSelectedTextHeld {
                window,
                target,
                context,
                point,
            } => {
                if self.ios_application_active
                    && self.state.as_ref().is_some_and(|state| {
                        state.window.id() == window && state.window.has_focus()
                    })
                    && context.is_current(&self.input, &self.scene)
                {
                    self.cancel_drag();
                    self.cancel_direct_scroll();
                    self.input.set_touch_selection_handles(true);
                    self.open_touch_selection_menu(target, point);
                    self.update_editor();
                }
                false
            }
            #[cfg(not(target_os = "android"))]
            UserEvent::Accessibility(event) => {
                self.accessibility_event(event);
                false
            }
            #[cfg(target_os = "android")]
            UserEvent::AndroidAccessibility(token, event) => {
                if self
                    .state
                    .as_ref()
                    .is_some_and(|state| state.accessibility.owns_token(token))
                {
                    self.accessibility_event(event);
                }
                false
            }
            #[cfg(target_os = "android")]
            UserEvent::AndroidAccessibilitySelection(request) => {
                self.apply_android_accessibility_selection(request);
                false
            }
        };
        if processed {
            self.refresh_after_update(false);
        }
    }

    fn update_accessibility(&mut self) {
        #[cfg(target_os = "ios")]
        let escape = self.accessibility_escape();
        let (Some(state), Some(layout)) = (&mut self.state, &self.layout) else {
            return;
        };
        let scale_factor = state.window.scale_factor();
        #[cfg(target_os = "ios")]
        if let Some(back) = &state.ios_edge_back {
            let request = escape.clone().filter(|request| {
                self.ios_application_active
                    && (!matches!(
                        request,
                        AccessibilityEscape::Control {
                            action: ControlAction::StackBack { .. },
                            ..
                        }
                    ) || !layout.has_leading_edge_drawer(&self.scene))
            });
            back.prepare(layout.layout_direction, request);
        }
        #[cfg(target_os = "ios")]
        state.accessibility.prepare(scale_factor, escape);
        if state.accessibility_active
            && let Some(update) = state.projection.update(
                &self.scene,
                layout,
                &self.input,
                scale_factor,
                Some(&state.renderer.text),
            )
        {
            #[cfg(target_os = "android")]
            state
                .accessibility
                .update_if_active(&state.projection, update);
            #[cfg(not(target_os = "android"))]
            state.accessibility.update_if_active(|| update);
        }
    }

    #[cfg(target_os = "android")]
    fn apply_android_accessibility_selection(
        &mut self,
        request: android_accessibility::SelectionRequest,
    ) {
        let Some(state) = self.state.as_ref().filter(|state| {
            state.accessibility_active && state.accessibility.owns_token(request.token)
        }) else {
            return;
        };
        let action = AccessibleAction::SetTextSelection;
        if let Some(route) = state.projection.route_for_action(request.node, action)
            && let Some(selection) = state.projection.text_selection_from_utf16(
                request.node,
                &request.value,
                request.anchor,
                request.focus,
            )
        {
            let target = match &route {
                Route::Element { target, .. } => target.clone(),
                _ => return,
            };
            if self.input.focused.as_ref() == Some(&target)
                && let Some(node) = target.node_in(&self.scene)
                && accessibility_text::accessible_text_value(node, &self.input, &target.path)
                    == request.value
                && let Some(editor) = self.input.editor.as_mut()
                && editor.has_committed_suggestion(&request.value)
            {
                // Replace the unchanged suggestion and select in one update.
                editor.set_preedit(String::new(), None);
            }
            self.accessibility_action(
                route,
                ActionRequest {
                    action,
                    target_tree: TreeId::ROOT,
                    target_node: request.node,
                    data: Some(ActionData::SetTextSelection(selection)),
                },
            );
            if let Some(traversal) = request.traversal
                && self.input.focused.as_ref() == Some(&target)
                && self.android_editor_selection(&target, &request.value)
                    == Some(traversal.selection)
            {
                self.update_accessibility();
                if let Some(state) = self
                    .state
                    .as_ref()
                    .filter(|state| state.accessibility.owns_token(request.token))
                {
                    state
                        .accessibility
                        .publish_traversal(request.node, &traversal);
                }
            }
        }
    }

    #[cfg(target_os = "android")]
    fn android_editor_selection(&self, target: &Target, value: &str) -> Option<[usize; 2]> {
        let node = target.node_in(&self.scene)?;
        if accessibility_text::accessible_text_value(node, &self.input, &target.path) != value {
            return None;
        }
        let editor = self.input.displayed_editor(node, &target.path)?;
        let (anchor, focus) = accessibility_text::selection_positions(editor, value);
        Some([
            value.get(..anchor)?.encode_utf16().count(),
            value.get(..focus.offset)?.encode_utf16().count(),
        ])
    }

    fn accessibility_event(&mut self, event: accesskit_winit::Event) {
        use accesskit_winit::WindowEvent as AccessEvent;
        let Some(state) = self
            .state
            .as_mut()
            .filter(|state| state.window.id() == event.window_id)
        else {
            return;
        };
        match event.window_event {
            AccessEvent::InitialTreeRequested => {
                state.accessibility_active = true;
                state.projection.clear();
                self.update_accessibility();
            }
            AccessEvent::AccessibilityDeactivated => {
                state.accessibility_active = false;
                #[cfg(target_os = "ios")]
                {
                    state.accessibility.clear_actions();
                    state.ios_scroll_announcement = None;
                }
                state.projection.clear();
            }
            AccessEvent::ActionRequested(request) if request.target_tree == TreeId::ROOT => {
                #[cfg(target_os = "ios")]
                if request.action == AccessibleAction::CustomAction
                    && matches!(
                        request.data,
                        Some(ActionData::CustomAction(
                            accessibility::REFRESH_VIEWPORT_ACTION
                        ))
                    )
                {
                    let viewport = state.projection.refresh_owner(request.target_node).cloned();
                    if let Some(viewport) = viewport
                        && let Some(layout) = &self.layout
                        && self
                            .input
                            .request_viewport_refresh(&self.scene, layout, &viewport)
                    {
                        self.refresh_editor(false);
                        self.refresh_after_update_with_caret(false, false);
                    }
                    return;
                }
                if let Some(route) = state
                    .projection
                    .route_for_action(request.target_node, request.action)
                {
                    self.accessibility_action(route, request);
                }
            }
            AccessEvent::ActionRequested(_) => {}
        }
    }

    fn accessibility_action(&mut self, route: Route, request: ActionRequest) {
        let Some(layout) = &self.layout else {
            return;
        };
        let target = match &route {
            Route::Element { target, .. }
            | Route::SplitHandle { target, .. }
            | Route::Choice { target, .. }
            | Route::ControlPart { target, .. }
            | Route::TableColumnHeader { target, .. }
            | Route::TableRowHeader(target) => target,
            Route::TabClose(target) => {
                if request.action == AccessibleAction::Click
                    && let Some(action) = self.input.tab_close_action(&self.scene, layout, target)
                {
                    if cfg!(target_os = "ios")
                        && self
                            .scene
                            .at_path(&action.path)
                            .and_then(xrack_runtime::group_index)
                            == target.path.last().copied()
                        && self
                            .input
                            .focused
                            .as_ref()
                            .is_none_or(|focus| !focus.path.starts_with(&target.path))
                        && let Some(header) =
                            self.input
                                .accessibility_target(&self.scene, layout, &target.path)
                    {
                        self.input.accessibility_focus(header);
                    }
                    self.apply_control(action);
                }
                return;
            }
            Route::TextLink(target) => {
                let Some(action) = self.input.text_link_activation(&self.scene, layout, target)
                else {
                    return;
                };
                match request.action {
                    AccessibleAction::Focus => {
                        self.input.focus_target(&self.scene, layout, target.clone());
                        self.update_editor();
                    }
                    AccessibleAction::Click => {
                        self.input.focus_target(&self.scene, layout, target.clone());
                        self.apply_control(action);
                    }
                    _ => {}
                }
                return;
            }
            Route::MenuEntry { trigger, entry } => {
                let activate = match request.action {
                    AccessibleAction::Focus => false,
                    AccessibleAction::Click => true,
                    AccessibleAction::Expand if entry.kind == "Menu" => true,
                    _ => return,
                };
                if let Some(action) = self.input.accessibility_menu_entry(
                    &self.scene,
                    layout,
                    trigger,
                    entry,
                    activate,
                ) {
                    self.apply_control(action);
                } else {
                    self.update_editor();
                }
                return;
            }
            Route::MenuPanel {
                trigger,
                path,
                session,
            } => {
                if request.action == AccessibleAction::Collapse {
                    self.input.accessibility_close_menu(
                        &self.scene,
                        layout,
                        trigger,
                        path,
                        *session,
                    );
                    self.update_editor();
                    return;
                }
                #[cfg(target_os = "ios")]
                if matches!(
                    request.action,
                    AccessibleAction::ScrollUp | AccessibleAction::ScrollDown
                ) && self.input.accessibility_menu_page(
                    &self.scene,
                    layout,
                    trigger,
                    path,
                    *session,
                    request.action == AccessibleAction::ScrollUp,
                ) {
                    self.update_editor();
                    if let Some(layout) = &self.layout
                        && let Some(panel) = self
                            .input
                            .menu_panels(&self.scene, layout)
                            .into_iter()
                            .find(|panel| panel.session() == *session && panel.path() == path)
                    {
                        let range = panel.visible_range();
                        ios::announce_accessibility_scroll(&format!(
                            "Menu rows {} to {} of {}",
                            range.start + 1,
                            range.end,
                            panel.row_count(),
                        ));
                    }
                }
                return;
            }
        };
        if self
            .input
            .accessibility_target(&self.scene, layout, &target.path)
            .as_ref()
            != Some(target)
        {
            return;
        }
        let Some(node) = self.scene.at_path(&target.path) else {
            return;
        };
        if self.input.menu_open() && node.kind != "Menu" {
            return;
        }
        match route {
            Route::SplitHandle {
                target,
                trailing_pane,
                orientation,
            } => {
                let Some(range) = layout.split_handle_range(&target.path).filter(|range| {
                    range.trailing_pane == trailing_pane && range.orientation == orientation
                }) else {
                    return;
                };
                let value = match (request.action, request.data) {
                    (AccessibleAction::Focus, _) => {
                        self.input.focus_target(&self.scene, layout, target);
                        self.update_editor();
                        return;
                    }
                    (AccessibleAction::Increment, _) => {
                        Some(range.value + xrack_layout::SplitHandleRange::STEP)
                    }
                    (AccessibleAction::Decrement, _) => {
                        Some(range.value - xrack_layout::SplitHandleRange::STEP)
                    }
                    (AccessibleAction::SetValue, Some(ActionData::NumericValue(value))) => {
                        Some(value)
                    }
                    (AccessibleAction::SetValue, Some(ActionData::Value(value))) => {
                        value.parse().ok()
                    }
                    _ => None,
                };
                if let Some(resize) = value.and_then(|value| {
                    self.input
                        .split_set_value(&self.scene, layout, &target, value)
                }) {
                    self.apply_split_resize(resize);
                }
            }
            Route::TableRowHeader(target) => {
                let action = match (request.action, request.data) {
                    (AccessibleAction::Click, _) => self.input.table_row_header_selection(
                        &self.scene,
                        layout,
                        &target,
                        RowSelection::Replace,
                    ),
                    (AccessibleAction::CustomAction, Some(ActionData::CustomAction(id))) => {
                        if let Some((_, increase, _)) = accessibility::TABLE_ROW_RESIZE_ACTIONS
                            .iter()
                            .find(|action| action.0 == id)
                        {
                            if let Some(action) = self.input.table_row_header_resize(
                                &self.scene,
                                layout,
                                &target,
                                *increase,
                            ) {
                                self.apply_accessible_layout_change(action, |_, _, _, _| false);
                            }
                            return;
                        }
                        accessibility::TABLE_ROW_MOVE_ACTIONS
                            .iter()
                            .find(|action| action.0 == id)
                            .and_then(|(_, direction, _)| {
                                self.input.table_row_header_move(
                                    &self.scene,
                                    layout,
                                    &target,
                                    *direction,
                                )
                            })
                    }
                    _ => None,
                };
                if let Some(action) = action
                    && let Some(table) =
                        self.input
                            .accessibility_target(&self.scene, layout, &action.path)
                {
                    self.input.accessibility_focus(table);
                    self.apply_control(action);
                }
            }
            Route::TableColumnHeader { target, column } => match (request.action, request.data) {
                (AccessibleAction::Click, _)
                    if node
                        .table_header
                        .as_ref()
                        .and_then(|header| header.next_sort(column))
                        .is_some()
                        && layout.at_path(&target.path).is_some_and(|item| {
                            item.table.as_ref().is_some_and(|table| {
                                table
                                    .header_cell_bounds(column, item.clip_rect(table.header))
                                    .is_some()
                            })
                        }) =>
                {
                    self.input.accessibility_focus(target.clone());
                    self.apply_control(ControlInput {
                        path: target.path,
                        action: ControlAction::SortColumn(column),
                    });
                }
                (AccessibleAction::CustomAction, Some(ActionData::CustomAction(id))) => {
                    let action = accessibility::TABLE_COLUMN_MOVE_ACTIONS
                        .iter()
                        .find(|action| action.0 == id)
                        .and_then(|(_, direction, _)| {
                            self.input.table_column_header_move(
                                &self.scene,
                                layout,
                                &target,
                                column,
                                *direction,
                            )
                        })
                        .or_else(|| {
                            let (_, increase, _) = accessibility::TABLE_COLUMN_RESIZE_ACTIONS
                                .iter()
                                .find(|action| action.0 == id)?;
                            self.input.table_column_header_resize(
                                &self.scene,
                                layout,
                                &target,
                                column,
                                *increase,
                            )
                        });
                    if let Some(action) = action {
                        self.apply_accessible_layout_change(
                            action,
                            |input, scene, layout, path| {
                                target.node_in(scene).zip(layout.at_path(path)).is_some_and(
                                    |(_, item)| input.reveal_table_column(path, item, column),
                                )
                            },
                        );
                    }
                }
                _ => {}
            },
            Route::ControlPart { target, action } => {
                if request.action == AccessibleAction::Click
                    && let Some(item) = layout.at_path(&target.path)
                    && match action {
                        ControlAction::Calendar(action) if is_calendar_control(node.kind) => {
                            xrack_layout::CalendarGeometry::for_layout(node, item)
                                .action_bounds(action)
                        }
                        ControlAction::PickTumbler { .. }
                            if matches!(node.kind, "Tumbler" | "PathView") =>
                        {
                            item.tumbler.as_ref().and_then(|wheel| {
                                wheel
                                    .rows
                                    .iter()
                                    .find(|entry| entry.action() == action)
                                    .map(|entry| entry.bounds.intersect(item.bounds))
                            })
                        }
                        ControlAction::SubmitSearch | ControlAction::ClearSearch
                            if node.kind == "SearchField" =>
                        {
                            xrack_layout::SearchGeometry::for_layout(item)
                                .actions(node)
                                .find_map(|(rect, candidate)| (candidate == action).then_some(rect))
                        }
                        ControlAction::SwipeDelegate(_) if node.kind == "SwipeDelegate" => item
                            .swipe_delegate
                            .as_deref()
                            .and_then(xrack_layout::SwipeDelegateGeometry::toggle)
                            .and_then(|(rect, candidate)| (candidate == action).then_some(rect)),
                        _ => None,
                    }
                    .is_some_and(|rect| {
                        let rect = rect.intersect(item.clip);
                        rect.width > 0.0 && rect.height > 0.0
                    })
                {
                    self.input.accessibility_focus(target.clone());
                    self.apply_control(ControlInput {
                        path: target.path,
                        action,
                    });
                }
            }
            Route::Choice {
                target,
                index,
                value,
                record,
            } => {
                if self.input.choice_is_open_for(&target.path)
                    && let Some(menu) = self.input.choice_menu(&self.scene, layout)
                    && let Some(offset) = index.checked_sub(menu.first)
                    && menu.rows.get(offset) == Some(&value)
                    && xrack_runtime::choice_identity(node, index)
                        .is_some_and(|current| current.record == record)
                {
                    match request.action {
                        AccessibleAction::Click if menu.row_enabled(offset) => {
                            self.input.dismiss_choice();
                            if node.kind != "SearchField" {
                                self.input.editor = None;
                            }
                            self.apply_control(ControlInput {
                                path: target.path,
                                action: ControlAction::Select(index),
                            });
                        }
                        #[cfg(target_os = "ios")]
                        AccessibleAction::ScrollUp | AccessibleAction::ScrollDown => {
                            if self.input.choice_page(
                                &self.scene,
                                layout,
                                request.action == AccessibleAction::ScrollUp,
                            ) {
                                self.update_editor();
                            }
                        }
                        _ => {}
                    }
                }
            }
            Route::Element {
                target,
                kind,
                standard_kind,
            } if kind == node.kind && standard_kind == node.standard_button_kind() => match request
                .action
            {
                AccessibleAction::Focus => {
                    self.input.focus_target(&self.scene, layout, target);
                    self.update_editor();
                }
                #[cfg(target_os = "ios")]
                AccessibleAction::CustomAction if kind == "SwipeDelegate" => {
                    if let Some(ActionData::CustomAction(id)) = request.data
                        && let Some((_, side, _)) = accessibility::SWIPE_DELEGATE_ACTIONS
                            .iter()
                            .find(|action| action.0 == id)
                        && let Some(action) = self.input.swipe_delegate_accessibility_side(
                            &self.scene,
                            layout,
                            &target,
                            *side,
                        )
                    {
                        self.input.accessibility_focus(target);
                        self.apply_control(action);
                    }
                }
                #[cfg(target_os = "ios")]
                AccessibleAction::CustomAction if matches!(kind, "Tab" | "TabButton") => {
                    if let Some(row) = target.row_key
                        && let Some(ActionData::CustomAction(id)) = request.data
                        && let Some((_, direction, _)) = accessibility::TAB_MOVE_ACTIONS
                            .iter()
                            .find(|action| action.0 == id)
                        && let Some(action) = self
                            .input
                            .tab_move_target(&self.scene, layout, &target, *direction)
                            .flatten()
                    {
                        self.apply_accessible_layout_change(
                            action,
                            |input, scene, layout, path| {
                                let Some((bar, item)) =
                                    scene.interactive_at_path(path).zip(layout.at_path(path))
                                else {
                                    return false;
                                };
                                bar.children
                                    .iter()
                                    .position(|tab| tab.row_key == Some(row))
                                    .is_some_and(|index| input.reveal_tab(bar, item, path, index))
                            },
                        );
                    }
                }
                #[cfg(target_os = "android")]
                AccessibleAction::CustomAction => {
                    if self.input.focused.as_ref() == Some(&target)
                        && let Some(ActionData::CustomAction(id)) = request.data
                        && let Some(shortcut) = android_accessibility::clipboard_shortcut(id)
                    {
                        self.finish_committed_suggestion();
                        self.request_clipboard(shortcut);
                    }
                }
                AccessibleAction::Expand | AccessibleAction::Collapse if kind == "Expander" => {
                    let expanded = request.action == AccessibleAction::Expand;
                    let path = target.path.clone();
                    self.input.accessibility_focus(target);
                    self.apply_control(ControlInput {
                        path,
                        action: ControlAction::Expander(expanded),
                    });
                }
                AccessibleAction::Expand | AccessibleAction::Collapse if kind == "ListRow" => {
                    if let Some(action) = self.input.tree_row_expansion(
                        &self.scene,
                        layout,
                        &target,
                        request.action == AccessibleAction::Expand,
                    ) {
                        self.input.accessibility_focus(target);
                        self.apply_control(action);
                    }
                }
                AccessibleAction::Expand | AccessibleAction::Collapse
                    if kind == "SwipeDelegate" =>
                {
                    let expand = request.action == AccessibleAction::Expand;
                    if let Some(action) = self.input.swipe_delegate_accessibility(
                        &self.scene,
                        layout,
                        &target,
                        expand,
                    ) {
                        self.input.accessibility_focus(target);
                        self.apply_control(action);
                    }
                }
                AccessibleAction::Click if kind == "DelayButton" => {
                    if let Some(action) = self.input.accessibility_delay_click(
                        &self.scene,
                        layout,
                        target,
                        Instant::now(),
                    ) {
                        self.apply_control(action);
                    } else {
                        self.update_editor();
                    }
                }
                #[cfg(target_os = "ios")]
                AccessibleAction::CustomAction if xrack_runtime::editable_table_cell(node) => {
                    let editing = self.input.table_cell_editing(&target.path);
                    match request.data {
                        Some(ActionData::CustomAction(accessibility::EDIT_CELL_ACTION)) => {
                            if self
                                .input
                                .begin_accessible_table_edit(&self.scene, layout, target)
                            {
                                self.update_editor();
                            }
                        }
                        Some(ActionData::CustomAction(accessibility::SAVE_CELL_ACTION))
                            if editing =>
                        {
                            self.commit_table_edit();
                        }
                        Some(ActionData::CustomAction(accessibility::CANCEL_CELL_ACTION))
                            if editing =>
                        {
                            self.cancel_table_edit();
                        }
                        _ => {}
                    }
                }
                AccessibleAction::Click if accessibility::supports_click(kind) => {
                    if node.boolean("focus-on-click", true) {
                        let focus = target
                            .path
                            .split_last()
                            .filter(|(_, parent)| {
                                kind == "ListRow"
                                    && self
                                        .scene
                                        .at_path(parent)
                                        .is_some_and(|node| node.kind == "GridView")
                            })
                            .and_then(|(_, parent)| {
                                self.input.accessibility_target(&self.scene, layout, parent)
                            })
                            .unwrap_or_else(|| target.clone());
                        self.input.accessibility_focus(focus);
                    }
                    if kind == "ComboBox" {
                        if self.input.choice_is_open_for(&target.path) {
                            self.input.dismiss_choice();
                        } else {
                            self.input.open_choice(&self.scene, layout, target);
                        }
                        self.update_editor();
                    } else {
                        self.activate(Some(target));
                    }
                }
                AccessibleAction::SetValue if accessibility::supports_text_value(node) => {
                    if let Some(ActionData::Value(value)) = request.data {
                        if node.table_cell_editor.is_some()
                            && !self.input.table_cell_editing(&target.path)
                        {
                            let original = node.text("text").unwrap_or_default();
                            if self.component.commit_table_cell(
                                &self.scene,
                                &target.path,
                                original,
                                value.into(),
                            ) {
                                self.refresh_scene();
                            }
                        } else {
                            self.input.accessibility_focus(target);
                            self.edit_text_from(EditAction::ReplaceAll(&value), EditSource::Atomic);
                        }
                    }
                }
                AccessibleAction::SetTextSelection
                    if target.focusable
                        && (accessibility_text::supports_ranges(node)
                            || self.input.table_cell_editing(&target.path))
                        && self
                            .input
                            .editor
                            .as_ref()
                            .is_none_or(|edit| edit.preedit.is_empty()) =>
                {
                    let value =
                        accessibility_text::accessible_text_value(node, &self.input, &target.path)
                            .into_owned();
                    if let Some(ActionData::SetTextSelection(selection)) = request.data
                        && let Some((anchor, focus)) = self.state.as_ref().and_then(|state| {
                            state
                                .projection
                                .text_selection(request.target_node, selection, &value)
                        })
                    {
                        self.pending_clipboard = None;
                        self.input.accessibility_focus(target);
                        self.input.break_text_edit_group();
                        self.input
                            .editor
                            .get_or_insert_with(|| xrack_layout::TextEdit::at_end(&value))
                            .set_selection(&value, anchor, focus);
                        self.update_editor();
                    }
                }
                AccessibleAction::SetValue
                    if node.kind == "DoubleSpinBox" && spin_editable(node) =>
                {
                    let value = match request.data {
                        Some(ActionData::NumericValue(value)) => SpinDecimal::new(value),
                        Some(ActionData::Value(text)) => {
                            SpinDecimal::parse(&text, decimal_separator(node))
                        }
                        _ => None,
                    };
                    if let Some(value) = value {
                        self.input.accessibility_focus(target.clone());
                        self.apply_control(ControlInput {
                            path: target.path,
                            action: ControlAction::Spin(SpinAction::CommitDecimal(value)),
                        });
                    }
                }
                AccessibleAction::ShowContextMenu
                    if target.focusable
                        && self
                            .input
                            .supports_context_menu(&self.scene, layout, &target) =>
                {
                    self.input.accessibility_focus(target);
                    self.input.open_context_menu(&self.scene, layout, true);
                    self.pending_clipboard = None;
                    self.update_editor();
                }
                AccessibleAction::SetValue if matches!(kind, "Slider" | "Dial" | "RangeThumb") => {
                    let value = match request.data {
                        Some(ActionData::NumericValue(value)) => Some(value),
                        Some(ActionData::Value(text)) => text.parse::<f64>().ok(),
                        _ => None,
                    };
                    if let Some(value) = value.filter(|value| value.is_finite()) {
                        self.adjust_range(RangeInput {
                            path: target.path,
                            action: RangeAction::SetValue(value),
                        });
                    }
                }
                AccessibleAction::Increment | AccessibleAction::Decrement
                    if matches!(kind, "Slider" | "Dial" | "RangeThumb") =>
                {
                    self.adjust_range(RangeInput {
                        path: target.path,
                        action: if request.action == AccessibleAction::Increment {
                            RangeAction::Increase
                        } else {
                            RangeAction::Decrease
                        },
                    });
                }
                AccessibleAction::Increment | AccessibleAction::Decrement
                    if matches!(kind, "Tumbler" | "PathView") =>
                {
                    let wheel = xrack_runtime::Tumbler::from_node(node);
                    let delta = if request.action == AccessibleAction::Increment {
                        1
                    } else {
                        -1
                    };
                    if let Some(action) = wheel.step(delta).and_then(|index| wheel.select(index)) {
                        self.apply_control(ControlInput {
                            path: target.path,
                            action,
                        });
                    }
                }
                AccessibleAction::Increment | AccessibleAction::Decrement
                    if kind == "PageIndicator" =>
                {
                    if let Some(index) = xrack_runtime::page_indicator_step(
                        node,
                        request.action == AccessibleAction::Increment,
                    ) {
                        self.apply_control(ControlInput {
                            path: target.path,
                            action: ControlAction::Select(index),
                        });
                    }
                }
                AccessibleAction::Increment | AccessibleAction::Decrement
                    if is_spin_box(kind) && !node.text_read_only() =>
                {
                    self.apply_control(ControlInput {
                        path: target.path,
                        action: ControlAction::Spin(
                            if request.action == AccessibleAction::Increment {
                                SpinAction::Increase
                            } else {
                                SpinAction::Decrease
                            },
                        ),
                    });
                }
                #[cfg(target_os = "ios")]
                AccessibleAction::ScrollUp
                | AccessibleAction::ScrollDown
                | AccessibleAction::ScrollLeft
                | AccessibleAction::ScrollRight
                    if kind == "SwipeView" =>
                {
                    if let Some(index) = layout.at_path(&target.path).and_then(|item| {
                        accessibility::swipe_scroll_selection(request.action, node, item)
                    }) {
                        let announcement =
                            ios::ScrollAnnouncement::page(target.clone(), node, index);
                        self.apply_control(ControlInput {
                            path: target.path,
                            action: ControlAction::Select(index),
                        });
                        if let Some(state) = &mut self.state {
                            state.ios_scroll_announcement = announcement;
                        }
                    }
                }
                AccessibleAction::ScrollUp
                | AccessibleAction::ScrollDown
                | AccessibleAction::ScrollLeft
                | AccessibleAction::ScrollRight
                | AccessibleAction::SetScrollOffset
                | AccessibleAction::SetValue
                | AccessibleAction::Increment
                | AccessibleAction::Decrement => {
                    if kind == "ScrollBar" && node.scroll_target.is_none() {
                        if let Some(action) = accessibility::requested_standalone_scroll(
                            request.action,
                            request.data.as_ref(),
                            node,
                        ) {
                            self.adjust_range(RangeInput {
                                path: target.path,
                                action,
                            });
                        }
                        return;
                    }
                    let Some(item) = layout.at_path(&target.path) else {
                        return;
                    };
                    let scale = self
                        .state
                        .as_ref()
                        .map_or(1.0, |state| state.window.scale_factor());
                    let offsets = accessibility::requested_scroll(
                        request.action,
                        request.data.as_ref(),
                        node,
                        item,
                        scale,
                    );
                    let mut moved = false;
                    for (axis, offset) in [Orientation::Horizontal, Orientation::Vertical]
                        .into_iter()
                        .zip(offsets)
                    {
                        if let Some(offset) = offset {
                            moved |= self.input.accessibility_scroll_to(
                                layout,
                                &target.path,
                                axis,
                                offset,
                            );
                        }
                    }
                    if moved {
                        #[cfg(target_os = "ios")]
                        if let Some(announcement) =
                            ios::ScrollAnnouncement::viewport(&target, item, request.action)
                            && let Some(state) = &mut self.state
                        {
                            state.ios_scroll_announcement = Some(announcement);
                            state.projection.follow_ios_list_scroll(
                                request.target_node,
                                request.action,
                                item,
                            );
                        }
                        self.relayout_with_caret(false);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn close_async(&self) {
        self.tasks.close();
    }

    fn fail(&mut self, error: String) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
    fn prepare_frame(&mut self, elapsed: Duration) -> bool {
        let Some(state) = &mut self.state else {
            return false;
        };
        let viewport = state.viewport();
        #[cfg(target_os = "ios")]
        let previous_insets = state.safe_area;
        #[cfg(any(target_os = "ios", target_os = "android"))]
        let insets = {
            #[cfg(target_os = "ios")]
            let insets = {
                let (insets, occlusion) =
                    ios::content_geometry(&state.window, state.ios_text.as_ref());
                state.ios_keyboard_occlusion = occlusion;
                insets
            };
            #[cfg(target_os = "android")]
            let insets = state.content_insets();
            if insets != state.safe_area {
                state.safe_area = insets;
                state.frame = FrameCache::default();
            }
            insets
        };
        let base = match self.transition.frame(&self.base_scene, elapsed) {
            Cow::Borrowed(_) => self.base_scene.clone(),
            Cow::Owned(frame) => Rc::new(frame),
        };
        #[cfg(target_os = "ios")]
        let resized_base = self.layout.as_ref().and_then(|layout| {
            (previous_insets != insets
                || viewport != Size::new(layout.bounds.width, layout.bounds.height))
            .then(|| base.clone())
        });
        #[cfg(target_os = "ios")]
        let floating_keyboard = match state.ios_keyboard_occlusion {
            ios::KeyboardOcclusion::Floating(keyboard) => Some(keyboard),
            _ => None,
        };
        #[cfg(target_os = "ios")]
        self.input.set_floating_keyboard(floating_keyboard);
        #[cfg(target_os = "ios")]
        let floating_base = (floating_keyboard.is_some()
            || self.input.keyboard_avoidance().is_some())
        .then(|| base.clone());
        let elastic_input = self
            .input
            .has_overscroll()
            .then(|| (base.clone(), self.input.overscroll_frame()));
        #[cfg(target_os = "ios")]
        let direction = state.ios_layout_direction;
        let prepare_base = |base| {
            #[cfg(any(target_os = "ios", target_os = "android"))]
            let base = inset_content(base, insets);
            #[cfg(target_os = "ios")]
            let base = xrack_layout::apply_system_layout_direction(base, direction);
            base
        };
        let (mut scene, mut layout) = state.frame.prepare(base, viewport, &self.input, |base| {
            state
                .renderer
                .prepare_scene(&self.component, prepare_base(base), viewport, &self.input)
        });
        #[cfg(target_os = "ios")]
        if let Some(base) = floating_base {
            let caret = floating_keyboard.and_then(|_| {
                state
                    .renderer
                    .text
                    .visible_multiline_caret_rect(&self.input, &scene, &layout)
            });
            if self
                .input
                .avoid_floating_keyboard(&scene, &layout, floating_keyboard, caret)
            {
                (scene, layout) = state.frame.prepare(base, viewport, &self.input, |base| {
                    state.renderer.prepare_scene(
                        &self.component,
                        prepare_base(base),
                        viewport,
                        &self.input,
                    )
                });
            }
        }
        if self
            .layout
            .as_ref()
            .is_some_and(|previous| Rc::ptr_eq(previous, &layout))
        {
            let cancelled = self.scale_rotate.reconcile(&scale_rotate::Context {
                scene: &scene,
                layout: &layout,
                input: &self.input,
            });
            let previous = std::mem::replace(&mut self.scene, scene);
            let swiped = self.flush_swipe_events(Some(&previous));
            if let Some(event) = cancelled {
                self.dispatch_scale_rotate(&previous, event);
                return true;
            }
            return swiped;
        }
        // A docked keyboard can clip the editor's old location completely.
        // Reveal it in the new viewport before focus reconciliation.
        #[cfg(target_os = "ios")]
        if let Some(base) = resized_base
            && self.input.reveal_focused_editor(&scene, &layout)
        {
            (scene, layout) = state.frame.prepare(base, viewport, &self.input, |base| {
                state.renderer.prepare_scene(
                    &self.component,
                    prepare_base(base),
                    viewport,
                    &self.input,
                )
            });
        }
        self.update_page_transitions(&scene, &layout, Instant::now());
        self.input.reconcile(&scene, &layout);
        if let Some((base, offsets)) = elastic_input
            && offsets != self.input.overscroll_frame()
        {
            let Some(state) = self.state.as_mut() else {
                return false;
            };
            (scene, layout) = state.frame.prepare(base, viewport, &self.input, |base| {
                state.renderer.prepare_scene(
                    &self.component,
                    prepare_base(base),
                    viewport,
                    &self.input,
                )
            });
            self.input.reconcile(&scene, &layout);
        }
        let requested = self.requested_drag_action();
        let drag = self.input.reconcile_drag(&scene, &layout, requested);
        let files = self.input.reconcile_file_drag(&scene, &layout);
        let cancelled_gesture = self
            .scale_rotate
            .reconcile(&scale_rotate::Context {
                scene: &scene,
                layout: &layout,
                input: &self.input,
            })
            .map(|event| (self.scene.clone(), event));
        if self.scroll_requests.is_empty() {
            self.viewport_observers.sync(&scene, &layout);
        }
        let previous = std::mem::replace(&mut self.scene, scene);
        self.layout = Some(layout);
        self.flush_swipe_events(Some(&previous));
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        self.install_file_drop();
        self.dispatch_drag(drag);
        self.dispatch_drag(files);
        if let Some((previous, event)) = cancelled_gesture {
            self.dispatch_scale_rotate(&previous, event);
        }
        self.sync_window_effects(Instant::now());
        true
    }

    fn update_page_transitions(&mut self, scene: &Node, layout: &LayoutNode, now: Instant) {
        if !self.reduce_motion
            && let Some(previous) = &self.layout
        {
            self.stack_transitions.update_with_input(
                (&self.scene, previous),
                (scene, layout),
                now,
                Some(&self.input),
            );
            self.input.sync_stack_transitions(&self.stack_transitions);
        }
    }

    #[cfg(target_os = "ios")]
    fn sync_ios_appearance(&mut self) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let appearance = ios::system_appearance(&state.window);
        let direction_changed = state.ios_layout_direction != appearance.layout_direction;
        state.ios_layout_direction = appearance.layout_direction;
        let layout_changed = direction_changed && self.base_scene.layout_direction().is_none();
        if layout_changed {
            state.frame = FrameCache::default();
        }
        self.update_system_appearance(appearance.scheme, Some(appearance.increased_contrast));
        if layout_changed && self.layout.is_some() {
            self.relayout();
        }
    }

    fn update_system_appearance(&mut self, scheme: ColorScheme, increased_contrast: Option<bool>) {
        let Some(state) = &mut self.state else {
            return;
        };
        let scheme_changed = state.renderer.set_system_color_scheme(scheme);
        let contrast_changed = increased_contrast
            .is_some_and(|increased| state.renderer.set_system_increased_contrast(increased));
        if contrast_changed
            || scheme_changed && self.base_scene.color_scheme() == Some(ColorScheme::System)
        {
            state.window.request_redraw();
        }
    }

    #[cfg(target_os = "ios")]
    fn sync_ios_accessibility_preferences(&mut self) -> bool {
        let reduced = self.set_reduce_motion(ios::reduce_motion());
        let text_changed = self.state.as_mut().is_some_and(|state| {
            let labels_changed = state
                .renderer
                .set_system_switch_labels(ios::switch_labels());
            let shapes_changed = state
                .renderer
                .set_system_button_shapes(ios::button_shapes());
            if labels_changed || shapes_changed {
                state.window.request_redraw();
            }
            let scale_changed = state.renderer.set_text_scale(ios::text_scale());
            let weight_changed = state.renderer.set_system_bold_text(ios::bold_text());
            if scale_changed || weight_changed {
                state.frame = FrameCache::default();
                true
            } else {
                false
            }
        });
        reduced || text_changed
    }

    #[cfg(any(target_os = "ios", test))]
    fn set_reduce_motion(&mut self, reduced: bool) -> bool {
        if self.reduce_motion == reduced {
            return false;
        }
        self.reduce_motion = reduced;
        self.input.set_reduce_motion(reduced);
        if reduced {
            self.transition = SceneTransition::default();
            self.animation_wake = None;
            self.stack_transitions = StackTransitions::default();
            if self.input.sync_stack_transitions(&self.stack_transitions)
                && let Some(layout) = &self.layout
            {
                self.input.reconcile(&self.scene, layout);
            }
        }
        true
    }

    fn relayout(&mut self) {
        self.relayout_with_caret(true);
    }

    fn relayout_with_caret(&mut self, follow_caret: bool) {
        self.prepare_frame(self.transition_epoch.elapsed());
        self.layout_changed(follow_caret);
    }

    fn layout_changed(&mut self, follow_caret: bool) {
        self.refresh_editor(follow_caret);
    }

    fn update_pointer_cursor(&self) {
        #[cfg(not(target_os = "android"))]
        {
            let Some(state) = &self.state else {
                return;
            };
            #[cfg(target_os = "ios")]
            let cursor = if self.hover_pointer.is_some() {
                self.pointer_cursor()
            } else {
                CursorIcon::Default
            };
            #[cfg(not(target_os = "ios"))]
            let cursor = self.pointer_cursor();
            if state.applied_cursor.get() != Some(cursor) {
                #[cfg(target_os = "ios")]
                if let Some(pointer) = &state.ios_pointer {
                    pointer.set_cursor(cursor);
                }
                #[cfg(not(target_os = "ios"))]
                state.window.set_cursor(cursor);
                state.applied_cursor.set(Some(cursor));
            }
        }
    }

    #[cfg(any(not(target_os = "android"), test))]
    fn pointer_cursor(&self) -> CursorIcon {
        if let Some(action) = self.input.drag_action() {
            match action {
                DragAction::None => CursorIcon::NoDrop,
                DragAction::Copy => CursorIcon::Copy,
                DragAction::Move => CursorIcon::Move,
                DragAction::Link => CursorIcon::Alias,
            }
        } else if let Some(orientation) = self.input.split_orientation(&self.scene) {
            match orientation {
                Orientation::Horizontal => CursorIcon::ColResize,
                Orientation::Vertical => CursorIcon::RowResize,
            }
        } else if self.input.table_row_resize_cursor() {
            CursorIcon::RowResize
        } else if self.input.table_resize_cursor() {
            CursorIcon::ColResize
        } else if let Some(edge) = self.window_resize_cursor() {
            resize_direction(edge).into()
        } else if self.input.menu_open() || self.input.choice_open() {
            CursorIcon::Default
        } else {
            let hit = self.input.hovered.as_ref().and_then(|target| {
                if self
                    .layout
                    .as_ref()
                    .is_some_and(|layout| !self.input.allows_popup(layout, &target.path))
                {
                    return None;
                }
                let node = self.scene.interactive_at_path(&target.path)?;
                (self.scene.row_key_at(&target.path) == target.row_key).then_some((target, node))
            });
            if self.hover_pointer.is_some() {
                let direct = hit
                    .filter(|(_, node)| node.kind != "TouchArea")
                    .and_then(|(_, node)| native_cursor_shape(node.cursor_shape()));
                if let Some(cursor) =
                    direct.or_else(|| native_cursor_shape(self.hover_observers.cursor_shape()))
                {
                    return cursor;
                }
            }
            match hit {
                Some((_, node)) if node.kind == "ColorPlane" => CursorIcon::Crosshair,
                Some((target, node)) if self.pointer_over_text(target, node) => CursorIcon::Text,
                Some(_) => CursorIcon::Pointer,
                None => CursorIcon::Default,
            }
        }
    }

    #[cfg(any(not(target_os = "android"), test))]
    fn pointer_over_text(&self, target: &Target, node: &Node) -> bool {
        if !(self.input.is_text_editor(node, target) || spin_editable(node)) {
            return false;
        }
        if !matches!(
            node.kind,
            "ComboBox" | "SearchField" | "SpinBox" | "DoubleSpinBox"
        ) {
            return true;
        }
        let Some((item, point)) = self
            .layout
            .as_ref()
            .and_then(|layout| layout.at_path(&target.path))
            .zip(self.input.pointer_position())
        else {
            return false;
        };
        let entry = match node.kind {
            "ComboBox" => {
                ComboGeometry::for_control(node, item.bounds, item.layout_direction).entry
            }
            "SearchField" => xrack_layout::SearchGeometry::for_layout(item).entry,
            _ => SpinGeometry::for_layout(item, node).text,
        };
        item.map_from_window(point)
            .is_some_and(|point| entry.contains(point))
    }

    fn window_resize_cursor(&self) -> Option<xrack_layout::WindowResizeEdge> {
        let state = self.state.as_ref()?;
        #[cfg(target_os = "macos")]
        if let Some(drag) = &state.resize_drag {
            return Some(drag.edge());
        }
        if state.presentation.observed().maximized
            || state.presentation.observed().full_screen
            || state.is_native_full_screen()
            || state.presentation.is_switching_full_screen()
        {
            return None;
        }
        self.input
            .window_resize_edge(&self.scene, self.layout.as_ref()?)
    }

    fn dismiss_tooltip(&mut self) -> bool {
        let visible = self.input.dismiss_tooltip();
        if visible && let Some(state) = &self.state {
            state.window.request_redraw();
        }
        visible
    }
    fn activate(&mut self, target: Option<Target>) {
        self.activate_with_double_click(target, None, false);
    }

    fn activate_with_double_click(
        &mut self,
        mut target: Option<Target>,
        double_click: Option<PointerHandle>,
        touch: bool,
    ) {
        if self.flush_button_press_events() {
            self.refresh_scene();
            target = target.filter(|target| {
                self.layout.as_ref().is_some_and(|layout| {
                    self.input
                        .accessibility_target(&self.scene, layout, &target.path)
                        .as_ref()
                        == Some(target)
                })
            });
        }
        if let (Some(delegate_target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(selection) =
                self.input
                    .list_item_delegate_selection(&self.scene, layout, delegate_target)
        {
            let row = delegate_target.row_key;
            let view = selection.path.clone();
            let operation = self.row_selection(false);
            let selection = selection.with_row_selection(&self.scene, operation);
            let selection_only = operation != RowSelection::Replace
                && matches!(selection.action, ControlAction::TableSelection(_));
            self.apply_control(selection);
            if selection_only {
                return;
            }
            let resolved = row.zip(self.layout.as_ref()).and_then(|(row, layout)| {
                self.input
                    .resolve_item_delegate(&self.scene, layout, &view, row)
            });
            if resolved.is_none() {
                return;
            }
            // Selecting a partially visible row can move it to a different slot.
            target = resolved;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(selection) = self.input.table_delegate_selection(
                &self.scene,
                layout,
                target,
                self.row_selection(false),
            )
        {
            self.apply_control(selection);
            // A selection handler can sort, replace, or remove the clicked row.
            if self.scene.row_key_at(&target.path) != target.row_key {
                return;
            }
        }
        if let Some(target) = target
            .as_ref()
            .filter(|target| target.text_link_index().is_some())
        {
            if let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.text_link_activation(&self.scene, layout, target))
            {
                self.apply_control(action);
            }
            return;
        }
        if let Some(target) = &target
            && let Some(node) = self.scene.interactive_at_path(&target.path)
            && node.kind == "DelayButton"
        {
            if node.boolean("checked", false) {
                self.apply_control(ControlInput {
                    path: target.path.clone(),
                    action: ControlAction::DelayButton(false),
                });
            }
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && self
                .input
                .open_menu_bar_with_touch(&self.scene, layout, target.clone(), touch)
        {
            self.pending_clipboard = None;
            self.update_editor();
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(action) = self.input.calendar_activate(&self.scene, layout, target)
        {
            self.apply_control(action);
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(action) = self.input.dialog_button(&self.scene, layout, target)
        {
            self.apply_control(action);
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(action) = self.input.tree_pointer_action(&self.scene, layout, target)
        {
            self.apply_control(action);
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(action) = self.input.select_group_option_with_selection(
                &self.scene,
                layout,
                target,
                self.row_selection(false),
            )
        {
            self.apply_control(action);
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && self
                .input
                .open_choice_at_pointer(&self.scene, layout, target.clone())
        {
            self.update_editor();
            return;
        }
        let mut dispatched = false;
        if let Some(target) = target {
            if self
                .component
                .dispatch_at(&self.scene, &target.path, "clicked")
            {
                self.refresh_scene();
                dispatched = true;
            }
            if let Some(handle) = double_click
                && self.scene.row_key_at(&target.path) == target.row_key
                && self
                    .scene
                    .interactive_at_path(&target.path)
                    .is_some_and(|node| node.pointer_handle.as_ref() == Some(&handle))
                && self
                    .component
                    .dispatch_at(&self.scene, &target.path, "double-clicked")
            {
                self.refresh_scene();
                dispatched = true;
            }
        }
        if !dispatched && let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn dialog_key(&mut self, event: &KeyEvent) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        if event.logical_key != Key::Named(NamedKey::Escape)
            || self
                .input
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return false;
        }
        if event.state == ElementState::Pressed
            && !event.repeat
            && let Some(action) = self.input.dismiss_focused_swipe(&self.scene, layout)
        {
            self.apply_control(action);
            return true;
        }
        let action = self.input.dialog_escape(&self.scene, layout);
        if layout.dialog_paths().is_empty() && action.is_none() {
            return false;
        }
        if event.state == ElementState::Pressed
            && !event.repeat
            && let Some(action) = action
        {
            self.apply_control(action);
        }
        true
    }

    #[cfg(any(target_os = "ios", test))]
    fn accessibility_escape(&self) -> Option<AccessibilityEscape> {
        let layout = self.layout.as_deref()?;
        if self.input.menu_open() {
            let panels = self.input.menu_panels(&self.scene, layout);
            let panel = panels.last()?;
            return Some(AccessibilityEscape::Menu {
                session: panel.session(),
                path: panel.path().to_vec(),
            });
        }
        if self.input.choice_open() {
            return self
                .input
                .focused
                .as_ref()
                .filter(|target| self.input.choice_is_open_for(&target.path))
                .cloned()
                .map(AccessibilityEscape::Choice);
        }
        if self
            .input
            .editor
            .as_ref()
            .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return None;
        }
        if let Some(target) = self.input.focused.as_ref()
            && self.input.table_cell_editing(&target.path)
        {
            return Some(AccessibilityEscape::CellEdit(target.clone()));
        }
        let next = self.input.dialog_escape(&self.scene, layout).or_else(|| {
            if layout
                .overlay_paths()
                .iter()
                .any(|path| self.input.allows_popup(layout, path))
            {
                return None;
            }
            self.input.stack_back_in_window(&self.scene, layout)
        })?;
        let node = self.scene.at_path(&next.path)?;
        let row = self.scene.row_key_at(&next.path);
        Some(AccessibilityEscape::Control {
            target: Target::from_node(next.path, node, false, row),
            action: next.action,
        })
    }

    #[cfg(any(target_os = "ios", test))]
    fn perform_accessibility_escape(&mut self, request: AccessibilityEscape) {
        if self.accessibility_escape().as_ref() != Some(&request) {
            return;
        }
        let key = KeyEvent {
            logical_key: Key::Named(NamedKey::Escape),
            unmodified_key: Key::Named(NamedKey::Escape),
            text: None,
            state: ElementState::Pressed,
            repeat: false,
        };
        match request {
            AccessibilityEscape::Menu { .. } => {
                self.menu_key(&key);
            }
            AccessibilityEscape::Choice(_) => {
                self.choice_key(&key);
            }
            AccessibilityEscape::CellEdit(_) => self.cancel_table_edit(),
            AccessibilityEscape::Control { target, action } => {
                self.apply_control(ControlInput {
                    path: target.path,
                    action,
                });
            }
        }
    }

    fn refresh_scene(&mut self) {
        self.refresh_after_update(true);
    }

    fn refresh_dependencies(&mut self) {
        if let Some(state) = &self.state
            && self
                .layout
                .as_ref()
                .is_some_and(|layout| self.input.choice_artwork_dirty(&self.scene, layout))
        {
            state.window.request_redraw();
        }
        if self.component.scene_dirty()
            || self.input.has_pending_swipe_events()
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty()
            || self
                .state
                .as_ref()
                .is_some_and(|state| state.frame.is_dirty())
        {
            self.refresh_after_update(false);
        }
    }

    fn refresh_after_update(&mut self, redraw_input: bool) {
        self.refresh_after_update_with_caret(redraw_input, true);
    }

    fn refresh_after_update_with_caret(&mut self, redraw_input: bool, follow_caret: bool) {
        let needs_update = redraw_input
            || self.component.scene_dirty()
            || self
                .state
                .as_ref()
                .is_some_and(|state| state.frame.is_dirty())
            || self.input.has_pending_swipe_events()
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty();
        let inspector = self
            .component
            .window_inspector(&self.base_scene)
            .filter(|inspector| needs_update && inspector.profiling());
        if let Some(inspector) = inspector {
            inspector.measure_update(|| {
                self.refresh_after_update_inner(redraw_input, follow_caret);
            });
        } else {
            self.refresh_after_update_inner(redraw_input, follow_caret);
        }
    }

    fn refresh_after_update_inner(&mut self, redraw_input: bool, follow_caret: bool) {
        self.flush_swipe_events(None);
        let scene_changed = self.sync_base_scene();
        if self.prepare_frame(self.transition_epoch.elapsed())
            || scene_changed
            || redraw_input
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty()
        {
            // Reactive layout updates, including scroll position observers, can run
            // after manual scrolling. Only an input update should reveal the caret
            // through the surrounding viewports again.
            let reveal = match (follow_caret, redraw_input) {
                (false, _) => CaretFollow::None,
                (true, false) => CaretFollow::Editor,
                (true, true) => CaretFollow::Viewports,
            };
            self.refresh_editor_with_caret(reveal);
        }
    }

    fn publish_viewport(&mut self) {
        let Some((handler, state)) = self.viewport_handler.as_mut().zip(self.state.as_ref()) else {
            return;
        };
        let size = state.viewport();
        #[cfg(any(target_os = "ios", target_os = "android"))]
        let size = {
            let [left, top, right, bottom] = state.content_insets();
            Size::new(
                size.width - (left + right) as f32,
                size.height - (top + bottom) as f32,
            )
        };
        if size.width <= 0.0 || size.height <= 0.0 {
            return;
        }
        handler(&mut self.component, size);
        self.sync_base_scene();
    }

    fn sync_base_scene(&mut self) -> bool {
        let target = self.component.render_shared();
        if Rc::ptr_eq(&self.base_scene, &target) {
            return false;
        }
        #[cfg(not(target_os = "android"))]
        let previous_width = self.base_scene.length("width");
        #[cfg(not(target_os = "android"))]
        let previous_height = self.base_scene.length("height");
        let was_visible = self.base_scene.boolean("visible", true);
        #[cfg(not(target_os = "android"))]
        let previous_chrome = WindowChrome::read(&self.base_scene);
        if !self.reduce_motion {
            self.transition
                .retarget(&self.base_scene, &target, self.transition_epoch.elapsed());
        }
        self.animation_wake = None;
        self.base_scene = target;
        if let Some(state) = &mut self.state {
            #[cfg(not(target_os = "android"))]
            state
                .window
                .set_title(self.base_scene.text("title").unwrap_or("XRack"));
            let visible = self.base_scene.boolean("visible", true);
            if visible != was_visible {
                state.set_visible(visible);
            }
            #[cfg(not(target_os = "android"))]
            {
                let chrome = WindowChrome::read(&self.base_scene);
                let width = self.base_scene.length("width");
                let height = self.base_scene.length("height");
                let size_changed = width != previous_width || height != previous_height;
                if chrome != previous_chrome || size_changed || !visible {
                    state.finish_client_resize();
                    self.input.cancel_window_move();
                }
                chrome.apply(previous_chrome, &state.window);
                let mut size = state
                    .window
                    .inner_size()
                    .to_logical::<f64>(state.window.scale_factor());
                if size_changed {
                    if width != previous_width
                        && let Some(width) = width.filter(|width| width.is_finite())
                    {
                        size.width = f64::from(width);
                    }
                    if height != previous_height
                        && let Some(height) = height.filter(|height| height.is_finite())
                    {
                        size.height = f64::from(height);
                    }
                    let _ = state.window.request_inner_size(chrome.constrain(size));
                }
            }
        }
        true
    }

    fn apply_window_presentation(&mut self, now: Instant) -> Option<Instant> {
        let state = self.state.as_mut()?;
        let commands = self.component.take_window_commands();
        #[cfg(target_os = "macos")]
        if commands.is_empty() && state.resize_drag.is_some() {
            return None;
        }
        if !commands.is_empty() {
            state.finish_client_resize();
            self.input.cancel_window_move();
        }
        let status = state.presentation.apply(&state.window, commands, now);
        self.component.publish_window_status(status);
        state.presentation.deadline(now)
    }

    fn update_editor(&mut self) {
        self.refresh_editor(true);
    }

    fn publish_focus(&mut self) {
        self.flush_button_press_events();
        self.input.sync_editor(&self.scene);
        let Some(layout) = &self.layout else {
            return;
        };
        let events = self
            .focus_observers
            .sync(&self.input, &self.scene, layout, self.focus_active);
        for event in events {
            event.apply(&mut self.component, &self.scene);
        }
        if (self.component.scene_dirty()
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty())
            && let Some(state) = &self.state
        {
            state.window.request_redraw();
        }
    }

    fn publish_hover(&mut self) {
        let Some(layout) = &self.layout else {
            return;
        };
        let pointer = self
            .hover_pointer
            .zip(self.state.as_ref())
            .map(|(position, state)| {
                let point = position.to_logical::<f32>(state.window.scale_factor());
                Point {
                    x: point.x,
                    y: point.y,
                }
            });
        self.viewport_observers.sync_scroll_activity(
            &self.scene,
            layout,
            &self.input,
            Instant::now(),
        );
        let events = self
            .hover_observers
            .sync(&self.input, &self.scene, layout, pointer);
        self.update_pointer_cursor();
        for event in events {
            event.apply(&mut self.component, &self.scene);
        }
        if (self.component.scene_dirty()
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty())
            && let Some(state) = &self.state
        {
            state.window.request_redraw();
        }
    }

    fn refresh_editor(&mut self, follow_caret: bool) {
        self.refresh_editor_with_caret(if follow_caret {
            CaretFollow::Viewports
        } else {
            CaretFollow::None
        });
    }

    fn refresh_editor_with_caret(&mut self, reveal: CaretFollow) {
        self.flush_swipe_events(None);
        if self.layout.is_some() && self.loader_events_scene.as_ptr() != Rc::as_ptr(&self.scene) {
            let _focus = self.focus_requests.enter();
            let _scroll = self.scroll_requests.enter();
            if xrack_runtime::dispatch_loader_events(&mut self.component, &self.scene) {
                self.loader_events_scene = Rc::downgrade(&self.scene);
            }
        }
        // Capture the result of input before applying queued programmatic moves.
        let scrolled = self.layout.as_ref().map_or_else(Vec::new, |layout| {
            self.input.take_scroll_events(&self.scene, layout)
        });
        let scroll = self.scroll_requests.drain();
        if !scroll.is_empty()
            && let Some(layout) = &self.layout
        {
            if self
                .input
                .apply_scroll_requests(&self.scene, layout, scroll)
            {
                self.prepare_frame(self.transition_epoch.elapsed());
            } else {
                self.viewport_observers.sync(&self.scene, layout);
            }
        }
        let edited_focused = self.apply_focus_and_text_requests();
        if let Some(layout) = &self.layout {
            self.input.remember_focus_scope(&self.scene, layout);
        }
        #[cfg(target_os = "ios")]
        if let Some(target) = &self.input.focused
            && let Some(node) = target.node_in(&self.scene)
            && let Some(native) = self
                .state
                .as_mut()
                .and_then(|state| state.ios_text.as_mut())
            && native.discard_stale_edits(target, &self.input.editor_value(node))
        {
            self.pending_clipboard = None;
            if let Some(editor) = &mut self.input.editor {
                editor.set_preedit(String::new(), None);
            }
        }
        #[cfg(target_os = "ios")]
        if ((reveal == CaretFollow::Viewports || edited_focused)
            && self.reveal_ios_keyboard_caret())
            || self.state.as_ref().is_some_and(|state| {
                matches!(
                    state.ios_keyboard_occlusion,
                    ios::KeyboardOcclusion::Floating(_)
                )
            })
        {
            self.prepare_frame(self.transition_epoch.elapsed());
        }
        self.publish_focus();
        self.publish_hover();
        self.reconcile_pointer();
        for event in scrolled {
            event.apply(&mut self.component, &self.scene);
        }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        self.sync_file_drop();
        self.input.sync_editor(&self.scene);
        if let Some(layout) = &self.layout {
            self.input.reconcile_text_menu(&self.scene, layout);
        }
        let Some(state) = &mut self.state else {
            return;
        };
        #[cfg(target_os = "ios")]
        if let Some(menu) = &state.ios_edit_menu {
            menu.retain_if(|menu| {
                self.ios_application_active
                    && state.window.has_focus()
                    && self.selection_handle_drag.is_none()
                    && self.layout.as_ref().is_some_and(|layout| {
                        self.input
                            .native_text_menu_is_current(menu, &self.scene, layout)
                    })
            });
        }
        let focused = self.input.focused.clone();
        let field = focused.as_ref().and_then(|target| {
            self.scene
                .at_path(&target.path)
                .filter(|node| self.input.is_text_editor(node, target) || spin_editable(node))
        });
        #[cfg(not(target_os = "android"))]
        {
            let purpose = if field.is_some_and(Node::is_password) {
                ImePurpose::Password
            } else {
                ImePurpose::Normal
            };
            if state.ime_purpose != purpose {
                state.window.set_ime_purpose(purpose);
                state.ime_purpose = purpose;
            }
        }
        let list_caret = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.list_typeahead_ime_area(&self.scene, layout));
        let ime_allowed = (field.is_some_and(|node| !node.text_read_only())
            || list_caret.is_some())
            && state.window.has_focus()
            && !self.input.menu_open()
            && self.selection_handle_drag.is_none();
        #[cfg(target_os = "android")]
        let ime_allowed = {
            if state.android_spin_step_focus != focused {
                state.android_spin_step_focus = None;
            }
            ime_allowed && state.android_spin_step_focus.is_none()
        };
        #[cfg(target_os = "ios")]
        let ime_allowed = {
            if state.ios_spin_step_focus != focused {
                state.ios_spin_step_focus = None;
            }
            if state.ios_keyboard_dismissed_focus != focused {
                state.ios_keyboard_dismissed_focus = None;
            }
            ime_allowed
                && self.ios_application_active
                && state.ios_spin_step_focus.is_none()
                && state.ios_keyboard_dismissed_focus.is_none()
        };
        #[cfg(target_os = "ios")]
        if ime_allowed && state.ios_text.is_none() {
            state.ios_text = self
                .events
                .as_ref()
                .and_then(|events| ios_text::TextInput::new(&state.window, events));
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let native_active = false;
        #[cfg(any(target_os = "ios", target_os = "android"))]
        let requested_return_key = field.map_or(ReturnKey::Auto, Node::return_key);
        #[cfg(target_os = "ios")]
        let keyboard = {
            let show_navigation = field.is_some_and(Node::keyboard_navigation);
            let neighbors = if show_navigation || requested_return_key == ReturnKey::Next {
                self.layout.as_ref().map_or([false, false], |layout| {
                    self.input
                        .adjacent_text_fields(&self.scene, layout)
                        .map(|target| target.is_some())
                })
            } else {
                [false, false]
            };
            let return_key = if requested_return_key == ReturnKey::Next && !neighbors[1] {
                ReturnKey::Done
            } else {
                requested_return_key
            };
            ios_text::EditorKeyboard {
                return_key,
                fields: show_navigation.then_some(neighbors),
            }
        };
        #[cfg(target_os = "android")]
        let return_key = match requested_return_key {
            ReturnKey::Next
                if self.layout.as_ref().is_none_or(|layout| {
                    self.input.next_text_field(&self.scene, layout).is_none()
                }) =>
            {
                ReturnKey::Done
            }
            hint => hint,
        };
        #[cfg(target_os = "android")]
        let native_active = if ime_allowed
            && let Some((target, node)) = focused.as_ref().zip(field)
            && let Some(editor) = self.input.editor.as_ref()
            && let Some(native) = state.android_text.as_mut()
        {
            let value = self.input.editor_value(node);
            native.focus(
                target,
                node,
                return_key,
                &value,
                editor.anchor(&value)..editor.caret(&value),
                !editor.preedit.is_empty(),
            )
        } else {
            if let Some(native) = state.android_text.as_mut() {
                native.clear_focus();
            }
            if let Some(editor) = self.input.editor.as_mut() {
                editor.set_preedit(String::new(), None);
            }
            false
        };
        #[cfg(target_os = "ios")]
        let native_active = if ime_allowed
            && let Some((target, node)) = focused.as_ref().zip(field)
            && (is_multiline_text_input(node.kind)
                || combo_editable(node)
                || spin_editable(node)
                || matches!(node.kind, "TextInput" | "LineEdit" | "SearchField")
                || self.input.table_cell_editing(&target.path))
            && let Some(editor) = self.input.editor.as_ref()
        {
            if let Some(native) = &mut state.ios_text {
                let value = self.input.editor_value(node);
                let selection = editor.selection(&value);
                let commands = self.input.text_command_availability(
                    &self.scene,
                    target,
                    node,
                    &value,
                    !selection.is_empty(),
                );
                native.focus(target, node, keyboard, &value, selection, commands)
            } else {
                false
            }
        } else {
            if let Some(native) = &mut state.ios_text {
                native.clear_focus();
            }
            false
        };
        if !native_active {
            state.window.set_ime_allowed(ime_allowed);
        }
        #[cfg(target_os = "ios")]
        if let Some(keyboard) = &state.ios_keyboard {
            keyboard.focus(self.focus_active && self.ios_application_active && !ime_allowed);
        }
        if let Some(layout) = &self.layout
            && let Some(caret) = state
                .renderer
                .text
                .editor_ime_area(
                    &mut self.input,
                    &self.scene,
                    layout,
                    reveal != CaretFollow::None || edited_focused,
                )
                .or(list_caret)
        {
            #[cfg(not(target_os = "android"))]
            state.window.set_ime_cursor_area(
                LogicalPosition::new(caret.x, caret.y),
                LogicalSize::new(caret.width, caret.height),
            );
            #[cfg(target_os = "android")]
            let _ = caret;
        }
        state.window.request_redraw();
        #[cfg(target_os = "ios")]
        self.sync_ios_text_drag();
    }

    #[cfg(target_os = "ios")]
    fn reveal_ios_keyboard_caret(&mut self) -> bool {
        let (Some(state), Some(layout)) = (&self.state, &self.layout) else {
            return false;
        };
        if matches!(state.ios_keyboard_occlusion, ios::KeyboardOcclusion::None)
            || state.ios_keyboard_dismissed_focus.is_some()
            || !self.ios_application_active
            || !state.window.has_focus()
            || !self
                .input
                .focused
                .as_ref()
                .and_then(|target| target.node_in(&self.scene))
                .is_some_and(|node| is_multiline_text_input(node.kind) && !node.text_read_only())
        {
            return false;
        }
        self.input.sync_editor(&self.scene);
        let caret =
            state
                .renderer
                .text
                .editor_caret_rect(&mut self.input, &self.scene, layout, true);
        // Floating keyboards move the viewport's contents temporarily in
        // prepare_frame. Only a docked keyboard changes normal scroll offsets.
        matches!(state.ios_keyboard_occlusion, ios::KeyboardOcclusion::Docked)
            && caret.is_some_and(|caret| {
                self.input
                    .reveal_focused_editor_caret(&self.scene, layout, caret)
            })
    }

    fn apply_focus_and_text_requests(&mut self) -> bool {
        let mut edited_focused = false;
        for request in self.focus_requests.drain_commands() {
            let Some(layout) = &self.layout else {
                break;
            };
            let request = match request {
                xrack_runtime::QueuedFocusRequest::Focus(request) => request,
                xrack_runtime::QueuedFocusRequest::ReplaceText {
                    target,
                    replacement,
                } => {
                    if let Some(target) = self.input.apply_text_replacement(
                        &mut self.component,
                        &self.scene,
                        layout,
                        &target,
                        &replacement,
                        Instant::now(),
                    ) {
                        edited_focused |= self.input.focused.as_ref() == Some(&target);
                        if self.sync_base_scene() {
                            self.prepare_frame(self.transition_epoch.elapsed());
                        }
                    }
                    continue;
                }
            };
            let focused = self.input.focused.clone();
            let was_editing = focused
                .as_ref()
                .is_some_and(|target| self.input.table_cell_editing(&target.path));
            let (command, scrolled) =
                self.input
                    .apply_focus_request_with_scroll(&self.scene, layout, request);
            if scrolled {
                self.sync_base_scene();
                self.prepare_frame(self.transition_epoch.elapsed());
            }
            let editing = self
                .input
                .focused
                .as_ref()
                .is_some_and(|target| self.input.table_cell_editing(&target.path));
            if self.input.focused != focused || was_editing != editing {
                self.pending_clipboard = None;
            }
            if command.is_some_and(|command| self.run_text_command(command))
                && self.sync_base_scene()
            {
                // Later requests from this callback see the committed value and layout.
                self.prepare_frame(self.transition_epoch.elapsed());
            }
        }
        edited_focused
    }

    fn run_text_command(&mut self, command: TextCommand) -> bool {
        self.pending_clipboard = None;
        match command {
            TextCommand::SelectAll
            | TextCommand::SelectWord
            | TextCommand::ClearSelection
            | TextCommand::SetSelection { .. } => {
                self.input.apply_text_selection(&self.scene, command);
            }
            TextCommand::Undo | TextCommand::Redo => {
                return self.replay_text_edit(command == TextCommand::Redo);
            }
            TextCommand::Cut => self.request_clipboard(ClipboardShortcut::Cut),
            TextCommand::Copy => self.request_clipboard(ClipboardShortcut::Copy),
            TextCommand::Paste => self.request_clipboard(ClipboardShortcut::Paste),
        }
        false
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn update_native_file_drag(
        &mut self,
        window: WindowId,
        input: xrack_layout::FileDragInput,
    ) -> bool {
        let Some(layout) = self.layout.as_ref().filter(|_| {
            self.state
                .as_ref()
                .is_some_and(|state| state.window.id() == window)
        }) else {
            return false;
        };
        let update = self.input.file_drag_event(&self.scene, layout, input);
        let dropped = update
            .events
            .iter()
            .any(|event| matches!(event.event, xrack_runtime::DragEvent::FilesDropped(_)));
        if !update.events.is_empty() {
            self.dispatch_drag(update);
            self.refresh_scene();
        }
        dropped
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn install_file_drop(&mut self) {
        let Some(state) = self
            .state
            .as_mut()
            .filter(|state| state.file_drop.is_none())
        else {
            return;
        };
        fn accepts_drop(node: &Node) -> bool {
            node.kind == "DropArea" && node.boolean("accept-files", false)
                || cfg!(target_os = "ios")
                    && (xrack_runtime::is_text_input(node.kind) || combo_editable(node))
                || node.children.iter().any(accepts_drop)
        }
        if accepts_drop(&self.scene) {
            let Some(events) = self.events.clone() else {
                return;
            };
            match file_drop::FileDropBridge::new(&state.window, events) {
                Ok(bridge) => state.file_drop = Some(bridge),
                Err(error) => self.fail(error),
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn sync_file_drop(&mut self) {
        if let Some(bridge) = self
            .state
            .as_ref()
            .and_then(|state| state.file_drop.as_ref())
            && let Some(layout) = &self.layout
        {
            let scope = self.input.file_drop_scope(layout);
            #[cfg(target_os = "ios")]
            self.pending_text_drop.take_if(|(_, drop)| {
                drop.destination(&self.input, &self.scene, layout, &scope)
                    .is_none()
            });
            bridge.sync(self.scene.clone(), layout.clone(), scope);
        }
    }

    fn place_text_caret(
        &mut self,
        point: Point,
        selecting: bool,
        mouse_press: Option<Instant>,
        dragging: bool,
    ) {
        let (Some(state), Some(layout)) = (&self.state, &self.layout) else {
            return;
        };
        let Some(pointer) =
            PointerCaret::from_pressed(&self.input, &self.scene, layout, point, selecting)
        else {
            return;
        };
        let pointer = if dragging {
            pointer.during_drag()
        } else if let Some(now) = mouse_press {
            pointer.mouse_press(&mut self.input, now)
        } else {
            pointer
        };
        if pointer.place(&state.renderer.text, &mut self.input, &self.scene, layout) {
            self.pending_clipboard = None;
        }
    }

    fn move_text_caret(&mut self, motion: TextAreaMotion, selecting: bool) -> bool {
        let (Some(state), Some(layout)) = (&self.state, &self.layout) else {
            return false;
        };
        if !state.renderer.text.move_editor_caret(
            &mut self.input,
            &self.scene,
            layout,
            motion,
            selecting,
        ) {
            return false;
        }
        self.pending_clipboard = None;
        self.update_editor();
        true
    }

    fn scroll_text_area(&mut self, delta: Point) -> Point {
        let (Some(state), Some(layout)) = (&self.state, &self.layout) else {
            return delta;
        };
        state
            .renderer
            .text
            .scroll_editor_area(&mut self.input, &self.scene, layout, delta)
    }

    #[cfg(target_os = "android")]
    fn commit_android_text(&mut self, edit: android_text::Edit) {
        let Some(target) = self.input.focused.clone() else {
            return;
        };
        if self
            .scene
            .at_path(&target.path)
            .is_none_or(|node| node.text_read_only())
        {
            return;
        }
        let accepted = self
            .state
            .as_mut()
            .and_then(|state| state.android_text.as_mut())
            .is_some_and(|native| native.accept(&target, &edit));
        if !accepted {
            return;
        }
        self.apply_android_text(&target, edit);
        if let Some(native) = self
            .state
            .as_mut()
            .and_then(|state| state.android_text.as_mut())
        {
            native.finish_event();
        }
        self.update_editor();
    }

    #[cfg(target_os = "android")]
    fn apply_android_text(&mut self, target: &Target, mut edit: android_text::Edit) {
        if edit.kind == 2 {
            self.submit_editor_action(target);
            return;
        }
        let Some(node) = self.scene.at_path(&target.path) else {
            return;
        };
        if node.kind == "DoubleSpinBox" || node.input_type() == xrack_runtime::InputType::Decimal {
            edit.value = edit
                .value
                .replace(['.', ','], &decimal_separator(node).to_string());
        }
        let changed = self.input.editor_value(node) != edit.value;
        if let Some(editor) = self.input.editor.as_mut() {
            editor.set_preedit(String::new(), None);
        }
        if changed {
            self.edit_text_from(EditAction::ReplaceAll(&edit.value), EditSource::Ime);
        }
        if self.input.focused.as_ref() != Some(target) {
            return;
        }
        let Some(value) = self
            .scene
            .at_path(&target.path)
            .map(|node| self.input.editor_value(node).into_owned())
            .filter(|value| *value == edit.value)
        else {
            return;
        };
        if edit.kind == 1 {
            edit.align_preedit();
        }
        if let Some(editor) = self.input.editor.as_mut() {
            let selection = if edit.kind == 1 {
                edit.anchor
            } else {
                edit.selection.clone()
            };
            editor.set_selection(
                &value,
                selection.start,
                xrack_layout::TextPosition {
                    offset: selection.end,
                    trailing: false,
                },
            );
            if edit.kind == 1 {
                editor.set_preedit(
                    edit.preedit,
                    Some((edit.selection.start, edit.selection.end)),
                );
            }
        }
    }

    #[cfg(target_os = "ios")]
    fn commit_ios_text(&mut self, edit: ios_text::NativeEdit) {
        // Apply pending model and focus changes before accepting a UIKit snapshot.
        self.refresh_dependencies();
        let Some(target) = self.input.focused.clone() else {
            return;
        };
        let Some(editor) = self
            .state
            .as_mut()
            .and_then(|state| state.ios_text.as_mut())
        else {
            return;
        };
        if !editor.accept(&target, &edit) {
            return;
        }
        let committed = match edit.input {
            ios_text::NativeInput::Command(command) => {
                if self.ios_application_active {
                    if self.run_text_command(command) {
                        self.refresh_scene();
                    } else {
                        self.update_editor();
                    }
                }
                return;
            }
            ios_text::NativeInput::Submit => {
                self.submit_editor_action(&target);
                return;
            }
            ios_text::NativeInput::NavigateField { reverse } => {
                self.submit_editor_action_with_navigation(&target, Some(reverse));
                return;
            }
            ios_text::NativeInput::Preedit {
                value,
                marked,
                selection,
            } => {
                self.pending_clipboard = None;
                if let Some(node) = self.scene.at_path(&target.path) {
                    let committed = self.input.editor_value(node).into_owned();
                    if let Some(editor) = &mut self.input.editor {
                        ios_text::apply_marked_text(editor, &committed, &value, marked, selection);
                        self.update_editor();
                    }
                }
                return;
            }
            ios_text::NativeInput::Commit { value, selection } => Some((value, selection)),
            ios_text::NativeInput::Dismissed { committed } => {
                if self.ios_application_active
                    && let Some(state) = &mut self.state
                    && state.window.has_focus()
                {
                    state.ios_keyboard_dismissed_focus = Some(target.clone());
                    if let Some(menu) = &state.ios_edit_menu {
                        menu.dismiss();
                    }
                }
                self.pending_clipboard = None;
                committed
            }
        };
        if let Some(editor) = &mut self.input.editor {
            editor.set_preedit(String::new(), None);
        }
        let Some((mut value, selection)) = committed else {
            self.update_editor();
            return;
        };
        let Some(node) = self.scene.at_path(&target.path) else {
            return;
        };
        if node.kind == "DoubleSpinBox" || node.input_type() == xrack_runtime::InputType::Decimal {
            value = value.replace(['.', ','], &decimal_separator(node).to_string());
        }
        let changes_text = self.input.editor_value(node) != value;
        if changes_text {
            self.edit_text_from(EditAction::ReplaceAll(&value), EditSource::Ime);
        }
        if self.input.focused.as_ref() == Some(&target)
            && let Some(displayed) = self
                .scene
                .at_path(&target.path)
                .map(|node| self.input.editor_value(node).into_owned())
                .filter(|displayed| *displayed == value)
            && let Some(editor) = self.input.editor.as_mut()
        {
            if editor.selection(&displayed) != selection {
                editor.set_selection(
                    &displayed,
                    selection.start,
                    xrack_layout::TextPosition {
                        offset: selection.end,
                        trailing: false,
                    },
                );
                if !changes_text {
                    self.pending_clipboard = None;
                    self.input.break_text_edit_group();
                }
            }
            self.update_editor();
        }
    }

    fn submit_editor_action(&mut self, target: &Target) {
        self.submit_editor_action_with_navigation(target, None);
    }

    fn submit_editor_action_with_navigation(&mut self, target: &Target, navigation: Option<bool>) {
        let Some(node) = target
            .node_in(&self.scene)
            .filter(|_| self.input.focused.as_ref() == Some(target))
        else {
            return;
        };
        if node.text_read_only()
            || self
                .input
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return;
        }
        if self.input.table_cell_editing(&target.path) {
            if navigation.is_none() {
                self.commit_table_edit();
            }
            return;
        }
        let search =
            node.kind == "SearchField" || node.input_type() == xrack_runtime::InputType::Search;
        let return_key = node.return_key();
        if let Some(reverse) = navigation {
            if !node.keyboard_navigation()
                || self.layout.as_ref().is_none_or(|layout| {
                    let [previous, next] = self.input.adjacent_text_fields(&self.scene, layout);
                    (if reverse { previous } else { next }).is_none()
                })
            {
                return;
            }
        }
        let dismiss_after_submit = spin_editable(node)
            || search
            || matches!(
                node.input_type(),
                xrack_runtime::InputType::Number
                    | xrack_runtime::InputType::Decimal
                    | xrack_runtime::InputType::Tel
            );
        if matches!(
            node.kind,
            "TextInput" | "LineEdit" | "TextEdit" | "TextArea"
        ) && !self.component.control_at(
            &self.scene,
            &target.path,
            ControlAction::ValidateTextSubmission,
        ) {
            return;
        }
        let submitted = if spin_editable(node) {
            self.commit_spin();
            true
        } else if node.kind == "SearchField" || combo_editable(node) {
            let Some(action) = self.layout.as_ref().and_then(|layout| {
                self.input
                    .choice_key(&self.scene, layout, ChoiceKey::Accept)
            }) else {
                return;
            };
            if action.action == ControlAction::AcceptComboText && !node.accepts_text_submission() {
                return;
            }
            self.apply_control(action);
            true
        } else {
            self.submit_text(
                target,
                navigation.is_none() && return_key == ReturnKey::Auto,
            )
        };
        if !submitted || self.input.focused.as_ref() != Some(target) {
            return;
        }
        if let Some(reverse) =
            navigation.or_else(|| (return_key == ReturnKey::Next).then_some(false))
            && let Some(revealed) = self.layout.as_ref().and_then(|layout| {
                self.input
                    .focus_adjacent_text_field(&self.scene, layout, reverse)
            })
        {
            if revealed {
                self.relayout();
            } else {
                self.update_editor();
            }
            return;
        }
        if navigation.is_none() && (dismiss_after_submit || return_key != ReturnKey::Auto) {
            self.input.clear_focus();
            self.update_editor();
        }
    }

    fn editor_return(&mut self, event: &KeyEvent) -> bool {
        if event.state != ElementState::Pressed || event.logical_key != Key::Named(NamedKey::Enter)
        {
            return false;
        }
        let Some(target) = self.input.focused.clone() else {
            return false;
        };
        let Some(node) = target.node_in(&self.scene) else {
            return false;
        };
        #[cfg(target_os = "ios")]
        let native = !is_multiline_text_input(node.kind)
            && self
                .state
                .as_ref()
                .and_then(|state| state.ios_text.as_ref())
                .is_some_and(|input| input.editing(&target));
        #[cfg(not(target_os = "ios"))]
        let native = false;
        if !native && node.return_key() == ReturnKey::Auto {
            return false;
        }
        // UIKit's responder forwards hardware Return as a key event. Use the
        // same field validation and dismissal as its native keyboard action.
        if !event.repeat {
            self.submit_editor_action(&target);
        }
        true
    }

    fn submit_text(&mut self, target: &Target, accept_dialog: bool) -> bool {
        if !self
            .scene
            .at_path(&target.path)
            .is_some_and(Node::accepts_text_submission)
        {
            return false;
        }
        if self
            .component
            .dispatch_at(&self.scene, &target.path, "accepted")
        {
            self.refresh_scene();
        } else if accept_dialog
            && let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.dialog_accept(&self.scene, layout))
        {
            self.apply_control(action);
        }
        true
    }

    fn edit_text(&mut self, action: EditAction<'_>) {
        self.edit_text_from(action, EditSource::Keyboard);
    }

    fn edit_text_from(&mut self, action: EditAction<'_>, source: EditSource) {
        self.pending_clipboard = None;
        let Some(target) = self.input.focused.clone() else {
            return;
        };
        let Some(node) = self
            .scene
            .at_path(&target.path)
            .filter(|node| self.input.is_text_editor(node, &target) || spin_editable(node))
        else {
            return;
        };
        if is_spin_box(node.kind) {
            self.input
                .edit_spin_from(node, action, source, Instant::now());
            self.update_editor();
            return;
        }
        if combo_editable(node) {
            self.input.dismiss_choice();
        }
        // UIKit sends the full draft and selection together. Reveal the caret after
        // commit_ios_text restores that selection, not at the end of the replacement.
        let follow_caret = !(cfg!(target_os = "ios")
            && source == EditSource::Ime
            && is_multiline_text_input(node.kind)
            && matches!(action, EditAction::ReplaceAll(_)));
        if self.input.edit_text(
            &mut self.component,
            &self.scene,
            action,
            source,
            Instant::now(),
        ) {
            self.refresh_after_update_with_caret(true, follow_caret);
            if let Some(inspector) = self.component.window_inspector(&self.base_scene)
                && self.input.focused.as_ref() == Some(&target)
            {
                inspector.record_fill(&self.scene, &target.path);
            }
        } else {
            self.refresh_editor(follow_caret);
        }
    }

    fn request_clipboard(&mut self, shortcut: ClipboardShortcut) {
        self.pending_clipboard = None;
        self.input.break_text_edit_group();
        let Some((operation, context)) =
            clipboard::edit_request(&self.input, &self.scene, &shortcut)
        else {
            return;
        };
        let (Some(clipboard), Some(events)) = (&self.clipboard, &self.events) else {
            return;
        };
        match clipboard.borrow_mut().request(events, operation) {
            Ok(id) if !matches!(shortcut, ClipboardShortcut::Copy) => {
                let context = if matches!(shortcut, ClipboardShortcut::Paste) {
                    ClipboardContext::Paste(context)
                } else {
                    ClipboardContext::Cut(context)
                };
                self.pending_clipboard = Some(PendingClipboard::new(id, context));
            }
            Ok(_) => {}
            Err(error) => eprintln!("XRack clipboard: {error}"),
        }
    }

    fn clipboard_reply_ready(&self) -> bool {
        #[cfg(target_os = "ios")]
        return self.ios_application_active && self.focus_active;
        #[cfg(not(target_os = "ios"))]
        true
    }

    fn finish_clipboard(&mut self, reply: ClipboardReply) {
        self.accept_clipboard_reply(reply, self.clipboard_reply_ready());
    }

    fn accept_clipboard_reply(&mut self, reply: ClipboardReply, ready: bool) {
        let Some(request) = self
            .pending_clipboard
            .as_mut()
            .filter(|request| request.id == reply.id)
        else {
            if let Err(error) = reply.result {
                eprintln!("XRack clipboard: {error}");
            }
            return;
        };
        let result = match reply.result {
            Ok(ClipboardValue::Text(text)) if !ready && !text.is_empty() => {
                request.deferred_text = Some(text);
                return;
            }
            Ok(result) => result,
            Err(error) => {
                self.pending_clipboard = None;
                eprintln!("XRack clipboard: {error}");
                return;
            }
        };
        let Some(request) = self.pending_clipboard.take() else {
            return;
        };
        if self.component.scene_dirty() {
            self.refresh_scene();
        }
        match (request.context, result) {
            (ClipboardContext::Paste(context), ClipboardValue::Text(text))
                if !text.is_empty() && context.is_current(&self.input, &self.scene) =>
            {
                self.edit_text_from(EditAction::Insert(&text), EditSource::Atomic);
            }
            (ClipboardContext::Cut(context), ClipboardValue::Written)
                if context.is_current(&self.input, &self.scene) =>
            {
                self.edit_text_from(EditAction::Delete, EditSource::Atomic);
            }
            (ClipboardContext::Table { destination, apply }, ClipboardValue::Text(text))
                if !text.is_empty()
                    && self.layout.as_ref().is_some_and(|layout| {
                        self.input.table_paste_target(&self.scene, layout).as_ref()
                            == Some(&destination)
                    }) =>
            {
                if apply(&text) {
                    self.refresh_scene();
                }
            }
            _ => {}
        }
    }

    #[cfg(target_os = "ios")]
    fn read_ios_clipboard(&mut self, id: u64) {
        if !self
            .pending_clipboard
            .as_ref()
            .is_some_and(|request| request.id == id)
        {
            return;
        }
        if !self.clipboard_reply_ready() {
            self.pending_clipboard = None;
            return;
        }
        let result = clipboard::perform_ios(clipboard::Operation::Read);
        if let Some(events) = &self.events {
            events.post(UserEvent::Clipboard(ClipboardReply { id, result }));
        }
    }

    fn finish_deferred_paste(&mut self) {
        if !self.clipboard_reply_ready() {
            return;
        }
        if let Some(request) = &mut self.pending_clipboard
            && let Some(text) = request.deferred_text.take()
        {
            let reply = ClipboardReply {
                id: request.id,
                result: Ok(ClipboardValue::Text(text)),
            };
            self.finish_clipboard(reply);
        }
    }

    fn table_clipboard_key(&mut self, event: &KeyEvent) -> bool {
        if !(self.modifiers.control_key() || self.modifiers.super_key()) || self.modifiers.alt_key()
        {
            return false;
        }
        let Key::Character(key) = &event.logical_key else {
            return false;
        };
        let pressed = event.state == ElementState::Pressed && !event.repeat;
        let shift = self.modifiers.shift_key();
        let undo_key = key.eq_ignore_ascii_case("z");
        let redo_key = !shift && key.eq_ignore_ascii_case("y");
        if undo_key || redo_key {
            return self.table_history_key(pressed, shift || redo_key);
        }
        if shift {
            return false;
        }
        if key.eq_ignore_ascii_case("v") {
            return self.table_paste_key(pressed);
        }
        if !key.eq_ignore_ascii_case("c") {
            return false;
        }
        let Some((path, columns)) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.table_copy_target(&self.scene, layout))
        else {
            return false;
        };
        if pressed {
            self.pending_clipboard = None;
            if let (Some(clipboard), Some(events)) = (&self.clipboard, &self.events)
                && let Some(text) = self.component.copy_table_at(&self.scene, &path, &columns)
                && let Err(error) = clipboard
                    .borrow_mut()
                    .request(events, clipboard::Operation::Write(text))
            {
                eprintln!("XRack clipboard: {error}");
            }
        }
        true
    }

    fn table_history_key(&mut self, pressed: bool, redo: bool) -> bool {
        let Some(path) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.table_edit_history_target(&self.scene, layout))
        else {
            return false;
        };
        if pressed {
            self.pending_clipboard = None;
            if self.component.replay_table_edit_at(&self.scene, path, redo)
                || self.component.scene_dirty()
            {
                self.refresh_scene();
            }
        }
        true
    }

    fn table_paste_key(&mut self, pressed: bool) -> bool {
        let Some(destination) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.table_paste_target(&self.scene, layout))
        else {
            return false;
        };
        if pressed {
            self.pending_clipboard = None;
            if let (Some(clipboard), Some(events)) = (&self.clipboard, &self.events)
                && let Some(apply) = self.component.prepare_table_paste(
                    &self.scene,
                    &destination.path,
                    destination.row,
                    destination.columns.clone(),
                )
            {
                match clipboard
                    .borrow_mut()
                    .request(events, clipboard::Operation::Read)
                {
                    Ok(id) => {
                        self.pending_clipboard = Some(PendingClipboard::new(
                            id,
                            ClipboardContext::Table { destination, apply },
                        ));
                    }
                    Err(error) => eprintln!("XRack clipboard: {error}"),
                }
            }
        }
        true
    }

    fn text_shortcut(&mut self, key: &str, shift: bool, repeat: bool) -> bool {
        let clipboard = if key.eq_ignore_ascii_case("c") {
            Some(ClipboardShortcut::Copy)
        } else if key.eq_ignore_ascii_case("x") {
            Some(ClipboardShortcut::Cut)
        } else if key.eq_ignore_ascii_case("v") {
            Some(ClipboardShortcut::Paste)
        } else {
            None
        };
        if let Some(shortcut) = clipboard {
            if !repeat {
                self.request_clipboard(shortcut);
            }
            return true;
        }
        if key.eq_ignore_ascii_case("z") || key.eq_ignore_ascii_case("y") {
            let redo = shift || key.eq_ignore_ascii_case("y");
            if self.replay_text_edit(redo) {
                self.refresh_scene();
            }
            return true;
        }
        false
    }

    fn replay_text_edit(&mut self, redo: bool) -> bool {
        self.pending_clipboard = None;
        self.input
            .replay_text_edit(&mut self.component, &self.scene, redo)
    }

    #[cfg(any(target_os = "android", test))]
    fn commit_text_preedit(&mut self) {
        let Some(target) = self.input.focused.clone() else {
            return;
        };
        let Some(node) = self.scene.at_path(&target.path) else {
            return;
        };
        let Some(editor) = self
            .input
            .editor
            .as_ref()
            .filter(|editor| !editor.preedit.is_empty())
        else {
            return;
        };
        let value = self.input.editor_value(node);
        let selection = editor.selection(&value);
        let position = selection.start + editor.preedit_cursor.unwrap_or(editor.preedit.len());
        let preedit = editor.preedit.clone();
        let changes_value = value[selection] != preedit;
        if let Some(editor) = self.input.editor.as_mut() {
            editor.set_preedit(String::new(), None);
        }
        if changes_value {
            self.edit_text_from(EditAction::Insert(&preedit), EditSource::Ime);
        }
        if self.input.focused.as_ref() == Some(&target)
            && let Some(value) = self
                .scene
                .at_path(&target.path)
                .map(|node| self.input.editor_value(node).into_owned())
            && let Some(editor) = self.input.editor.as_mut()
        {
            editor.place_caret(
                &value,
                xrack_layout::TextPosition {
                    offset: position.min(value.len()),
                    trailing: false,
                },
                false,
            );
            self.update_editor();
        }
    }

    #[cfg(any(target_os = "android", test))]
    fn finish_committed_suggestion(&mut self) -> bool {
        let same_text = self
            .input
            .focused
            .as_ref()
            .and_then(|target| {
                let node = self.scene.at_path(&target.path)?;
                let editor = self.input.editor.as_ref()?;
                let value = self.input.editor_value(node);
                Some(editor.has_committed_suggestion(&value))
            })
            .unwrap_or(false);
        if same_text {
            self.commit_text_preedit();
        }
        same_text
    }

    fn text_key(&mut self, event: &KeyEvent) -> bool {
        // NativeActivity also delivers keys left unconsumed by the input method.
        // Finish its suggestion region before applying those editing keys.
        #[cfg(target_os = "android")]
        if event.state == ElementState::Pressed
            && matches!(
                event.logical_key,
                Key::Character(_)
                    | Key::Named(
                        NamedKey::Space
                            | NamedKey::Enter
                            | NamedKey::Backspace
                            | NamedKey::Delete
                            | NamedKey::ArrowLeft
                            | NamedKey::ArrowRight
                            | NamedKey::ArrowUp
                            | NamedKey::ArrowDown
                            | NamedKey::Home
                            | NamedKey::End
                            | NamedKey::PageUp
                            | NamedKey::PageDown
                    )
            )
        {
            let focused = self.input.focused.clone();
            self.commit_text_preedit();
            if self.input.focused != focused {
                return true;
            }
        }
        let Some(target) = self.input.focused.clone() else {
            return false;
        };
        let Some(node) = self
            .scene
            .at_path(&target.path)
            .filter(|node| self.input.is_text_editor(node, &target) || spin_editable(node))
        else {
            return false;
        };
        if target.text_link_index().is_some()
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::Enter | NamedKey::Space)
            )
        {
            return false;
        }
        let spin = is_spin_box(node.kind);
        if self
            .input
            .editor
            .as_ref()
            .is_some_and(|edit| !edit.preedit.is_empty())
        {
            return true;
        }
        if event.state != ElementState::Pressed {
            return false;
        }
        if combo_editable(node)
            && (match event.logical_key {
                Key::Named(NamedKey::ArrowLeft) => Some(NavigationKey::Left),
                Key::Named(NamedKey::ArrowRight) => Some(NavigationKey::Right),
                _ => None,
            })
            .is_some_and(|key| self.input.combo_completion_accepts(key))
            && self.modifiers.is_empty()
            && !event.repeat
            && self
                .input
                .accept_combo_completion(&mut self.component, &self.scene, Instant::now())
        {
            self.pending_clipboard = None;
            self.refresh_scene();
            return true;
        }
        let shift = self.modifiers.shift_key();
        let shortcut = (self.modifiers.control_key() || self.modifiers.super_key())
            && !self.modifiers.alt_key();
        let multiline = is_multiline_text_input(node.kind);
        if shortcut
            && let Key::Character(key) = &event.logical_key
            && self.text_shortcut(key, shift, event.repeat)
        {
            return true;
        }
        let word_key = match event.logical_key {
            Key::Named(NamedKey::ArrowLeft) => Some("ArrowLeft"),
            Key::Named(NamedKey::ArrowRight) => Some("ArrowRight"),
            Key::Named(NamedKey::Backspace) => Some("Backspace"),
            Key::Named(NamedKey::Delete) => Some("Delete"),
            _ => None,
        };
        if let Some(action) = word_key.and_then(|key| {
            EditAction::word_shortcut(
                key,
                self.input_modifiers(),
                cfg!(any(target_os = "macos", target_os = "ios")),
            )
        }) {
            if let Some((motion, selecting)) = TextAreaMotion::from_word_action(action)
                && self.move_text_caret(motion, selecting)
            {
                return true;
            }
            self.edit_text(action);
            return true;
        }
        if let Some(motion) = navigation_key_value(&event.logical_key)
            .and_then(|key| TextAreaMotion::from_navigation(key, shortcut))
            && self.move_text_caret(motion, shift)
        {
            return true;
        }
        let action = match &event.logical_key {
            Key::Named(NamedKey::Backspace) => Some(EditAction::Backspace),
            Key::Named(NamedKey::Delete) => Some(EditAction::Delete),
            Key::Named(NamedKey::ArrowLeft) => Some(EditAction::Left(shift)),
            Key::Named(NamedKey::ArrowRight) => Some(EditAction::Right(shift)),
            Key::Named(NamedKey::Home) => Some(EditAction::Home(shift)),
            Key::Named(NamedKey::End) => Some(EditAction::End(shift)),
            Key::Character(key) if shortcut && key.eq_ignore_ascii_case("a") => {
                Some(EditAction::SelectAll)
            }
            Key::Named(NamedKey::Escape) if spin => {
                self.input.discard_spin_draft();
                self.update_editor();
                return true;
            }
            Key::Named(NamedKey::Enter) if spin => {
                self.commit_spin();
                self.update_editor();
                return true;
            }
            Key::Named(NamedKey::Enter) if multiline => Some(EditAction::Insert("\n")),
            Key::Named(NamedKey::Enter) => {
                if !event.repeat {
                    self.submit_text(&target, true);
                }
                return true;
            }
            _ if !shortcut
                && !matches!(event.logical_key, Key::Named(key) if key != NamedKey::Space) =>
            {
                event.text.as_deref().map(EditAction::Insert)
            }
            _ => None,
        };
        if let Some(action) = action {
            self.edit_text(action);
            true
        } else {
            false
        }
    }
    fn apply_control(&mut self, mut input: ControlInput) {
        if input.action == ControlAction::Dialog(xrack_runtime::DialogRole::Accept)
            && let Some(session) = self
                .scene
                .at_path(&input.path)
                .and_then(Node::picker_session_id)
            && let Some(wheel) = self.layout.as_ref().and_then(|layout| {
                self.input
                    .settle_time_picker_wheel(&self.scene, layout, &input.path)
            })
        {
            self.apply_control(wheel);
            // The wheel writes a new draft. Confirm against that draft's scene
            // without letting the old activation reach a replacement dialog.
            if self
                .scene
                .at_path(&input.path)
                .and_then(Node::picker_session_id)
                != Some(session)
            {
                return;
            }
        }
        if let ControlAction::Spin(action) = input.action {
            let Some(action) = self
                .input
                .resolve_spin_action(&self.scene, &input.path, action)
            else {
                return;
            };
            input.action = ControlAction::Spin(action);
            if self.input.focused.as_ref().is_some_and(|target| {
                target.path == input.path && target.node_in(&self.scene).is_some()
            }) {
                self.input.discard_spin_draft();
            }
        }
        let search = self
            .scene
            .at_path(&input.path)
            .is_some_and(|node| node.kind == "SearchField");
        let changed = match input.action {
            ControlAction::TextCommand(command) => {
                self.input.focused.as_ref().is_some_and(|target| {
                    target.path == input.path
                        && target.row_key == self.scene.row_key_at(&target.path)
                }) && self.run_text_command(command)
            }
            ControlAction::Select(index) if search => {
                self.pending_clipboard = None;
                self.input.select_search_suggestion(
                    &mut self.component,
                    &self.scene,
                    index,
                    Instant::now(),
                )
            }
            ControlAction::ClearSearch if search => {
                self.pending_clipboard = None;
                self.input.clear_search(
                    &mut self.component,
                    &self.scene,
                    &input.path,
                    Instant::now(),
                )
            }
            action => self.component.control_at(&self.scene, &input.path, action),
        };
        if changed || self.component.scene_dirty() {
            self.refresh_scene();
        } else if self
            .scene
            .at_path(&input.path)
            .is_some_and(|node| node.list.is_some() || node.kind == "TabBar")
        {
            self.relayout();
        } else {
            self.update_editor();
        }
    }

    fn apply_accessible_layout_change(
        &mut self,
        input: ControlInput,
        reveal: impl FnOnce(&mut Interaction, &Node, &LayoutNode, &[usize]) -> bool,
    ) {
        if !self
            .component
            .control_at(&self.scene, &input.path, input.action)
        {
            self.refresh_after_update_with_caret(false, false);
            return;
        }
        self.sync_base_scene();
        self.prepare_frame(self.transition_epoch.elapsed());
        // Reveal the affected control before UIKit receives the new accessibility tree.
        if let Some(layout) = &self.layout
            && reveal(&mut self.input, &self.scene, layout, &input.path)
        {
            self.prepare_frame(self.transition_epoch.elapsed());
        }
        self.layout_changed(false);
    }

    fn commit_spin(&mut self) {
        if let Some(input) = self.input.commit_spin_draft() {
            self.apply_control(input);
        }
    }

    fn apply_split_resize(&mut self, input: SplitResize) {
        let changed = if let Some(sizes) = &input.sizes {
            self.component
                .resize_split_sizes_at(&self.scene, &input.path, sizes)
        } else {
            self.component
                .resize_split_at(&self.scene, &input.path, &input.weights)
        };
        if changed {
            self.refresh_scene();
        }
    }

    fn input_modifiers(&self) -> xrack_runtime::KeyModifiers {
        let control = self.modifiers.control_key();
        let meta = self.modifiers.super_key();
        xrack_runtime::KeyModifiers {
            control,
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
            meta,
            primary: if cfg!(any(target_os = "macos", target_os = "ios")) {
                meta
            } else {
                control
            },
        }
    }

    fn deliver_pointer(&mut self, delivery: PointerDelivery) {
        delivery.apply(&mut self.component, &self.scene);
        if (self.component.scene_dirty()
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty())
            && let Some(state) = &self.state
        {
            state.window.request_redraw();
        }
    }

    fn start_pointer(&mut self, contact: PointerContact, point: Point, now: Instant) {
        if self.input.direct_pointer_claimed() || self.input.drag_position().is_some() {
            return;
        }
        if self
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.contact == contact)
        {
            return;
        }
        let Some((mut sequence, delivery)) = self.new_pointer_sequence(contact, point, now) else {
            return;
        };
        if contact == PointerContact::Mouse(PointerButton::Primary)
            && !self.input.owns_pointer_press(sequence.target())
        {
            return;
        }
        if self.pointer_capture.is_some() {
            self.cancel_pointer();
        }
        if matches!(contact, PointerContact::Touch(..)) {
            if self
                .additional_touch_captures
                .iter()
                .any(|capture| capture.sequence.target() == sequence.target())
            {
                sequence.disable_press_and_hold();
                self.input.cancel_pointer_activation(sequence.target());
            }
            self.advance_touch_finger_id();
        }
        self.pointer_capture = Some(CapturedPointer { contact, sequence });
        self.deliver_pointer(delivery);
        self.refresh_dependencies();
    }

    fn new_pointer_sequence(
        &self,
        contact: PointerContact,
        point: Point,
        now: Instant,
    ) -> Option<(PointerSequence, PointerDelivery)> {
        let (button, is_touch) = contact.payload();
        let event = PointerEvent {
            button,
            is_touch,
            touch_finger_id: if is_touch {
                (self.next_touch_finger_id > 0).then_some(self.next_touch_finger_id)?
            } else {
                0
            },
            modifiers: self.input_modifiers(),
            ..Default::default()
        };
        self.input
            .begin_pointer_sequence(&self.scene, self.layout.as_ref()?, point, event, now)
    }

    fn advance_touch_finger_id(&mut self) {
        self.next_touch_finger_id = self.next_touch_finger_id.checked_add(1).unwrap_or_default();
    }

    fn accepts_additional_touch(&self, target: &Target) -> bool {
        self.scene
            .at_path(&target.path)
            .is_some_and(|node| node.boolean("multi-touch", false))
    }

    fn start_additional_touch(&mut self, contact: PointerContact, point: Point, now: Instant) {
        if self
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.contact == contact)
            || self
                .additional_touch_captures
                .iter()
                .any(|capture| capture.contact == contact)
        {
            return;
        }
        let Some((mut sequence, delivery)) = self.new_pointer_sequence(contact, point, now) else {
            return;
        };
        if !self.accepts_additional_touch(sequence.target()) {
            return;
        }
        if let Some(primary) = &mut self.pointer_capture
            && matches!(primary.contact, PointerContact::Touch(..))
            && primary.sequence.target() == sequence.target()
        {
            primary.sequence.disable_press_and_hold();
            self.input
                .cancel_pointer_activation(primary.sequence.target());
        }
        sequence.disable_press_and_hold();
        self.advance_touch_finger_id();
        self.additional_touch_captures
            .push(CapturedPointer { contact, sequence });
        self.deliver_pointer(delivery);
        self.refresh_dependencies();
    }

    fn advance_pointer(&mut self, contact: PointerContact, point: Point, kind: PointerEventKind) {
        let additional = self
            .additional_touch_captures
            .iter()
            .position(|capture| capture.contact == contact);
        let mut capture = if let Some(index) = additional {
            self.additional_touch_captures.remove(index)
        } else if let Some(capture) = self
            .pointer_capture
            .take_if(|capture| capture.contact == contact)
        {
            capture
        } else {
            return;
        };
        let modifiers = self.input_modifiers();
        let delivery = if kind == PointerEventKind::Cancel
            || additional.is_some() && !self.accepts_additional_touch(capture.sequence.target())
        {
            None
        } else {
            self.layout.as_ref().and_then(|layout| {
                capture
                    .sequence
                    .sample(&self.input, &self.scene, layout, point, kind, modifiers)
            })
        };
        if let Some(delivery) = delivery {
            if kind == PointerEventKind::Move {
                if let Some(index) = additional {
                    self.additional_touch_captures.insert(index, capture);
                } else {
                    self.pointer_capture = Some(capture);
                }
            } else if capture.sequence.press_and_hold_triggered() {
                self.last_pointer_click = None;
                self.last_button_click = None;
                self.input
                    .cancel_pointer_activation(capture.sequence.target());
            }
            self.deliver_pointer(delivery);
        } else if additional.is_some() {
            self.deliver_pointer_cancel(capture.sequence);
        } else {
            self.cancel_captured_pointer(capture);
        }
        self.refresh_dependencies();
    }

    fn cancel_pointer(&mut self) {
        self.last_pointer_click = None;
        self.last_button_click = None;
        if let Some(capture) = self.pointer_capture.take() {
            self.cancel_captured_pointer(capture);
        }
    }

    fn cancel_all_pointers(&mut self) {
        self.touch_hold = None;
        self.selection_handle_drag = None;
        self.input.set_touch_selection_handles(false);
        self.cancel_pointer();
        for capture in std::mem::take(&mut self.additional_touch_captures) {
            self.deliver_pointer_cancel(capture.sequence);
        }
    }

    fn cancel_pointer_contact(&mut self, contact: PointerContact) {
        if self
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.contact == contact)
        {
            self.cancel_pointer();
        } else if let Some(index) = self
            .additional_touch_captures
            .iter()
            .position(|capture| capture.contact == contact)
        {
            let capture = self.additional_touch_captures.remove(index);
            self.deliver_pointer_cancel(capture.sequence);
        }
    }

    fn cancel_captured_pointer(&mut self, capture: CapturedPointer) {
        self.last_pointer_click = None;
        self.last_button_click = None;
        if capture.contact.payload().0 == PointerButton::Primary {
            self.input
                .cancel_pointer_activation(capture.sequence.target());
        }
        self.deliver_pointer_cancel(capture.sequence);
    }

    fn deliver_pointer_cancel(&mut self, sequence: PointerSequence) {
        let event = sequence.cancel(self.input_modifiers());
        self.deliver_pointer(event);
    }

    fn reconcile_pointer(&mut self) {
        if self.pointer_capture.as_ref().is_some_and(|capture| {
            self.input.direct_pointer_claimed()
                || capture.contact == PointerContact::Mouse(PointerButton::Primary)
                    && !self.input.owns_pointer_press(capture.sequence.target())
                || self.layout.as_ref().is_none_or(|layout| {
                    !capture.sequence.is_live(&self.input, &self.scene, layout)
                })
        }) {
            self.cancel_pointer();
        } else if let (Some(capture), Some(layout)) = (&mut self.pointer_capture, &self.layout) {
            capture.sequence.constrain_press_and_hold(layout, None);
        }
        let mut index = 0;
        while index < self.additional_touch_captures.len() {
            let sequence = &self.additional_touch_captures[index].sequence;
            if self.layout.as_ref().is_some_and(|layout| {
                sequence.is_live(&self.input, &self.scene, layout)
                    && self.accepts_additional_touch(sequence.target())
            }) {
                index += 1;
            } else {
                let capture = self.additional_touch_captures.remove(index);
                self.deliver_pointer_cancel(capture.sequence);
            }
        }
    }

    fn tick_pointer_hold(&mut self, now: Instant) {
        if let (Some(capture), Some(layout)) = (&mut self.pointer_capture, &self.layout)
            && capture.sequence.advance_press_and_hold(
                &self.input,
                &self.scene,
                layout,
                &mut self.component,
                now,
            )
        {
            self.last_pointer_click = None;
            self.last_button_click = None;
            self.refresh_dependencies();
        }
    }

    fn move_mouse_pointer(&mut self, point: Point) {
        let Some(contact @ PointerContact::Mouse(_)) =
            self.pointer_capture.as_ref().map(|capture| capture.contact)
        else {
            return;
        };
        let claimed = self.input.direct_pointer_claimed()
            || self.input.drag_position().is_some()
            || self.input.table_drag_selection_claimed()
            || self.input.table_row_move_claimed();
        let kind = if claimed {
            PointerEventKind::Cancel
        } else {
            PointerEventKind::Move
        };
        self.advance_pointer(contact, point, kind);
    }

    fn release_pointer(&mut self, contact: PointerContact, point: Point, now: Instant) {
        if !self
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.contact == contact)
        {
            return;
        }
        if self.scroll_contact.map(PointerContact::for_scroll) == Some(contact) {
            self.move_direct_scroll(point, now);
        }
        let kind = if self.input.direct_pointer_claimed() {
            PointerEventKind::Cancel
        } else {
            if let (Some(capture), Some(layout)) = (&mut self.pointer_capture, &self.layout) {
                capture
                    .sequence
                    .constrain_press_and_hold(layout, Some(point));
            }
            self.tick_pointer_hold(now);
            PointerEventKind::Up
        };
        self.advance_pointer(contact, point, kind);
    }

    fn wheel_event(&self, delta: Point, lines: Option<Point>) -> xrack_runtime::WheelEvent {
        xrack_runtime::WheelEvent {
            delta_x: xrack_runtime::Length(f64::from(delta.x)),
            delta_y: xrack_runtime::Length(f64::from(delta.y)),
            lines_x: lines.map_or(0.0, |lines| f64::from(lines.x)),
            lines_y: lines.map_or(0.0, |lines| f64::from(lines.y)),
            is_line: lines.is_some(),
            modifiers: self.input_modifiers(),
            ..Default::default()
        }
    }

    fn dispatch_wheel(&mut self, event: xrack_runtime::WheelEvent) -> bool {
        let Some(route) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.wheel_route(&self.scene, layout, event))
        else {
            return false;
        };
        let result = route.dispatch(&mut self.component, &self.scene);
        self.refresh_dependencies();
        result == EventResult::Accept
    }

    fn adjust_wheel(&mut self, event: xrack_runtime::WheelEvent, now: Instant) -> bool {
        let Some(adjustment) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.wheel_adjustment(&self.scene, layout, event, now))
        else {
            return false;
        };
        let changed = match adjustment {
            WheelAdjustment::Pending => return true,
            WheelAdjustment::Range(input) => {
                self.component
                    .adjust_range_at(&self.scene, &input.path, input.action)
            }
            WheelAdjustment::Spin(input) => {
                let changed = self
                    .component
                    .control_at(&self.scene, &input.path, input.action);
                if changed
                    && self
                        .input
                        .focused
                        .as_ref()
                        .is_some_and(|focus| focus.path == input.path)
                {
                    self.input.discard_spin_draft();
                }
                changed
            }
        };
        if changed {
            self.refresh_scene();
        }
        changed
    }

    fn commit_table_edit(&mut self) -> TableEditCommit {
        self.pending_clipboard = None;
        let result = self
            .input
            .finish_table_edit(&mut self.component, &self.scene);
        if result == TableEditCommit::Changed {
            self.refresh_scene();
        } else {
            self.update_editor();
        }
        result
    }

    fn cancel_table_edit(&mut self) {
        if self
            .input
            .editor
            .as_ref()
            .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return;
        }
        if self.input.cancel_table_edit() {
            self.pending_clipboard = None;
            self.update_editor();
        }
    }

    fn continue_table_edit(&mut self, origin: &TableEditTarget, backward: bool) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let Some(target) = self.input.next_table_edit_target(
            &self.component,
            &self.scene,
            layout,
            origin,
            backward,
        ) else {
            return false;
        };
        if self
            .input
            .apply_scroll_requests(&self.scene, layout, [target.scroll_request()])
        {
            self.prepare_frame(self.transition_epoch.elapsed());
        }
        let opened = self.layout.as_ref().is_some_and(|layout| {
            self.input
                .begin_table_edit_target(&self.scene, layout, &target)
        });
        if opened {
            self.update_editor();
        }
        opened
    }

    fn table_edit_key(&mut self, event: &KeyEvent) -> bool {
        let active = self
            .input
            .focused
            .as_ref()
            .is_some_and(|target| self.input.table_cell_editing(&target.path));
        if active
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::Escape | NamedKey::Enter)
            )
        {
            if event.state == ElementState::Pressed
                && !event.repeat
                && self
                    .input
                    .editor
                    .as_ref()
                    .is_none_or(|editor| editor.preedit.is_empty())
            {
                if event.logical_key == Key::Named(NamedKey::Enter) {
                    self.commit_table_edit();
                } else {
                    self.cancel_table_edit();
                }
            }
            return true;
        }
        if active || event.state != ElementState::Pressed || event.repeat {
            return false;
        }
        let edit_key = match event.logical_key {
            Key::Named(NamedKey::F2) => true,
            Key::Named(NamedKey::Enter) => self
                .input
                .focused
                .as_ref()
                .and_then(|target| self.scene.at_path(&target.path))
                .is_some_and(|node| matches!(node.kind, "TableView" | "TableCell")),
            _ => false,
        };
        let shortcut = (self.modifiers.control_key() || self.modifiers.super_key())
            && !self.modifiers.alt_key();
        let typed = event.text.as_deref().filter(|text| {
            !shortcut
                && matches!(
                    event.logical_key,
                    Key::Character(_) | Key::Named(NamedKey::Space)
                )
                && !text.is_empty()
                && text.chars().all(|ch| !ch.is_control())
        });
        let started = self.layout.as_ref().is_some_and(|layout| {
            if edit_key {
                self.input.begin_table_edit(&self.scene, layout)
            } else {
                typed.is_some() && self.input.begin_table_typing(&self.scene, layout)
            }
        });
        if started {
            self.pending_clipboard = None;
            if let Some(text) = typed {
                self.edit_text(EditAction::Insert(text));
            } else {
                self.update_editor();
            }
            return true;
        }
        false
    }

    fn spin_key(&mut self, event: &KeyEvent) -> bool {
        let Some(target) = self.input.focused.as_ref() else {
            return false;
        };
        let Some(node) = self
            .scene
            .interactive_at_path(&target.path)
            .filter(|node| is_spin_box(node.kind))
        else {
            return false;
        };
        let action = match event.logical_key {
            Key::Named(NamedKey::ArrowUp) => SpinAction::Increase,
            Key::Named(NamedKey::ArrowDown) => SpinAction::Decrease,
            Key::Named(NamedKey::Home) if !spin_editable(node) => SpinAction::Minimum,
            Key::Named(NamedKey::End) if !spin_editable(node) => SpinAction::Maximum,
            _ => return false,
        };
        if event.state == ElementState::Pressed
            && !node.text_read_only()
            && !self
                .input
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.preedit.is_empty())
        {
            self.apply_control(ControlInput {
                path: target.path.clone(),
                action: ControlAction::Spin(action),
            });
        }
        true
    }

    fn context_menu_key(&mut self, event: &KeyEvent) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        if event.state == ElementState::Pressed
            && !event.repeat
            && (event.logical_key == Key::Named(NamedKey::ContextMenu)
                || event.logical_key == Key::Named(NamedKey::F10)
                    && self.modifiers == ModifiersState::SHIFT)
            && self.input.open_context_menu(&self.scene, layout, true)
        {
            self.pending_clipboard = None;
            self.update_editor();
            return true;
        }
        false
    }

    fn shortcut_key(&mut self, event: &KeyEvent) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }
        let Some(layout) = &self.layout else {
            return false;
        };
        let key = event.unmodified_key.clone();
        let key = match key {
            Key::Character(text) => {
                let mut chars = text.chars();
                let Some(ch) = chars.next().filter(|_| chars.next().is_none()) else {
                    self.input.cancel_shortcut_sequence();
                    return false;
                };
                ShortcutKey::Character(ch.to_ascii_uppercase())
            }
            Key::Named(NamedKey::F1) => ShortcutKey::Function(1),
            Key::Named(NamedKey::F2) => ShortcutKey::Function(2),
            Key::Named(NamedKey::F3) => ShortcutKey::Function(3),
            Key::Named(NamedKey::F4) => ShortcutKey::Function(4),
            Key::Named(NamedKey::F5) => ShortcutKey::Function(5),
            Key::Named(NamedKey::F6) => ShortcutKey::Function(6),
            Key::Named(NamedKey::F7) => ShortcutKey::Function(7),
            Key::Named(NamedKey::F8) => ShortcutKey::Function(8),
            Key::Named(NamedKey::F9) => ShortcutKey::Function(9),
            Key::Named(NamedKey::F10) => ShortcutKey::Function(10),
            Key::Named(NamedKey::F11) => ShortcutKey::Function(11),
            Key::Named(NamedKey::F12) => ShortcutKey::Function(12),
            Key::Named(NamedKey::Escape) => ShortcutKey::Escape,
            Key::Named(
                NamedKey::Control
                | NamedKey::Alt
                | NamedKey::AltGraph
                | NamedKey::Shift
                | NamedKey::Super
                | NamedKey::Meta
                | NamedKey::Hyper,
            ) => return false,
            _ => {
                self.input.cancel_shortcut_sequence();
                return false;
            }
        };
        let modifiers = ShortcutModifiers {
            ctrl: self.modifiers.control_key(),
            alt: self.modifiers.alt_key(),
            shift: self.modifiers.shift_key(),
            super_key: self.modifiers.super_key(),
        };
        if self.input.editor.is_some() && !self.input.menu_open() {
            let editing_shortcut = (modifiers.ctrl || modifiers.super_key)
                && !modifiers.alt
                && matches!(
                    key,
                    ShortcutKey::Character('A' | 'C' | 'V' | 'X' | 'Y' | 'Z')
                );
            // Some layouts report AltGr as Ctrl+Alt while producing ordinary text.
            let text_entry = modifiers.ctrl
                && modifiers.alt
                && event
                    .text
                    .as_ref()
                    .is_some_and(|text| text.chars().any(|ch| !ch.is_control()));
            if editing_shortcut || text_entry {
                self.input.cancel_shortcut_sequence();
                return false;
            }
        }
        let Some(key) = MenuShortcut::new(key, modifiers) else {
            self.input.cancel_shortcut_sequence();
            return false;
        };
        match self
            .input
            .shortcut_press(&self.scene, layout, key, event.repeat, Instant::now())
        {
            ShortcutOutcome::Unmatched => false,
            ShortcutOutcome::Consumed => true,
            ShortcutOutcome::Activate(action) => {
                self.pending_clipboard = None;
                self.input.close_menu(&self.scene, layout);
                self.apply_control(action);
                true
            }
            ShortcutOutcome::Ambiguous(actions) => {
                let mut handled = false;
                for action in actions {
                    handled |= self
                        .component
                        .control_at(&self.scene, &action.path, action.action);
                }
                if handled {
                    self.refresh_scene();
                }
                true
            }
        }
    }

    fn menu_key(&mut self, event: &KeyEvent) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let open = self.input.menu_open();
        if event.state != ElementState::Pressed {
            return open;
        }
        let key = match event.logical_key {
            Key::Named(NamedKey::ArrowUp) => MenuKey::Up,
            Key::Named(NamedKey::ArrowDown) => MenuKey::Down,
            Key::Named(NamedKey::ArrowLeft) => MenuKey::Left,
            Key::Named(NamedKey::ArrowRight) => MenuKey::Right,
            Key::Named(NamedKey::Home) => MenuKey::Home,
            Key::Named(NamedKey::End) => MenuKey::End,
            Key::Named(NamedKey::Enter | NamedKey::Space) => MenuKey::Accept,
            Key::Named(NamedKey::Escape) => MenuKey::Escape,
            _ => return open,
        };
        let header = self
            .input
            .focused
            .as_ref()
            .and_then(|target| self.scene.at_path(&target.path))
            .is_some_and(|node| node.kind == "Menu");
        if !open && (!header || matches!(key, MenuKey::Escape | MenuKey::Home | MenuKey::End)) {
            return false;
        }
        self.pending_clipboard = None;
        if let Some(action) = self.input.menu_key(&self.scene, layout, key) {
            self.apply_control(action);
        } else {
            self.update_editor();
        }
        true
    }

    fn list_typeahead_key(&mut self, event: &KeyEvent) -> bool {
        if event.state == ElementState::Pressed
            && !event.repeat
            && !(self.modifiers.control_key()
                || self.modifiers.super_key()
                || self.modifiers.alt_key())
            && let Key::Character(text) = &event.logical_key
        {
            return self.list_typeahead_text(event.text.as_deref().unwrap_or(text));
        }
        false
    }

    fn list_typeahead_text(&mut self, text: &str) -> bool {
        let Some(action) = self.layout.as_ref().and_then(|layout| {
            self.input
                .list_typeahead(&self.scene, layout, text, Instant::now())
        }) else {
            return false;
        };
        self.apply_control(action);
        true
    }

    fn choice_key(&mut self, event: &KeyEvent) -> bool {
        let open = self.input.choice_open();
        let focused = self
            .input
            .focused
            .as_ref()
            .and_then(|target| self.scene.at_path(&target.path));
        let search = focused.is_some_and(|node| node.kind == "SearchField");
        let editing = search || focused.is_some_and(combo_editable);
        if event.state != ElementState::Pressed {
            return open;
        }
        if !editing
            && focused.is_some_and(|node| node.kind == "ComboBox")
            && !(self.modifiers.control_key()
                || self.modifiers.super_key()
                || self.modifiers.alt_key())
            && let Key::Character(text) = &event.logical_key
            && let Some(layout) = &self.layout
        {
            if let Some(action) =
                self.input
                    .choice_typeahead(&self.scene, layout, text, Instant::now())
            {
                self.apply_control(action);
            } else if open {
                self.update_editor();
            }
            return true;
        }
        if editing
            && self
                .input
                .editor
                .as_ref()
                .is_some_and(|edit| !edit.preedit.is_empty())
        {
            return false;
        }
        if !open
            && event.logical_key == Key::Named(NamedKey::Escape)
            && self.input.dismiss_combo_completion(&self.scene)
        {
            self.update_editor();
            return true;
        }
        if search
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::Home | NamedKey::End)
            )
            && (self.modifiers.shift_key()
                || self.modifiers.control_key()
                || self.modifiers.super_key()
                || self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.choice_menu(&self.scene, layout))
                    .is_none_or(|menu| menu.highlighted.is_none()))
        {
            return false;
        }
        let key = match event.logical_key {
            Key::Named(NamedKey::F4) if !event.repeat => ChoiceKey::Open,
            Key::Named(NamedKey::ArrowDown) if self.modifiers.alt_key() => {
                if event.repeat {
                    return open;
                }
                ChoiceKey::Open
            }
            Key::Named(NamedKey::ArrowUp) => ChoiceKey::Up,
            Key::Named(NamedKey::ArrowDown) => ChoiceKey::Down,
            Key::Named(NamedKey::Home) => ChoiceKey::Home,
            Key::Named(NamedKey::End) => ChoiceKey::End,
            Key::Named(NamedKey::PageUp) => ChoiceKey::PageUp,
            Key::Named(NamedKey::PageDown) => ChoiceKey::PageDown,
            Key::Named(NamedKey::Enter) => ChoiceKey::Accept,
            Key::Named(NamedKey::Space) if !editing => ChoiceKey::Accept,
            Key::Named(NamedKey::Escape) => ChoiceKey::Escape,
            _ if open
                && editing
                && (event.text.is_some()
                    || matches!(
                        event.logical_key,
                        Key::Named(
                            NamedKey::Backspace
                                | NamedKey::Delete
                                | NamedKey::ArrowLeft
                                | NamedKey::ArrowRight
                        )
                    )) =>
            {
                if !search {
                    self.input.dismiss_choice();
                    self.update_editor();
                }
                return false;
            }
            _ => return open && !search,
        };
        if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.choice_key(&self.scene, layout, key))
        {
            if !event.repeat
                || !matches!(
                    input.action,
                    ControlAction::AcceptComboText | ControlAction::SubmitSearch
                )
            {
                self.apply_control(input);
            }
            true
        } else if open || self.input.choice_open() {
            self.update_editor();
            true
        } else {
            false
        }
    }

    fn adjust_range(&mut self, input: RangeInput) {
        if self
            .component
            .adjust_range_at(&self.scene, &input.path, input.action)
        {
            self.refresh_scene();
        } else if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn apply_scroll_bar(&mut self, update: ScrollBarUpdate, refresh_focus: bool) {
        match update {
            ScrollBarUpdate::ViewportMoved => self.relayout_with_caret(false),
            ScrollBarUpdate::Range(input) => self.adjust_range(input),
            ScrollBarUpdate::Unchanged if refresh_focus => self.refresh_editor(false),
            ScrollBarUpdate::Unchanged => {}
        }
    }

    fn keyboard_input(&mut self, event: &KeyEvent, is_synthetic: bool) {
        if event.state == ElementState::Pressed {
            self.input.clear_editor_click();
        }
        let route = (!is_synthetic)
            .then(|| self.input.key_route(&self.scene, self.layout.as_deref()?))
            .flatten();
        let Some(route) = route else {
            self.default_key_input(event, is_synthetic);
            return;
        };
        let (capture, bubble) = if event.state == ElementState::Pressed {
            (KeyPhase::CapturePressed, KeyPhase::Pressed)
        } else {
            (KeyPhase::CaptureReleased, KeyPhase::Released)
        };
        let key = xrack_runtime::KeyEvent {
            key: match &event.logical_key {
                Key::Character(text) => text.to_string(),
                Key::Named(key) => format!("{key:?}"),
                Key::Dead(_) => "Dead".into(),
                Key::Unidentified(_) => "Unidentified".into(),
            },
            text: event.text.as_deref().unwrap_or_default().to_owned(),
            repeat: event.repeat,
            modifiers: self.input_modifiers(),
        };
        if let Some(result) = self.layout.as_ref().and_then(|layout| {
            route.dispatch(&mut self.component, &self.scene, layout, capture, &key)
        }) {
            if result == EventResult::Accept {
                self.input.cancel_shortcut_sequence();
                if event.state == ElementState::Released {
                    if let Some(key) = activation_key_value(&event.logical_key) {
                        self.input.key_release_target(key);
                    }
                    if let Some(key) = navigation_key_value(&event.logical_key) {
                        self.input.discard_range_key(key);
                    }
                }
                self.refresh_scene();
                return;
            }
            self.refresh_after_update(false);
        }
        if self.input.focused.as_ref() != Some(route.focused())
            || self.default_key_input(event, is_synthetic)
            || self.input.focused.as_ref() != Some(route.focused())
        {
            return;
        }
        if self
            .layout
            .as_ref()
            .and_then(|layout| {
                route.dispatch(&mut self.component, &self.scene, layout, bubble, &key)
            })
            .is_some()
        {
            self.refresh_after_update(false);
        }
    }

    fn default_key_input(&mut self, event: &KeyEvent, is_synthetic: bool) -> bool {
        #[cfg(target_os = "android")]
        if self.android_back_key(event) {
            return true;
        }
        if event.state == ElementState::Pressed
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::Tab | NamedKey::Escape)
            )
        {
            self.input.cancel_shortcut_sequence();
            self.input.reset_list_typeahead();
        }
        if event.state == ElementState::Pressed && event.logical_key == Key::Named(NamedKey::Escape)
        {
            if self.cancel_drag() {
                return true;
            }
            if self.input.button_hold_deadline().is_some() {
                self.input.cancel_press();
                self.update_editor();
                return true;
            }
            if self.input.cancel_slider_preview()
                || self.input.has_switch_preview()
                || self.input.tab_move_active()
                || self.input.cancel_table_drag_selection()
                || self.input.cancel_table_row_move()
                || self.input.cancel_delay()
            {
                self.cancel_direct_scroll();
                self.update_editor();
                return true;
            }
        }
        // Android can deliver the preceding shortcut's key-up after a new touch
        // has started. Only key presses interrupt the pointer's scroll/tap route.
        if event.state == ElementState::Pressed {
            self.cancel_direct_scroll_with_key(Some(&event.logical_key));
        }
        if event.state == ElementState::Pressed && event.logical_key != Key::Named(NamedKey::Tab) {
            let dismissed = self.dismiss_tooltip();
            if dismissed && event.logical_key == Key::Named(NamedKey::Escape) {
                return true;
            }
        }
        if event.logical_key == Key::Named(NamedKey::Tab)
            && event.state == ElementState::Pressed
            && !event.repeat
        {
            self.pending_clipboard = None;
            self.commit_spin();
            if self
                .input
                .focused
                .as_ref()
                .is_some_and(|target| self.input.table_cell_editing(&target.path))
            {
                let origin = self.input.tab_edit_origin(&self.scene);
                if self
                    .input
                    .editor
                    .as_ref()
                    .is_none_or(|editor| editor.preedit.is_empty())
                {
                    let result = self.commit_table_edit();
                    if result == TableEditCommit::Kept {
                        return true;
                    }
                    if matches!(
                        result,
                        TableEditCommit::Changed | TableEditCommit::Unchanged
                    ) && let Some(origin) = origin
                        && self.continue_table_edit(&origin, self.modifiers.shift_key())
                    {
                        return true;
                    }
                } else if origin.is_some() {
                    return true;
                } else {
                    self.input.cancel_table_edit();
                }
            }
            self.input.dismiss_choice();
            self.input.dismiss_menu();
            if self.layout.as_ref().is_some_and(|layout| {
                self.input
                    .focus_next(&self.scene, layout, self.modifiers.shift_key())
            }) {
                self.relayout();
            } else {
                self.update_editor();
            }
            return true;
        }
        if self.context_menu_key(event)
            || !is_synthetic && self.shortcut_key(event)
            || self.menu_key(event)
        {
            return true;
        }
        if !is_synthetic && self.editor_return(event) {
            return true;
        }
        if self.choice_key(event)
            || self.tab_close_key(event)
            || !is_synthetic && self.list_typeahead_key(event)
            || self.table_edit_key(event)
            || self.selection_key(event)
            || self.tree_toggle_key(event)
            || self.dialog_key(event)
            || self.spin_key(event)
            || self.text_key(event)
            || self.table_clipboard_key(event)
            || self.navigation_key(event)
        {
            return true;
        }
        let Some(key) = activation_key_value(&event.logical_key).filter(|_| !event.repeat) else {
            return false;
        };
        if !self.input.focused.as_ref().is_some_and(|target| {
            self.scene.at_path(&target.path).is_some_and(|node| {
                node.kind != "FocusScope"
                    && !self.input.is_text_editor(node, target)
                    && !spin_editable(node)
            })
        }) {
            return false;
        }
        if event.state == ElementState::Pressed {
            self.input.key_press_with_scene(&self.scene, key);
            if let Some(layout) = &self.layout {
                let now = Instant::now();
                self.input.begin_key_delay(&self.scene, layout, key, now);
                self.input
                    .begin_key_button_hold(&self.scene, layout, key, now);
            }
            self.flush_button_press_events();
            if let Some(state) = &self.state {
                state.window.request_redraw();
            }
        } else {
            let target = self.input.key_release_with_scene(&self.scene, key);
            let target = target.map(|target| {
                self.layout
                    .as_ref()
                    .and_then(|layout| {
                        self.input
                            .current_item_delegate(&self.scene, layout, &target)
                    })
                    .unwrap_or(target)
            });
            if target.is_some() {
                self.last_button_click = None;
            }
            self.activate(target);
        }
        true
    }

    fn tab_close_key(&mut self, event: &KeyEvent) -> bool {
        let modifiers = self.input_modifiers();
        if event.state != ElementState::Pressed
            || event.repeat
            || event.logical_key != Key::Named(NamedKey::Delete)
            || modifiers.shift
            || modifiers.control
            || modifiers.alt
            || modifiers.meta
        {
            return false;
        }
        let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.focused_tab_close(&self.scene, layout))
        else {
            return false;
        };
        self.apply_control(action);
        true
    }

    #[cfg(any(target_os = "android", test))]
    fn android_back_key(&mut self, event: &KeyEvent) -> bool {
        if event.logical_key != Key::Named(NamedKey::BrowserBack) {
            return false;
        }
        // One physical Back gesture dismisses at most one layer. Its key-up
        // and repeated key-down events must not reach the layer underneath.
        if event.state != ElementState::Pressed || event.repeat {
            return true;
        }
        let cancel = KeyEvent {
            logical_key: Key::Named(NamedKey::Escape),
            unmodified_key: Key::Named(NamedKey::Escape),
            text: None,
            state: ElementState::Pressed,
            repeat: false,
        };
        if self.input.menu_open() {
            self.menu_key(&cancel);
            return true;
        }
        if self.input.choice_open() {
            self.choice_key(&cancel);
            return true;
        }
        if self
            .input
            .editor
            .as_ref()
            .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return true;
        }
        if self.dismiss_tooltip() || self.dialog_key(&cancel) {
            return true;
        }
        if self.layout.as_ref().is_some_and(|layout| {
            layout
                .overlay_paths()
                .iter()
                .any(|path| self.input.allows_popup(layout, path))
        }) {
            // A nonmodal popup may explicitly prohibit Escape/Back dismissal.
            return true;
        }
        let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.stack_back_in_window(&self.scene, layout))
        else {
            return false;
        };
        self.apply_control(action);
        true
    }

    fn navigation_key(&mut self, event: &KeyEvent) -> bool {
        if event.logical_key == Key::Named(NamedKey::BrowserBack)
            || self.modifiers.alt_key() && event.logical_key == Key::Named(NamedKey::ArrowLeft)
        {
            return event.state == ElementState::Pressed && !event.repeat && self.back_in_stack();
        }
        let Some(key) = navigation_key_value(&event.logical_key) else {
            return false;
        };
        let modifiers = self.input_modifiers();
        if event.state == ElementState::Pressed
            && modifiers.primary
            && modifiers.shift
            && !modifiers.alt
            && let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.tab_move_key(&self.scene, layout, key))
        {
            if let Some(action) = action {
                self.apply_control(action);
            }
            return true;
        }
        if event.state == ElementState::Pressed
            && self.modifiers.alt_key()
            && !self.modifiers.shift_key()
            && !self.modifiers.control_key()
            && !self.modifiers.super_key()
            && let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.table_row_move_key(&self.scene, layout, key))
        {
            self.apply_control(action);
            return true;
        }
        if event.state == ElementState::Pressed
            && let Some(update) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.scroll_bar_key(&self.scene, layout, key))
        {
            self.apply_scroll_bar(update, false);
            return true;
        }
        if event.state == ElementState::Pressed
            && let Some(input) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.split_key(&self.scene, layout, key))
        {
            self.apply_split_resize(input);
            return true;
        }
        if let Some(input) = self.layout.as_ref().and_then(|layout| {
            self.input.slider_key(
                &self.scene,
                layout,
                key,
                event.state == ElementState::Pressed,
            )
        }) {
            self.adjust_range(input);
            return true;
        }
        if event.state == ElementState::Pressed
            && self
                .layout
                .as_ref()
                .is_some_and(|layout| self.input.calendar_page_key(&self.scene, layout, key))
        {
            self.relayout();
            return true;
        }
        let operation = self.row_selection(true);
        if event.state == ElementState::Pressed
            && let Some(input) = self.layout.as_ref().and_then(|layout| {
                self.input
                    .tree_key_with_selection(&self.scene, layout, key, operation)
                    .or_else(|| {
                        self.input
                            .list_key_with_selection(&self.scene, layout, key, operation)
                    })
                    .or_else(|| self.input.group_key(&self.scene, layout, key))
                    .or_else(|| self.input.page_indicator_key(&self.scene, layout, key))
                    .or_else(|| self.input.calendar_key(&self.scene, layout, key))
                    .or_else(|| self.input.tumbler_key(&self.scene, layout, key))
                    .or_else(|| self.input.swipe_delegate_key(&self.scene, layout, key))
                    .or_else(|| self.input.swipe_key(&self.scene, layout, key))
            })
        {
            self.apply_control(input);
            return true;
        }
        false
    }

    fn back_in_stack(&mut self) -> bool {
        let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.stack_back(&self.scene, layout))
        else {
            return false;
        };
        self.apply_control(action);
        true
    }

    fn tree_toggle_key(&mut self, event: &KeyEvent) -> bool {
        if event.state != ElementState::Pressed
            || event.repeat
            || !matches!(
                event.logical_key,
                Key::Named(NamedKey::Enter | NamedKey::Space)
            )
        {
            return false;
        }
        let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.tree_toggle(&self.scene, layout))
        else {
            return false;
        };
        self.apply_control(action);
        true
    }

    fn row_selection(&self, keyboard: bool) -> RowSelection {
        RowSelection::from_modifiers(self.input_modifiers(), keyboard)
    }

    fn selection_key(&mut self, event: &KeyEvent) -> bool {
        let key = match &event.logical_key {
            Key::Named(NamedKey::Space) => " ",
            Key::Named(NamedKey::Escape) => "Escape",
            Key::Character(key) => key.as_str(),
            _ => return false,
        };
        let input = self.layout.as_ref().and_then(|layout| {
            self.input
                .selection_key_action(&self.scene, layout, key, self.input_modifiers())
        });
        let Some(input) = input else {
            return false;
        };
        if event.state == ElementState::Pressed && !event.repeat {
            self.apply_control(input);
        }
        true
    }

    fn window_chrome_press(&mut self) -> bool {
        let edge = self.window_resize_cursor();
        let (Some(layout), Some(state)) = (&self.layout, &mut self.state) else {
            return false;
        };
        if state.presentation.observed().full_screen
            || state.is_native_full_screen()
            || state.presentation.is_switching_full_screen()
        {
            return false;
        }
        if edge.is_some_and(|edge| state.begin_client_resize(edge)) {
            self.input.cancel_press();
        } else if !self.input.arm_window_move(&self.scene, layout) {
            return false;
        }
        self.pending_clipboard = None;
        self.input.cancel_shortcut_sequence();
        self.dismiss_tooltip();
        true
    }

    fn left_pointer(&mut self, state: ElementState, click: Option<PointerClick>) {
        self.left_pointer_with_mode(state, click, false);
    }

    fn left_pointer_with_mode(
        &mut self,
        state: ElementState,
        click: Option<PointerClick>,
        replay: bool,
    ) {
        let previous_button = if state == ElementState::Released {
            self.last_button_click.take()
        } else {
            None
        };
        let previous_click = if state == ElementState::Released {
            self.last_pointer_click.take()
        } else {
            None
        };
        if state == ElementState::Released && self.input.table_row_move_claimed() {
            let action = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.finish_table_row_move(&self.scene, layout));
            if let Some(action) = action {
                self.apply_control(action);
            } else {
                self.input.cancel_table_row_move();
                self.update_editor();
            }
            return;
        }
        if state == ElementState::Released && self.input.table_drag_selection_claimed() {
            let action = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.finish_table_drag_selection(&self.scene, layout));
            if let Some(action) = action {
                self.apply_control(action);
            } else {
                self.input.cancel_table_drag_selection();
                self.update_editor();
            }
            return;
        }
        if state == ElementState::Released
            && let Some(point) = self.input.drag_position()
            && self.finish_drag(point)
        {
            return;
        }
        if state == ElementState::Pressed {
            self.pending_clipboard = None;
            if self.layout.as_ref().is_some_and(|layout| {
                self.input.menu_press(&self.scene, layout)
                    || self.input.choice_press(&self.scene, layout)
            }) {
                self.update_editor();
                return;
            }
            if self.input.pointer_on_stack_slide() {
                return;
            }
            if let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.popup_outside_press(&self.scene, layout))
            {
                self.apply_control(action);
            }
            if let Some(update) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.scroll_bar_press(&self.scene, layout))
            {
                self.apply_scroll_bar(update, true);
                return;
            }
            if self
                .layout
                .as_ref()
                .is_some_and(|layout| self.input.dialog_pointer_press(layout))
            {
                self.update_editor();
                return;
            }
            if self
                .layout
                .as_ref()
                .is_some_and(|layout| self.input.press_table_selection_handle(&self.scene, layout))
            {
                self.update_editor();
                return;
            }
            if self.input.focused != self.input.hovered && !self.input.table_row_resize_cursor() {
                self.commit_spin();
            }
            if let Some(layout) = &self.layout {
                if replay {
                    self.input.replay_tap_in_scene(&self.scene, layout);
                } else {
                    self.input.pointer_press_in_scene(&self.scene, layout);
                }
            } else {
                self.input.pointer_press();
            }
            if let Some(point) = self.input.pointer_position() {
                if let Some(layout) = &self.layout {
                    self.input.arm_drag(&self.scene, layout, point);
                    self.input.arm_table_drag_selection(
                        &self.scene,
                        layout,
                        self.modifiers.control_key() || self.modifiers.super_key(),
                    );
                }
                if !self.input.table_row_resize_cursor() {
                    let mouse_press = click
                        .as_ref()
                        .filter(|sample| !replay && sample.touch_device.is_none())
                        .map(|sample| sample.time);
                    self.place_text_caret(point, self.modifiers.shift_key(), mouse_press, false);
                }
            }
            if let Some(layout) = &self.layout {
                let now = Instant::now();
                self.input.begin_pointer_delay(&self.scene, layout, now);
                self.input
                    .begin_pointer_button_hold(&self.scene, layout, now);
                self.input.split_press(&self.scene, layout);
                let _step_pressed = self.input.spin_press(&self.scene, layout);
                #[cfg(target_os = "android")]
                if let Some(state) = &mut self.state {
                    // A stepper changes a number without opening an editor.
                    // A later tap on the text body restores native text input.
                    state.android_spin_step_focus = self
                        .input
                        .focused
                        .as_ref()
                        .filter(|_| _step_pressed)
                        .cloned();
                }
                #[cfg(target_os = "ios")]
                if let Some(state) = &mut self.state {
                    if !_step_pressed && self.input.hovered == self.input.focused {
                        state.ios_keyboard_dismissed_focus = None;
                    }
                    state.ios_spin_step_focus = self
                        .input
                        .focused
                        .as_ref()
                        .filter(|target| {
                            _step_pressed
                                && !state
                                    .ios_text
                                    .as_ref()
                                    .is_some_and(|input| input.editing(target))
                        })
                        .cloned();
                }
            }
            if let Some(input) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.slider_press(&self.scene, layout))
            {
                self.adjust_range(input);
            }
            self.update_editor();
        } else if self.input.scroll_bar_captured() {
            let update = if let Some(layout) = &self.layout {
                self.input
                    .release_scroll_bar(&self.scene, layout)
                    .unwrap_or(ScrollBarUpdate::Unchanged)
            } else {
                self.input.finish_scroll_bar();
                ScrollBarUpdate::Unchanged
            };
            self.apply_scroll_bar(update, true);
        } else if self.input.menu_open() {
            if let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.menu_release(&self.scene, layout))
            {
                self.apply_control(action);
            } else {
                self.update_editor();
            }
        } else if self.input.choice_open() {
            if let Some(input) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.choice_release(&self.scene, layout))
            {
                self.apply_control(input);
            } else {
                self.update_editor();
            }
        } else if let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.dialog_pointer_release(&self.scene, layout))
        {
            self.apply_control(action);
        } else if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.spin_release(&self.scene, layout))
        {
            self.apply_control(input);
        } else if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.slider_release(&self.scene, layout))
        {
            self.adjust_range(input);
        } else if self.input.finish_split_resize() || self.input.finish_table_resize() {
            self.layout_changed(true);
        } else if self.input.table_column_move_claimed() {
            let action = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.finish_table_column_move(&self.scene, layout));
            if let Some(action) = action {
                self.apply_control(action);
            } else {
                self.input.cancel_press();
                self.update_editor();
            }
        } else if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.control_part_release(&self.scene, layout))
        {
            self.apply_control(input);
        } else if let Some(input) = self.input.table_header_release(
            &self.scene,
            self.row_selection(false),
            self.modifiers.alt_key(),
        ) {
            self.apply_control(input);
        } else {
            let target = self.input.pointer_release_target();
            if let Some(target) = &target
                && let Some(inspector) = self.component.window_inspector(&self.base_scene)
            {
                inspector.record_click(&self.scene, &target.path);
            }
            let edit_target = if self.row_selection(false) == RowSelection::Replace {
                target.as_ref()
            } else {
                None
            };
            let editing = self.layout.as_ref().is_some_and(|layout| {
                self.input
                    .table_cell_click(&self.scene, layout, edit_target, Instant::now())
            });
            let double_button = click
                .as_ref()
                .zip(target.as_ref())
                .and_then(|(click, target)| {
                    let candidate = ButtonClick::new(
                        &self.scene,
                        target,
                        click.touch_device,
                        (click.point, click.time),
                    )?;
                    if previous_button.is_some_and(|previous| candidate.follows(&previous)) {
                        Some(candidate)
                    } else {
                        self.last_button_click = Some(candidate);
                        None
                    }
                });
            let touch = click
                .as_ref()
                .is_some_and(|click| click.touch_device.is_some());
            let double_click = click.zip(target.as_ref()).and_then(|(click, target)| {
                let handle = self
                    .scene
                    .interactive_at_path(&target.path)?
                    .pointer_handle
                    .clone()?;
                if previous_click.is_some_and(|(prior, row, previous)| {
                    prior == handle && row == target.row_key && click.follows(&previous)
                }) {
                    Some(handle)
                } else {
                    self.last_pointer_click = Some((handle, target.row_key, click));
                    None
                }
            });
            self.activate_with_double_click(target, double_click, touch);
            if let (Some(click), Some(layout)) = (double_button, &self.layout)
                && click.deliver(&mut self.component, &self.input, &self.scene, layout)
            {
                self.refresh_scene();
            }
            if editing {
                self.update_editor();
            }
        }
    }

    fn begin_direct_scroll(&mut self, contact: ScrollContact, point: Point, now: Instant) {
        let kind = match contact {
            ScrollContact::Touch(..) => ScrollGestureKind::Touch,
            ScrollContact::MousePan(_) => ScrollGestureKind::MousePan,
            ScrollContact::MouseControl(_) => ScrollGestureKind::MouseControl,
            ScrollContact::MouseViewport(_) => {
                unreachable!("a viewport pan starts with an ordinary press")
            }
        };
        let overscroll = self.input.overscroll_frame();
        let additive = kind == ScrollGestureKind::MouseControl
            && (self.modifiers.control_key() || self.modifiers.super_key());
        if self.layout.as_ref().is_some_and(|layout| {
            self.input.begin_scroll_gesture_with_selection(
                &self.scene,
                layout,
                point,
                now,
                kind,
                additive,
            )
        }) {
            self.scroll_contact = Some(contact);
            self.pending_clipboard = None;
            if overscroll != self.input.overscroll_frame() {
                self.relayout_with_caret(false);
            } else {
                self.layout_changed(false);
            }
        }
    }

    fn requested_drag_action(&self) -> Option<DragAction> {
        xrack_runtime::KeyModifiers {
            control: self.modifiers.control_key(),
            shift: self.modifiers.shift_key(),
            ..Default::default()
        }
        .drag_action()
    }

    fn dispatch_drag(&mut self, update: DragUpdate) -> bool {
        let mut handled = false;
        for event in update.events {
            handled |= event.apply(&mut self.component, &self.scene);
        }
        handled
    }

    fn apply_drag(&mut self, update: DragUpdate) -> bool {
        let captured = update.captured;
        if self.dispatch_drag(update) {
            self.refresh_scene();
        }
        if captured && let Some(state) = &self.state {
            self.update_pointer_cursor();
            state.window.request_redraw();
        }
        captured
    }

    fn move_drag(&mut self, point: Point, now: Instant) -> bool {
        let requested = self.requested_drag_action();
        let Some(layout) = &self.layout else {
            return false;
        };
        let update = self
            .input
            .drag_motion_at(&self.scene, layout, point, requested, now);
        self.apply_drag(update)
    }

    fn finish_drag(&mut self, point: Point) -> bool {
        let requested = self.requested_drag_action();
        let update = if let Some(layout) = &self.layout {
            self.input
                .finish_drag(&self.scene, layout, point, requested)
        } else {
            self.input.cancel_drag()
        };
        self.apply_drag(update)
    }

    fn cancel_drag(&mut self) -> bool {
        let update = self.input.cancel_drag();
        self.apply_drag(update)
    }

    fn flush_swipe_events(&mut self, previous: Option<&Node>) -> bool {
        let mut delivered = false;
        loop {
            if let Some(layout) = &self.layout {
                self.input.reconcile_swipe(&self.scene, layout);
            }
            let events = self.input.take_swipe_events();
            if events.is_empty() {
                return delivered;
            }
            delivered = true;
            for event in events {
                let scene = if event.matches(&self.scene) {
                    &*self.scene
                } else {
                    previous.unwrap_or(&self.scene)
                };
                event.apply(&mut self.component, scene);
            }
        }
    }

    fn flush_button_press_events(&mut self) -> bool {
        self.layout.as_ref().is_some_and(|layout| {
            self.input
                .dispatch_button_press_events(&mut self.component, &self.scene, layout)
        })
    }

    fn move_direct_scroll(&mut self, point: Point, now: Instant) -> bool {
        let was_claimed = self.input.mouse_viewport_pan_claimed();
        let was_dragging_table = self.input.scroll_table_drag_claimed();
        let moved = self.layout.as_ref().is_some_and(|layout| {
            self.input
                .move_scroll_gesture(&self.scene, layout, point, now)
        });
        #[cfg(target_os = "ios")]
        let dismissed_keyboard = self.dismiss_ios_keyboard_for_scroll();
        #[cfg(not(target_os = "ios"))]
        let dismissed_keyboard = false;
        let claimed = self.input.mouse_viewport_pan_claimed();
        if self.input.has_pending_swipe_events() {
            self.refresh_after_update(false);
        } else if moved && self.input.scroll_table_drag_claimed() {
            if !was_dragging_table {
                self.update_editor();
            } else if let Some(state) = &self.state {
                state.window.request_redraw();
            }
        } else if moved {
            self.relayout_after_scroll();
        } else if dismissed_keyboard {
            self.layout_changed(false);
        } else if claimed && !was_claimed {
            self.update_editor();
        }
        if self.flush_button_press_events() {
            self.refresh_scene();
        }
        claimed
    }

    #[cfg(target_os = "ios")]
    fn dismiss_ios_keyboard_for_scroll(&mut self) -> bool {
        if !self.input.take_scroll_keyboard_dismissal()
            || self
                .input
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return false;
        }
        let (Some(state), Some(target)) = (&mut self.state, &self.input.focused) else {
            return false;
        };
        if state.ios_keyboard_dismissed_focus.is_some()
            || !state
                .ios_text
                .as_ref()
                .is_some_and(|input| input.can_dismiss_for_scroll(target))
            || !ios::software_keyboard_visible(&state.window)
        {
            return false;
        }
        state.ios_keyboard_dismissed_focus = Some(target.clone());
        true
    }

    fn relayout_after_scroll(&mut self) {
        if self.component.scene_dirty() {
            self.sync_base_scene();
        }
        self.relayout_with_caret(false);
    }

    fn end_direct_scroll(&mut self, point: Point, now: Instant) {
        let contact = self.scroll_contact.take();
        let Some(layout) = &self.layout else {
            self.cancel_direct_scroll();
            return;
        };
        let outcome = self
            .input
            .end_scroll_gesture(&self.scene, layout, point, now);
        #[cfg(target_os = "ios")]
        self.dismiss_ios_keyboard_for_scroll();
        if self.input.has_pending_swipe_events() {
            self.refresh_after_update(false);
        }
        if outcome.scrolled {
            self.relayout_after_scroll();
        }
        if let Some(control) = outcome.control {
            self.apply_control(control);
        }
        if (outcome.tap
            || matches!(
                contact,
                Some(
                    ScrollContact::MousePan(_)
                        | ScrollContact::MouseControl(_)
                        | ScrollContact::MouseViewport(_)
                )
            ))
            && let Some(layout) = &self.layout
        {
            self.input.pointer_move(&self.scene, layout, point);
        }
        let previous_target = self.input.hovered.clone();
        let handled = self.flush_button_press_events();
        if handled {
            self.refresh_scene();
        }
        if outcome.tap && (!handled || previous_target == self.input.hovered) {
            let touch_device = match contact {
                Some(ScrollContact::Touch(device, _, _)) => Some(device),
                _ => None,
            };
            self.left_pointer_with_mode(ElementState::Pressed, None, true);
            self.left_pointer(
                ElementState::Released,
                Some(PointerClick {
                    touch_device,
                    point,
                    time: now,
                }),
            );
            self.input.pointer_leave();
        } else if !matches!(contact, Some(ScrollContact::MousePan(_))) {
            self.last_pointer_click = None;
            self.last_button_click = None;
        }
        self.layout_changed(false);
    }

    fn cancel_direct_scroll(&mut self) {
        self.cancel_direct_scroll_with_key(None);
    }

    fn cancel_direct_scroll_with_key(&mut self, key: Option<&Key>) {
        self.touch_hold = None;
        if self.pointer_capture.as_ref().is_some_and(|capture| {
            self.scroll_contact.map(PointerContact::for_scroll) == Some(capture.contact)
        }) {
            self.cancel_pointer();
        }
        let range_capture = self.input.touch_range_captured();
        let split_capture = match self.scroll_contact {
            Some(ScrollContact::Touch(_, _, TouchRoute::Split)) => {
                self.input.finish_split_resize();
                self.input.pointer_leave();
                true
            }
            Some(ScrollContact::Touch(_, _, TouchRoute::Direct)) => {
                self.input.finish_touch_scroll_bar();
                self.input.finish_touch_range();
                false
            }
            _ => false,
        };
        self.scroll_contact = None;
        let preview = self.input.tab_move_active()
            || self.input.tumbler_preview().is_some()
            || self.input.has_switch_preview()
            || self.input.has_swipe_preview()
            || self.input.drawer_preview().is_some()
            || self.input.has_overscroll();
        let table_drag = self.input.cancel_scroll_table_drag();
        match (self.layout.as_ref(), key) {
            (Some(layout), Some(Key::Named(NamedKey::Tab))) => self
                .input
                .cancel_scroll_gesture_for_picker_key(&self.scene, layout, false),
            (Some(layout), Some(key)) if activation_key_value(key).is_some() => self
                .input
                .cancel_scroll_gesture_for_picker_key(&self.scene, layout, true),
            _ => self.input.cancel_scroll_gesture(),
        }
        if self.input.has_pending_swipe_events() {
            self.refresh_after_update(false);
        } else if preview || self.component.scene_dirty() {
            self.relayout_after_scroll();
        } else if (table_drag || range_capture || split_capture)
            && let Some(state) = &self.state
        {
            state.window.request_redraw();
        }
    }

    fn touch_event(&mut self, touch: Touch) {
        let Some(state) = &self.state else {
            return;
        };
        let point = touch
            .location
            .to_logical::<f32>(state.window.scale_factor());
        let point = Point {
            x: point.x,
            y: point.y,
        };
        #[cfg(target_os = "android")]
        if touch.phase == TouchPhase::Started
            && self
                .input
                .focused
                .as_ref()
                .and_then(|target| self.layout.as_ref()?.at_path(&target.path))
                .is_some_and(|item| item.visible_window_bounds(item.bounds).contains(point))
        {
            self.finish_committed_suggestion();
        }
        #[cfg(target_os = "android")]
        if touch.phase == TouchPhase::Started
            && self.layout.as_ref().is_some_and(|layout| {
                self.input
                    .touch_text_target(&self.scene, layout, point)
                    .is_some()
            })
            && let Some(native) = self
                .state
                .as_mut()
                .and_then(|state| state.android_text.as_mut())
        {
            native.request_keyboard();
        }
        self.update_touch(
            touch.device_id,
            touch.id,
            touch.phase,
            point,
            Instant::now(),
        );
    }

    fn update_touch(
        &mut self,
        device: DeviceId,
        id: u64,
        phase: TouchPhase,
        point: Point,
        now: Instant,
    ) {
        let contact = PointerContact::Touch(device, id);
        if phase == TouchPhase::Started {
            self.dismiss_tooltip();
        }
        if self.update_selection_handle_drag(contact, phase, point) {
            return;
        }
        if phase == TouchPhase::Started && self.input.touch_selection_target().is_some() {
            self.input.set_touch_selection_handles(false);
            if let Some(state) = &self.state {
                state.window.request_redraw();
            }
        }
        if let Some(layout) = &self.layout {
            let (consumed, events) = self.scale_rotate.touch(
                &scale_rotate::Context {
                    scene: &self.scene,
                    layout,
                    input: &self.input,
                },
                scale_rotate::ContactId(device, id),
                scale_rotate::phase(phase),
                point,
            );
            self.apply_scale_rotate_batch(events);
            if consumed {
                self.touch_hold = None;
                return;
            }
        }
        if self.update_touch_hold(contact, phase, point) {
            return;
        }
        let route = match self.scroll_contact {
            Some(ScrollContact::Touch(current_device, current_id, route))
                if current_device == device && current_id == id =>
            {
                Some(route)
            }
            _ => None,
        };
        if matches!(route, Some(TouchRoute::Split)) {
            if matches!(phase, TouchPhase::Moved | TouchPhase::Ended)
                && self.input.split_resize_active()
                && self.input.pointer_position() != Some(point)
                && let Some(layout) = &self.layout
            {
                self.input.pointer_move(&self.scene, layout, point);
                if let Some(resize) = self.input.split_drag(&self.scene, layout) {
                    self.apply_split_resize(resize);
                }
            }
            if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.cancel_direct_scroll();
                self.update_editor();
            }
            return;
        }
        let owned = route.is_some();
        match phase {
            TouchPhase::Started
                if self.scroll_contact.is_none()
                    && self.input.drag_position().is_none()
                    && !self.input.split_resize_active()
                    && !self.input.scroll_bar_captured() =>
            {
                if self.input.menu_open() {
                    self.begin_direct_scroll(
                        ScrollContact::Touch(device, id, TouchRoute::Direct),
                        point,
                        now,
                    );
                    return;
                }
                if let Some(update) = self.layout.as_ref().and_then(|layout| {
                    self.input
                        .touch_scroll_bar_press(&self.scene, layout, point)
                }) {
                    self.scroll_contact =
                        Some(ScrollContact::Touch(device, id, TouchRoute::Direct));
                    self.apply_scroll_bar(update, true);
                    return;
                }
                if let Some(layout) = &self.layout
                    && self.input.touch_range_press(&self.scene, layout, point)
                {
                    self.scroll_contact =
                        Some(ScrollContact::Touch(device, id, TouchRoute::Direct));
                    if let Some(action) = self.input.slider_press(&self.scene, layout) {
                        self.adjust_range(action);
                    } else {
                        self.update_editor();
                    }
                    return;
                }
                if let Some(layout) = &self.layout
                    && self.input.touch_split_press(&self.scene, layout, point)
                {
                    self.scroll_contact = Some(ScrollContact::Touch(device, id, TouchRoute::Split));
                    self.pending_clipboard = None;
                    self.update_editor();
                    return;
                }
                self.begin_direct_scroll(
                    ScrollContact::Touch(device, id, TouchRoute::Direct),
                    point,
                    now,
                );
                if self.scroll_contact.is_some()
                    && let Some(layout) = &self.layout
                {
                    self.input.arm_touch_drag(&self.scene, layout, point, now);
                }
                if matches!(self.scroll_contact, Some(ScrollContact::Touch(current_device, current_id, _))
                    if current_device == device && current_id == id)
                {
                    self.arm_touch_hold(contact, point, now);
                    self.start_pointer(PointerContact::Touch(device, id), point, now);
                }
            }
            TouchPhase::Moved if owned && self.input.touch_range_captured() => {
                self.move_touch_range(point);
            }
            TouchPhase::Ended if owned && self.input.touch_range_captured() => {
                self.move_touch_range(point);
                let released = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.slider_release(&self.scene, layout));
                self.input.finish_touch_range();
                self.scroll_contact = None;
                if let Some(action) = released {
                    self.adjust_range(action);
                } else {
                    self.update_editor();
                }
            }
            TouchPhase::Moved if owned && self.input.scroll_bar_captured() => {
                if let Some(update) = self
                    .layout
                    .as_ref()
                    .map(|layout| self.input.touch_scroll_bar_drag(&self.scene, layout, point))
                {
                    self.apply_scroll_bar(update, false);
                }
            }
            TouchPhase::Moved if owned => {
                let dragging = self.move_drag(point, now);
                if !dragging {
                    self.move_direct_scroll(point, now);
                }
                let kind = if dragging || self.input.direct_pointer_claimed() {
                    PointerEventKind::Cancel
                } else {
                    PointerEventKind::Move
                };
                self.advance_pointer(PointerContact::Touch(device, id), point, kind);
            }
            TouchPhase::Ended if owned && self.input.scroll_bar_captured() => {
                self.scroll_contact = None;
                let update = if let Some(layout) = &self.layout {
                    self.input
                        .release_touch_scroll_bar(&self.scene, layout, point)
                        .unwrap_or(ScrollBarUpdate::Unchanged)
                } else {
                    self.input.finish_touch_scroll_bar();
                    ScrollBarUpdate::Unchanged
                };
                self.apply_scroll_bar(update, true);
            }
            TouchPhase::Cancelled if owned && self.input.finish_touch_scroll_bar() => {
                self.scroll_contact = None;
                if let Some(state) = &self.state {
                    state.window.request_redraw();
                }
            }
            TouchPhase::Ended if owned => {
                let contact = PointerContact::Touch(device, id);
                if self.finish_drag(point) {
                    self.advance_pointer(contact, point, PointerEventKind::Cancel);
                    self.scroll_contact = None;
                } else {
                    self.release_pointer(contact, point, now);
                    self.end_direct_scroll(point, now);
                }
            }
            TouchPhase::Cancelled if owned => {
                self.cancel_drag();
                self.cancel_direct_scroll();
            }
            TouchPhase::Started => {
                self.start_additional_touch(PointerContact::Touch(device, id), point, now)
            }
            phase => self.advance_pointer(
                PointerContact::Touch(device, id),
                point,
                match phase {
                    TouchPhase::Moved => PointerEventKind::Move,
                    TouchPhase::Ended => PointerEventKind::Up,
                    _ => PointerEventKind::Cancel,
                },
            ),
        }
    }

    fn arm_touch_hold(&mut self, contact: PointerContact, point: Point, now: Instant) {
        #[cfg(target_os = "ios")]
        if self
            .state
            .as_ref()
            .and_then(|state| state.file_drop.as_ref())
            .is_some_and(|bridge| bridge.can_drag_text_at(point))
        {
            // UIKit can cancel winit's touch when lifting selected text. The
            // native recognizer follows the hold through the finger's release.
            self.touch_hold = None;
            return;
        }
        self.touch_hold = self.layout.as_ref().and_then(|layout| {
            let hold_deadline = now + Duration::from_millis(500);
            let (kind, deadline) = if let Some(target) =
                self.input
                    .touch_context_menu_target(&self.scene, layout, point)
            {
                (TouchHoldKind::ContextMenu(target), hold_deadline)
            } else if let Some(target) = self.input.touch_text_target(&self.scene, layout, point) {
                PointerCaret::new(
                    &self.input,
                    &self.scene,
                    layout,
                    &target,
                    point,
                    point,
                    false,
                )?;
                (TouchHoldKind::Editor(target), hold_deadline)
            } else {
                let (tooltip, deadline) =
                    self.input
                        .touch_tooltip_at(&self.scene, layout, point, now)?;
                (TouchHoldKind::Tooltip(tooltip), deadline)
            };
            Some(TouchHold {
                contact,
                origin: point,
                deadline: Some(deadline),
                kind,
            })
        });
    }

    fn tick_touch_hold(&mut self, now: Instant) {
        // Android drains input before drawing. Touch events that arrive during
        // a stalled frame must reach us before they can be outrun by this timer.
        #[cfg(target_os = "android")]
        let now = self.input_batch_started_at.unwrap_or(now);
        let Some(selection) = self
            .touch_hold
            .take_if(|selection| selection.deadline.is_some_and(|deadline| deadline <= now))
        else {
            return;
        };
        if self.input.direct_pointer_claimed()
            || self.input.drag_position().is_some()
            || self.scroll_contact.map(PointerContact::for_scroll) != Some(selection.contact)
            || self.pointer_capture.as_ref().is_some_and(|capture| {
                capture.contact == selection.contact
                    && (capture.sequence.press_and_hold_deadline().is_some()
                        || capture.sequence.press_and_hold_triggered())
            })
        {
            return;
        }
        let target = match selection.kind {
            TouchHoldKind::ContextMenu(target) => {
                if self.input.delay_deadline().is_none()
                    && self.input.button_hold_deadline().is_none()
                    && self.layout.as_ref().is_some_and(|layout| {
                        self.input.open_touch_context_menu(
                            &self.scene,
                            layout,
                            target,
                            selection.origin,
                        )
                    })
                {
                    self.cancel_drag();
                    self.cancel_direct_scroll();
                    self.update_editor();
                }
                return;
            }
            TouchHoldKind::Tooltip(candidate) => {
                if self.input.delay_deadline().is_none()
                    && self.input.button_hold_deadline().is_none()
                    && self.input.table_touch_hold_deadline().is_none()
                    && let Some(layout) = &self.layout
                    && self.input.show_touch_tooltip(
                        &self.scene,
                        layout,
                        candidate,
                        selection.origin,
                        now,
                    )
                {
                    self.cancel_direct_scroll();
                    if let Some(state) = &self.state {
                        state.window.request_redraw();
                    }
                }
                return;
            }
            TouchHoldKind::Editor(target) => target,
        };
        self.cancel_drag();
        self.cancel_direct_scroll();
        if self.input.focused.as_ref() != Some(&target) {
            self.commit_spin();
        }
        #[cfg(target_os = "android")]
        if self.input.focused.as_ref() == Some(&target) {
            self.finish_committed_suggestion();
        }
        let (Some(state), Some(layout)) = (&self.state, &self.layout) else {
            return;
        };
        if self
            .input
            .touch_text_target(&self.scene, layout, selection.origin)
            .as_ref()
            != Some(&target)
        {
            return;
        }
        let Some(caret) = PointerCaret::new(
            &self.input,
            &self.scene,
            layout,
            &target,
            selection.origin,
            selection.origin,
            false,
        ) else {
            return;
        };
        if self
            .input
            .focus_text_from_touch(&self.scene, layout, target.clone())
            && caret.place(&state.renderer.text, &mut self.input, &self.scene, layout)
        {
            self.pending_clipboard = None;
            self.input
                .apply_text_selection(&self.scene, TextCommand::SelectWord);
            self.input.set_touch_selection_handles(true);
            self.touch_hold = Some(TouchHold {
                deadline: None,
                kind: TouchHoldKind::Editor(target),
                ..selection
            });
            self.update_editor();
        }
    }

    fn update_touch_hold(
        &mut self,
        contact: PointerContact,
        phase: TouchPhase,
        point: Point,
    ) -> bool {
        let Some(selection) = self.touch_hold.as_ref() else {
            return false;
        };
        if selection.contact != contact {
            if selection.deadline.is_none() {
                return true;
            }
            if phase == TouchPhase::Started {
                self.touch_hold = None;
            }
            return false;
        }
        let distance = (point.x - selection.origin.x).hypot(point.y - selection.origin.y);
        if selection.deadline.is_some() {
            // A queued release or swipe can arrive after the clock deadline.
            // Only the event-loop timer can activate a hold while contact continues.
            if distance > TOUCH_HOLD_SLOP
                || matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled)
            {
                self.touch_hold = None;
            }
            return false;
        }
        let Some(selection) = self.touch_hold.take() else {
            return false;
        };
        let TouchHoldKind::Editor(target) = &selection.kind else {
            return false;
        };
        if let (Some(state), Some(layout)) = (&self.state, &self.layout)
            && self.input.focused.as_ref() == Some(target)
        {
            if matches!(phase, TouchPhase::Moved | TouchPhase::Ended)
                && distance > TOUCH_HOLD_SLOP
                && let Some(caret) = PointerCaret::new(
                    &self.input,
                    &self.scene,
                    layout,
                    target,
                    selection.origin,
                    point,
                    true,
                )
            {
                caret.place(&state.renderer.text, &mut self.input, &self.scene, layout);
            }
        }
        if phase == TouchPhase::Ended && self.input.focused.as_ref() == Some(target) {
            self.open_touch_selection_menu(target.clone(), point);
        }
        if phase == TouchPhase::Moved {
            self.touch_hold = Some(selection);
        }
        self.update_editor();
        true
    }

    fn update_selection_handle_drag(
        &mut self,
        contact: PointerContact,
        phase: TouchPhase,
        point: Point,
    ) -> bool {
        if let Some(drag) = self.selection_handle_drag.take() {
            if drag.contact != contact {
                self.selection_handle_drag = Some(drag);
                return true;
            }
            if matches!(phase, TouchPhase::Moved | TouchPhase::Ended)
                && self.input.focused.as_ref() == Some(&drag.target)
                && let Some(node) = drag.target.node_in(&self.scene)
                && self.input.editor_value(node) == drag.value
                && let (Some(state), Some(layout)) = (&self.state, &self.layout)
            {
                let caret = Point {
                    x: point.x - drag.finger_offset.x,
                    y: point.y - drag.finger_offset.y,
                };
                if let Some(hit) = PointerCaret::new(
                    &self.input,
                    &self.scene,
                    layout,
                    &drag.target,
                    drag.origin,
                    caret,
                    true,
                ) {
                    hit.place(&state.renderer.text, &mut self.input, &self.scene, layout);
                }
                if phase == TouchPhase::Ended {
                    self.open_touch_selection_menu(drag.target.clone(), point);
                } else {
                    self.selection_handle_drag = Some(drag);
                }
            }
            self.update_editor();
            return true;
        }
        if phase != TouchPhase::Started || self.scroll_contact.is_some() {
            return false;
        }
        let (Some(state), Some(layout), Some(target)) = (
            &self.state,
            &self.layout,
            self.input.touch_selection_target().cloned(),
        ) else {
            return false;
        };
        let Some(handle) = state
            .renderer
            .text
            .touch_selection_handles(&self.scene, layout, &self.input)
            .into_iter()
            .filter(|handle| handle.distance(point) <= 17.0)
            .min_by(|a, b| a.distance(point).total_cmp(&b.distance(point)))
        else {
            return false;
        };
        let Some(node) = target.node_in(&self.scene) else {
            return false;
        };
        let value = self.input.editor_value(node).into_owned();
        let Some(editor) = self.input.editor.as_mut() else {
            return false;
        };
        let selection = editor.selection(&value);
        let anchor = if handle.position.trailing {
            selection.start
        } else {
            selection.end
        };
        editor.set_selection(&value, anchor, handle.position);
        self.pending_clipboard = None;
        self.input.dismiss_menu();
        self.selection_handle_drag = Some(SelectionHandleDrag {
            contact,
            target,
            origin: handle.caret,
            finger_offset: Point {
                x: point.x - handle.caret.x,
                y: point.y - handle.caret.y,
            },
            value,
        });
        self.update_editor();
        true
    }

    fn open_touch_selection_menu(&mut self, target: Target, mut point: Point) {
        let (Some(state), Some(layout)) = (&mut self.state, &self.layout) else {
            return;
        };
        let handles = state
            .renderer
            .text
            .touch_selection_handles(&self.scene, layout, &self.input);
        for handle in handles {
            point.y = point.y.max(handle.center.y + 14.0);
        }
        self.input
            .open_touch_text_menu(&self.scene, layout, target, point);
        #[cfg(target_os = "ios")]
        if self.input.menu_open() {
            if state.ios_edit_menu.is_none() {
                state.ios_edit_menu = self
                    .events
                    .as_ref()
                    .and_then(|events| ios_edit_menu::EditMenu::new(&state.window, events.clone()));
            }
            if let Some(menu) = &state.ios_edit_menu
                && let Some(snapshot) = self.input.take_native_text_menu()
            {
                menu.present(snapshot, point);
            }
        }
    }

    fn move_touch_range(&mut self, point: Point) {
        if self.input.pointer_position() == Some(point) {
            return;
        }
        if let Some(layout) = &self.layout {
            self.input.pointer_move(&self.scene, layout, point);
            if let Some(action) = self.input.slider_drag(&self.scene, layout) {
                self.adjust_range(action);
            } else if self.input.has_slider_preview()
                && let Some(state) = &self.state
            {
                state.window.request_redraw();
            }
        }
    }

    fn dispatch_scale_rotate(&mut self, scene: &Node, event: scale_rotate::Delivery) {
        event.apply(&mut self.component, scene);
    }

    fn apply_scale_rotate(&mut self, event: scale_rotate::Delivery) {
        let scene = self.scene.clone();
        self.apply_scale_rotate_in(&scene, event);
    }

    fn apply_scale_rotate_in(&mut self, scene: &Node, event: scale_rotate::Delivery) {
        if event.event == "started" {
            let mut cancels_scroll = true;
            if let Some(pair) = self.scale_rotate.captured_touch_pair() {
                let contacts = pair
                    .map(|scale_rotate::ContactId(device, id)| PointerContact::Touch(device, id));
                for contact in contacts {
                    self.cancel_pointer_contact(contact);
                }
                cancels_scroll = self
                    .scroll_contact
                    .is_some_and(|scroll| contacts.contains(&PointerContact::for_scroll(scroll)));
            } else {
                self.cancel_all_pointers();
            }
            if cancels_scroll {
                self.input.finish_touch_range();
                if matches!(
                    self.scroll_contact,
                    Some(ScrollContact::Touch(_, _, TouchRoute::Split))
                ) {
                    self.input.finish_split_resize();
                    self.input.pointer_leave();
                }
                self.scroll_contact = None;
                self.pending_clipboard = None;
                self.input.cancel_press();
            }
        }
        self.dispatch_scale_rotate(scene, event);
        self.refresh_after_update(false);
    }

    fn apply_scale_rotate_batch(&mut self, events: [Option<scale_rotate::Delivery>; 2]) {
        if events.iter().all(Option::is_none) {
            return;
        }
        let starting = events[0]
            .as_ref()
            .is_some_and(|event| event.event == "started");
        let scene = self.scene.clone();
        for event in events.into_iter().flatten() {
            if !starting || event.event != "updated" || self.scale_rotate.is_active() {
                self.apply_scale_rotate_in(&scene, event);
            }
        }
    }

    fn cancel_scale_rotate(&mut self) {
        if let Some(event) = self.scale_rotate.reset() {
            self.apply_scale_rotate(event);
        }
    }

    #[cfg(any(not(target_os = "android"), test))]
    fn update_trackpad_gesture(
        &mut self,
        device: DeviceId,
        phase: TouchPhase,
        delta: scale_rotate::TrackpadDelta,
    ) {
        let Some(layout) = &self.layout else {
            return;
        };
        if self.scroll_contact.is_some() && !self.scale_rotate.is_active() {
            return;
        }
        let deliveries = self.scale_rotate.trackpad(
            &scale_rotate::Context {
                scene: &self.scene,
                layout,
                input: &self.input,
            },
            device,
            scale_rotate::phase(phase),
            delta,
        );
        self.apply_scale_rotate_batch(deliveries);
    }

    fn tick_button_holds(&mut self, now: Instant) {
        let pending = self.input.delay_deadline().is_some();
        if let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.advance_delay(&self.scene, layout, now))
        {
            self.apply_control(action);
        } else if pending
            && self.input.delay_deadline().is_none()
            && let Some(state) = &self.state
        {
            state.window.request_redraw();
        }
        if let Some((target, event)) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.advance_button_hold(&self.scene, layout, now))
        {
            self.last_button_click = None;
            match event {
                xrack_layout::ButtonHoldEvent::Repeat => self.activate(Some(target)),
                xrack_layout::ButtonHoldEvent::PressAndHold => {
                    if self
                        .component
                        .dispatch_at(&self.scene, &target.path, event.handler())
                    {
                        self.refresh_scene();
                    }
                }
            }
        }
    }

    fn sync_window_effects(&mut self, now: Instant) {
        let effects = self
            .layout
            .as_ref()
            .map_or_else(WindowEffects::default, |layout| {
                WindowEffects::collect(&self.scene, layout)
            });
        self.timers.sync(now, effects.timers);
        self.button_animations.sync(effects.button_animations);
        self.native_dialogs.sync(&effects.dialogs);
        self.image_playback.sync(now, effects.playback);
        self.media.sync(effects.media);
    }

    fn tick_image_playback(&mut self, now: Instant) {
        if self.image_playback.advance(now) {
            self.refresh_after_update(false);
        }
        for _ in 0..64 {
            let Some(delivery) = self.image_playback.next_finished() else {
                break;
            };
            if delivery.finish() {
                self.refresh_after_update(false);
            }
        }
    }

    fn present_native_dialog(&mut self) {
        let (Some(state), Some(events)) = (&self.state, &self.events) else {
            return;
        };
        let Some(request) = self.native_dialogs.begin_next() else {
            return;
        };
        #[cfg(not(target_os = "android"))]
        let parent = state.window.clone();
        #[cfg(target_os = "android")]
        let parent = state.android_app.clone();
        #[cfg(target_os = "ios")]
        let queue = state.ios_modal_queue.clone();
        let events = events.clone();
        let id = request.id;
        if self
            .tasks
            .spawn(async move {
                if request.is_abandoned() {
                    events.post(UserEvent::NativeDialogFinished {
                        id,
                        result: NativeDialogResult::Abandoned,
                    });
                    return;
                }
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                let result = native_dialog::present(&parent, &request.options).await;
                #[cfg(target_os = "ios")]
                let result = match native_dialog::present(&parent, &request, queue).await {
                    native_dialog::IosDialogResult::Ready(result) => result,
                    native_dialog::IosDialogResult::ImportedFolders(folders) => {
                        events.post(UserEvent::IosFoldersImported { id, folders });
                        return;
                    }
                };
                #[cfg(target_os = "android")]
                let result = match native_dialog::present(&parent, &request.options).await {
                    native_dialog::AndroidDialogResult::Ready(result) => result,
                    native_dialog::AndroidDialogResult::Imported(files) => {
                        events.post(UserEvent::AndroidDocumentsImported { id, files });
                        return;
                    }
                };
                events.post(UserEvent::NativeDialogFinished { id, result });
            })
            .is_err()
        {
            self.process_user_event(UserEvent::NativeDialogFinished {
                id,
                result: NativeDialogResult::Failed("The UI executor is closed.".into()),
            });
        }
    }

    fn tick_timers(&mut self, now: Instant) {
        for _ in 0..64 {
            let Some(delivery) = self.timers.next_due(now) else {
                break;
            };
            if delivery.fire() {
                // A callback may stop another due timer or remove its owning subtree.
                self.refresh_after_update(false);
            }
        }
    }

    fn tick_table_hold(&mut self, now: Instant) {
        if self.layout.as_ref().is_some_and(|layout| {
            self.input
                .advance_table_touch_hold(&self.scene, layout, now)
        }) {
            self.update_editor();
        }
    }

    fn apply_inspection_input(
        &mut self,
        inspector: &xrack_runtime::WindowInspector,
        action: xrack_runtime::RecordedAction,
    ) -> Result<(), String> {
        use xrack_runtime::RecordedAction;
        if self.pointer_capture.is_some() || self.scroll_contact.is_some() {
            return Err("The window is already handling a pointer gesture".into());
        }
        let (id, allowed) = match &action {
            RecordedAction::Click { id } => (id, &["Button"][..]),
            RecordedAction::Fill { id, .. } => (id, &["SearchField", "LineEdit", "TextEdit"][..]),
            RecordedAction::Reload { .. } => return Err("Expected a pointer or text action".into()),
        };
        let path = inspector
            .input_path(&self.scene, id)
            .ok_or_else(|| format!("Target {id} is absent, repeated, or ambiguous"))?;
        let node = self.scene.at_path(&path).ok_or("Target disappeared")?;
        if !allowed.contains(&node.kind)
            || node.is_password()
            || matches!(action, RecordedAction::Fill { .. }) && node.text_read_only()
        {
            return Err(format!(
                "Target {id} does not support that inspection action"
            ));
        }
        let item = self
            .layout
            .as_ref()
            .and_then(|layout| layout.at_path(&path))
            .filter(|item| item.visible && item.enabled)
            .ok_or("Target is hidden or disabled")?;
        let bounds = item.visible_window_bounds(item.bounds);
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Err("Target is outside the visible window".into());
        }
        let point = Point {
            x: bounds.x + bounds.width * 0.5,
            y: bounds.y + bounds.height * 0.5,
        };
        let hit = self.layout.as_ref().and_then(|layout| {
            self.input.pointer_move(&self.scene, layout, point);
            self.input.pointer_hover_target(&self.scene, layout)
        });
        if !hit.is_some_and(|target| target.path == path) {
            return Err("Target center is covered by another control".into());
        }
        let contact = PointerContact::Mouse(PointerButton::Primary);
        self.left_pointer(ElementState::Pressed, None);
        self.start_pointer(contact, point, Instant::now());
        let now = Instant::now();
        self.release_pointer(contact, point, now);
        self.left_pointer(
            ElementState::Released,
            Some(PointerClick {
                touch_device: None,
                point,
                time: now,
            }),
        );
        self.publish_hover();
        if let RecordedAction::Fill { text, .. } = action {
            if !self
                .input
                .focused
                .as_ref()
                .is_some_and(|target| target.path == path)
            {
                return Err("The text target did not receive keyboard focus".into());
            }
            self.edit_text_from(EditAction::ReplaceAll(&text), EditSource::Atomic);
        }
        Ok(())
    }

    fn redraw(&mut self) -> Result<(), String> {
        let loader_redraw = std::mem::take(&mut self.loader_redraw);
        if loader_redraw
            || self.component.scene_dirty()
            || self
                .state
                .as_ref()
                .is_some_and(|state| state.frame.slider_preview_changed(&self.input))
        {
            self.refresh_dependencies();
        }
        // Mounting can invalidate bindings that read Loader.status in the same scene.
        if loader_redraw && self.component.scene_dirty() {
            self.refresh_dependencies();
        }
        let now = Instant::now();
        self.tick_button_holds(now);
        self.tick_table_hold(now);
        self.tick_pointer_hold(now);
        self.tick_touch_hold(now);
        let animating = self.transition.running(Duration::ZERO);
        let elapsed = self.transition_epoch.elapsed();
        let tumbler = self
            .layout
            .as_ref()
            .map_or_else(Default::default, |layout| {
                self.input.advance_tumbler_motion(&self.scene, layout, now)
            });
        if let Some(control) = tumbler.control {
            self.apply_control(control);
        }
        let scrolled = self.layout.as_ref().is_some_and(|layout| {
            self.input.advance_scroll_motion(&self.scene, layout, now)
                || self.input.advance_drag_scroll(&self.scene, layout, now)
                || self
                    .input
                    .advance_table_column_move(&self.scene, layout, now)
                || self.input.advance_table_row_move(&self.scene, layout, now)
                || self
                    .input
                    .advance_table_drag_selection(&self.scene, layout, now)
        });
        if scrolled || tumbler.scrolled {
            self.relayout_after_scroll();
        } else if animating
            || self.layout.is_none()
            || self
                .state
                .as_ref()
                .is_some_and(|state| state.frame.slider_preview_changed(&self.input))
        {
            self.prepare_frame(elapsed);
        }
        let Some(state) = &mut self.state else {
            return Ok(());
        };
        let size = drawable_size(&state.window);
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let (frame, suboptimal) = match state.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Timeout => {
                state.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Outdated => {
                state.resize()?;
                self.relayout();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                state.surface = state
                    .renderer
                    .instance
                    .create_surface(state.window.clone())
                    .map_err(|e| e.to_string())?;
                state.resize()?;
                self.relayout();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("GPU surface validation failed".into());
            }
        };
        self.stack_transitions.advance(Instant::now());
        let layout = self
            .layout
            .as_ref()
            .ok_or("layout was not prepared for the window")?;
        let pointer_changed = self.input.sync_stack_transitions(&self.stack_transitions);
        if pointer_changed {
            self.input.reconcile(&self.scene, layout);
        }
        let outline = self
            .component
            .window_inspector(&self.base_scene)
            .and_then(|inspector| inspector.selected_path())
            .and_then(|path| layout.at_path(&path))
            .filter(|item| item.visible)
            .map(|item| item.visible_window_bounds(item.bounds));
        state.renderer.set_inspection_outline(outline);
        state.renderer.draw_with_transitions(
            &self.scene,
            layout,
            &frame.texture,
            state.window.scale_factor() as f32,
            &self.input,
            &self.stack_transitions,
        )?;
        if animating {
            self.animation_wake = match self.transition.next_frame(elapsed) {
                Some(next) if next > elapsed => self.transition_epoch.checked_add(next),
                Some(_) => {
                    // UIKit can coalesce setNeedsDisplay issued inside drawRect.
                    // Wake a later event-loop turn to request the next animation frame.
                    #[cfg(target_os = "ios")]
                    {
                        Some(Instant::now() + Duration::from_millis(16))
                    }
                    #[cfg(not(target_os = "ios"))]
                    {
                        state.window.request_redraw();
                        None
                    }
                }
                None => {
                    self.transition = SceneTransition::default();
                    None
                }
            };
        }
        #[cfg(not(target_os = "android"))]
        state.window.pre_present_notify();
        state.renderer.queue.present(frame);
        if self.loader_frame_scene.as_ptr() != Rc::as_ptr(&self.scene) {
            self.loader_frame_scene = Rc::downgrade(&self.scene);
            if xrack_runtime::finish_loader_frame(&self.scene) {
                self.loader_redraw = true;
                state.window.request_redraw();
            }
        }
        let completion = state.renderer.take_painted_combo_completion();
        self.input
            .set_painted_combo_completion(completion.filter(|_| state.window.has_focus()));
        if state.renderer.needs_redraw() || self.stack_transitions.is_active() {
            #[cfg(target_os = "ios")]
            self.animation_wake
                .get_or_insert(Instant::now() + Duration::from_millis(16));
            #[cfg(target_os = "android")]
            {
                // Wake a later event-loop turn for the next visual frame.
                let next_frame = Instant::now() + Duration::from_millis(16);
                self.animation_wake = Some(
                    self.animation_wake
                        .map_or(next_frame, |wake| wake.min(next_frame)),
                );
            }
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            state.window.request_redraw();
        }
        if suboptimal {
            state.resize()?;
            self.relayout();
        } else if pointer_changed {
            self.publish_hover();
        }
        if animating && !self.transition.running(Duration::ZERO) {
            self.update_accessibility();
        }
        #[cfg(target_os = "ios")]
        if let Some(state) = &mut self.state
            && state
                .ios_scroll_announcement
                .as_ref()
                .is_some_and(|scroll| scroll.ready(&self.stack_transitions))
            && let Some(scroll) = state.ios_scroll_announcement.take()
            && let Some(layout) = &self.layout
            && self
                .input
                .accessibility_target(&self.scene, layout, &scroll.target.path)
                .as_ref()
                == Some(&scroll.target)
        {
            scroll.announce(&self.scene, layout);
        }
        Ok(())
    }
}

impl<C: Component> HostedWindow for Application<C> {
    fn refresh_after_desktop_update(&mut self) {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        self.refresh_after_update(false);
        if let Some(inspector) = self.component.window_inspector(&self.base_scene)
            && let Some(input) = inspector.take_input()
        {
            let result = self.apply_inspection_input(&inspector, input.action);
            let _ = input.reply.send(result);
        }
        if let Some(state) = &mut self.state {
            let revision = self
                .component
                .window_inspector(&self.base_scene)
                .map(|inspector| inspector.revision());
            if state.inspection_revision != revision {
                state.inspection_revision = revision;
                state.window.request_redraw();
            }
        }
    }

    fn user_event(&mut self, event: UserEvent) {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        self.process_user_event(event);
    }

    fn close(&mut self) {
        if let Some(state) = &self.state {
            state.set_visible(false);
        }
        self.close_async();
        self.component.take_window_close_request();
        self.component.take_window_commands();
        self.native_dialogs.close();
        self.suspend();
    }

    fn request_close(&mut self) -> bool {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        self.component.take_window_close_request();
        let accepted = self.component.close_requested();
        // Synchronous requests made during the decision belong to that decision.
        self.component.take_window_close_request();
        if !accepted {
            self.refresh_after_update(false);
        }
        accepted
    }

    fn take_close_request(&mut self) -> bool {
        self.component.take_window_close_request()
    }

    fn window_id(&self) -> Option<WindowId> {
        self.state.as_ref().map(|state| state.window.id())
    }

    fn native_window(&self) -> Option<Arc<Window>> {
        self.state.as_ref().map(|state| state.window.clone())
    }

    fn secondary_windows(&self) -> Option<Rc<[SecondaryWindowRegistration]>> {
        if self.state.is_some() && self.base_scene.boolean("visible", true) {
            self.component.secondary_windows()
        } else {
            None
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn resume(
        &mut self,
        event_loop: &ActiveEventLoop,
        graphics: &mut DesktopGraphics,
        owner: Option<&Window>,
    ) -> Result<WindowId, String> {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        if let Some(id) = self.window_id() {
            return Ok(id);
        }
        if self.base_scene.kind == "SystemTrayIcon" {
            return Err("use Desktop::register_tray for a SystemTrayIcon component".into());
        }
        #[cfg(not(target_os = "android"))]
        let attrs = Window::default_attributes()
            .with_title(self.base_scene.text("title").unwrap_or("XRack"))
            .with_visible(false);
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let attrs = WindowChrome::read(&self.base_scene).attributes(attrs, &self.base_scene);
        #[cfg(not(target_os = "android"))]
        let window = event_loop
            .create_window(window_owner::attributes(attrs, owner)?)
            .map(Arc::new)
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "android")]
        let window = event_loop.create_window()?;
        let events = self
            .events
            .as_ref()
            .ok_or("window events are unavailable")?;
        let adapter = events.accessibility(event_loop, &window)?;
        let mut state = WindowState::new(
            window,
            adapter,
            graphics,
            owner,
            #[cfg(target_os = "android")]
            event_loop.android_app().clone(),
        )?;
        #[cfg(target_os = "android")]
        {
            match android_text::TextInput::new(state.android_app.clone(), events.clone()) {
                Ok(input) => state.android_text = Some(input),
                Err(error) => eprintln!("XRack Android input: {error}"),
            }
        }
        #[cfg(target_os = "ios")]
        {
            state._ios_accessibility_observer =
                Some(ios::AccessibilityObserver::new(events.clone()));
            state._ios_scroll_to_top = ios::ScrollToTop::new(&state.window, events.clone());
            state.ios_edge_back = ios_back::EdgeBack::new(&state.window, events.clone());
            ios::observe_system_appearance(&state.window, events.clone());
            state.ios_keyboard = ios_keyboard::KeyboardInput::new(&state.window, events.clone());
            state.ios_pointer = ios_pointer::PointerInput::new(&state.window, events.clone());
        }
        let id = state.window.id();
        state.set_visible(self.base_scene.boolean("visible", true));
        self.focus_active = state.window.has_focus();
        self.input.set_editor_window_active(self.focus_active);
        self.component
            .publish_window_status(state.sample_window_status());
        self.state = Some(state);
        #[cfg(target_os = "ios")]
        {
            self.sync_ios_appearance();
            self.sync_ios_accessibility_preferences();
        }
        self.publish_viewport();
        self.sync_base_scene();
        self.relayout();
        Ok(id)
    }
    fn suspend(&mut self) {
        #[cfg(target_os = "android")]
        {
            self.modifiers = self.android_keyboard.clear();
        }
        if let Some(event) = self.scale_rotate.reset() {
            let scene = self.scene.clone();
            self.dispatch_scale_rotate(&scene, event);
        }
        self.focus_requests.drain();
        self.input.dismiss_choice();
        self.focus_observers.clear();
        self.hover_observers.clear();
        self.hover_pointer = None;
        self.cancel_all_pointers();
        self.scroll_requests.drain();
        self.viewport_observers.clear();
        self.input.clear_scroll_events();
        self.focus_active = false;
        self.animation_wake = None;
        self.cancel_direct_scroll();
        self.pending_clipboard = None;
        self.input.dismiss_tooltip();
        self.input.dismiss_choice();
        self.input.dismiss_menu();
        self.input.discard_spin_draft();
        #[cfg(target_os = "android")]
        if let Some(editor) = self.input.editor.as_mut() {
            editor.set_preedit(String::new(), None);
        }
        self.input.cancel_press();
        self.input.pointer_leave();
        self.input.clear_file_drag();
        self.layout = None;
        self.timers = TimerQueue::default();
        self.button_animations.sync([]);
        self.image_playback.sync(Instant::now(), []);
        self.media.sync([]);
        // The Android system picker can suspend the surface while the dialog is open.
        #[cfg(not(target_os = "android"))]
        self.native_dialogs.sync(&[]);
        self.state = None;
        self.component
            .publish_window_status(xrack_runtime::WindowStatus::default());
    }

    #[cfg(target_os = "ios")]
    fn set_ios_application_active(&mut self, active: bool) {
        self.ios_application_active = active;
        if !active {
            self.pending_text_drop = None;
            if let Some(bridge) = self
                .state
                .as_ref()
                .and_then(|state| state.file_drop.as_ref())
            {
                bridge.sync_text_source(None);
            }
            self.modifiers = ModifiersState::empty();
            self.input.set_editor_window_active(false);
            self.cancel_ios_pointer_button();
            self.cancel_all_pointers();
            self.cancel_scale_rotate();
            if self.hover_pointer.take().is_some() {
                self.input.pointer_leave();
                self.publish_hover();
            }
            if self
                .pending_clipboard
                .as_ref()
                .is_some_and(|request| !request.reads_text())
            {
                self.pending_clipboard = None;
            }
            return;
        }
        self.focus_active = self
            .state
            .as_ref()
            .is_some_and(|state| state.window.has_focus());
        self.input.set_editor_window_active(self.focus_active);
        self.sync_ios_appearance();
        if self.sync_ios_accessibility_preferences() {
            self.relayout();
        } else {
            self.update_editor();
        }
        self.finish_deferred_paste();
    }

    #[cfg(target_os = "android")]
    fn begin_input_batch(&mut self, started_at: Instant) {
        self.input_batch_started_at = Some(started_at);
    }

    #[cfg(target_os = "android")]
    fn android_event(&mut self, event: AndroidEvent) {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let Some(state) = &self.state else { return };
        match event {
            AndroidEvent::Touch(touch) => {
                let started = touch.phase == TouchPhase::Started;
                if started {
                    self.hover_pointer = None;
                }
                self.touch_event(touch);
                if started {
                    self.publish_hover();
                }
            }
            AndroidEvent::Key(physical, event) => {
                let (event, modifiers) = self.android_keyboard.translate(physical, event);
                self.modifiers = modifiers;
                self.keyboard_input(&event, false);
            }
            AndroidEvent::ConfigurationChanged => {
                let scheme = android_color_scheme(&state.android_app);
                let size = state.window.inner_size();
                self.cancel_scale_rotate();
                if let Some(scheme) = scheme {
                    self.update_system_appearance(scheme, None);
                }
                self.handle_window_event(WindowEvent::Resized(size), Instant::now());
            }
            AndroidEvent::ContentChanged => {
                self.publish_viewport();
                self.relayout();
            }
        }
    }

    fn tick(&mut self, now: Instant) {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        if let Some(state) = &mut self.state {
            self.component
                .publish_window_status(state.sample_window_status());
        }
        if self
            .viewport_observers
            .scroll_activity_deadline()
            .is_some_and(|wake| now >= wake)
            && let Some(layout) = &self.layout
        {
            self.viewport_observers
                .sync_scroll_activity(&self.scene, layout, &self.input, now);
        }
        self.refresh_dependencies();
        self.tick_button_holds(now);
        for _ in 0..64 {
            let Some(delivery) = self.button_animations.next(&self.scene, now) else {
                break;
            };
            delivery.apply(&mut self.component, &self.scene);
            self.refresh_after_update(true);
        }
        self.tick_table_hold(now);
        self.tick_pointer_hold(now);
        self.tick_touch_hold(now);
        self.tick_timers(now);
        self.tick_image_playback(now);
        if self.media.advance(now) {
            self.refresh_after_update(false);
        }
    }

    fn refresh_deadline(&mut self, now: Instant) -> Option<Instant> {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let window_deadline = self.apply_window_presentation(now);
        self.refresh_dependencies();
        if self.input.expire_password_echo(now) {
            self.refresh_editor(true);
        }
        if let (Some(state), Some(layout)) = (&self.state, &self.layout)
            && PointerCaret::advance_drag_scroll(
                &state.renderer.text,
                &mut self.input,
                &self.scene,
                layout,
                now,
            )
        {
            self.pending_clipboard = None;
            self.refresh_editor(false);
        }
        self.present_native_dialog();
        self.publish_focus();
        self.update_accessibility();
        if self.button_animations.take_redraw()
            && let Some(state) = &self.state
        {
            state.window.request_redraw();
        }
        let deadline = if let Some(state) = &self.state
            && state.window.has_focus()
            && let Some(layout) = &self.layout
        {
            if self.input.update_tooltip(&self.scene, layout, now) {
                state.window.request_redraw();
            }
            self.input.tooltip_deadline()
        } else {
            None
        };
        if self.animation_wake.is_some_and(|wake| now >= wake) {
            self.animation_wake = None;
            if let Some(state) = &self.state {
                state.window.request_redraw();
            }
        }
        let scroll_wake =
            self.state.as_ref().and_then(|state| {
                let wake =
                    self.input
                        .scroll_motion_deadline()
                        .into_iter()
                        .chain(self.layout.as_ref().and_then(|layout| {
                            self.input.drag_scroll_deadline(&self.scene, layout)
                        }))
                        .chain(self.layout.as_ref().and_then(|layout| {
                            self.input.table_column_move_deadline(&self.scene, layout)
                        }))
                        .chain(self.layout.as_ref().and_then(|layout| {
                            self.input.table_row_move_deadline(&self.scene, layout)
                        }))
                        .chain(self.layout.as_ref().and_then(|layout| {
                            self.input
                                .table_drag_selection_deadline(&self.scene, layout)
                        }))
                        .min()?;
                if now >= wake {
                    state.window.request_redraw();
                    return None;
                }
                Some(wake)
            });
        [
            deadline,
            window_deadline,
            (self.state.is_some() && self.component.has_window_commands()).then_some(now),
            self.animation_wake,
            scroll_wake,
            self.input.text_selection_scroll_deadline(),
            self.input.password_echo_deadline(),
            self.viewport_observers.scroll_activity_deadline(),
            self.input.table_touch_hold_deadline(),
            self.touch_hold
                .as_ref()
                .and_then(|selection| selection.deadline),
            self.input.delay_deadline(),
            self.input.button_hold_deadline(),
            self.button_animations.deadline(),
            self.pointer_capture
                .as_ref()
                .and_then(|capture| capture.sequence.press_and_hold_deadline()),
            self.timers.deadline(),
            self.image_playback.deadline(),
            self.media.deadline(now),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn window_event(&mut self, id: WindowId, event: WindowEvent) {
        self.window_event_at(id, event, Instant::now());
    }
}

impl<C: Component> Application<C> {
    fn window_event_at(&mut self, id: WindowId, event: WindowEvent, at: Instant) {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let Some(state) = self.state.as_mut().filter(|state| state.window.id() == id) else {
            return;
        };
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        state.accessibility.process_event(&state.window, &event);
        let scale = state.window.scale_factor();
        let publish_hover = match &event {
            WindowEvent::CursorMoved { position, .. } => {
                self.hover_pointer =
                    (!matches!(self.scroll_contact, Some(ScrollContact::Touch(..))))
                        .then_some(*position);
                true
            }
            WindowEvent::CursorLeft { .. }
            | WindowEvent::Focused(false)
            | WindowEvent::Occluded(true) => {
                self.hover_pointer = None;
                true
            }
            WindowEvent::Touch(touch) if touch.phase == TouchPhase::Started => {
                self.hover_pointer = None;
                true
            }
            _ => false,
        };
        let point = self
            .hover_pointer
            .map(|position| {
                let logical = position.to_logical::<f32>(scale);
                Point {
                    x: logical.x,
                    y: logical.y,
                }
            })
            .or_else(|| self.input.pointer_position());
        if let WindowEvent::MouseInput {
            button: MouseButton::Left,
            state: button,
            ..
        } = &event
        {
            if std::mem::take(&mut self.inspection_press) && *button == ElementState::Released {
                return;
            }
            if *button == ElementState::Pressed
                && self.scroll_contact.is_none()
                && self.pointer_capture.is_none()
                && let Some((layout, point)) = self.layout.as_ref().zip(point)
                && let Some(inspector) = self.component.window_inspector(&self.base_scene)
                && (inspector.picking() || self.modifiers.alt_key())
            {
                let path = self.input.inspection_path(&self.scene, layout, point);
                inspector.pick(path.as_deref());
                self.inspection_press = true;
                state.window.request_redraw();
                return;
            }
        }
        let mouse = match &event {
            WindowEvent::MouseInput { button, state, .. }
                if !matches!(self.scroll_contact, Some(ScrollContact::Touch(..))) =>
            {
                PointerContact::for_mouse(*button)
                    .zip(point)
                    .map(|(contact, point)| (contact, *state, point, Instant::now()))
            }
            _ => None,
        };
        let moved =
            matches!(&event, WindowEvent::CursorMoved { .. }) && self.hover_pointer.is_some();
        if matches!(
            &event,
            WindowEvent::Focused(false) | WindowEvent::Occluded(true)
        ) {
            #[cfg(target_os = "ios")]
            self.cancel_ios_pointer_button();
            self.cancel_all_pointers();
        } else if matches!(&event, WindowEvent::CursorLeft { .. })
            && self
                .pointer_capture
                .as_ref()
                .is_some_and(|capture| matches!(capture.contact, PointerContact::Mouse(_)))
        {
            self.cancel_pointer();
        }
        if let Some((contact, ElementState::Released, point, now)) = mouse {
            self.release_pointer(contact, point, now);
        }
        self.handle_window_event(event, at);
        if let Some((contact, ElementState::Pressed, point, now)) = mouse {
            self.start_pointer(contact, point, now);
        } else if moved && let Some(point) = point {
            self.move_mouse_pointer(point);
        }
        if publish_hover {
            self.publish_hover();
        }
    }
    #[cfg(target_os = "ios")]
    fn ios_pointer_event(&mut self, event: ios_pointer::PointerInputEvent) {
        use ios_pointer::PointerInputEvent;

        let Some(state) = &self.state else { return };
        if !self.ios_application_active
            || matches!(self.scroll_contact, Some(ScrollContact::Touch(..)))
        {
            return;
        }
        let id = state.window.id();
        let scale = state.window.scale_factor();
        let device_id = DeviceId::dummy();
        match event {
            PointerInputEvent::Moved(position) => {
                if self.ios_pressed_button.is_none() {
                    self.window_event(
                        id,
                        WindowEvent::CursorMoved {
                            device_id,
                            position: position.to_physical(scale),
                        },
                    );
                }
            }
            PointerInputEvent::Left => {
                if self.ios_pressed_button.is_none() && self.hover_pointer.is_some() {
                    self.window_event(id, WindowEvent::CursorLeft { device_id });
                }
            }
            PointerInputEvent::Button {
                button,
                position,
                phase,
                modifiers,
                at,
            } => {
                if phase == TouchPhase::Started && self.ios_pressed_button.is_none() {
                    self.ios_pressed_button = Some(button);
                } else if phase == TouchPhase::Started || self.ios_pressed_button != Some(button) {
                    return;
                }
                let position = position.to_physical(scale);
                let size = state.window.inner_size();
                self.modifiers = modifiers;
                self.window_event(
                    id,
                    WindowEvent::CursorMoved {
                        device_id,
                        position,
                    },
                );
                match phase {
                    TouchPhase::Started => self.window_event_at(
                        id,
                        WindowEvent::MouseInput {
                            device_id,
                            state: ElementState::Pressed,
                            button,
                        },
                        at,
                    ),
                    TouchPhase::Ended => {
                        self.ios_pressed_button = None;
                        self.window_event_at(
                            id,
                            WindowEvent::MouseInput {
                                device_id,
                                state: ElementState::Released,
                                button,
                            },
                            at,
                        );
                        if position.x < 0.0
                            || position.x >= f64::from(size.width)
                            || position.y < 0.0
                            || position.y >= f64::from(size.height)
                        {
                            self.window_event(id, WindowEvent::CursorLeft { device_id });
                        }
                    }
                    _ => {}
                }
            }
            PointerInputEvent::ButtonCancelled(button) => {
                if self.ios_pressed_button == Some(button) {
                    self.cancel_ios_pointer_button();
                    self.window_event(id, WindowEvent::CursorLeft { device_id });
                    self.update_editor();
                }
            }
            PointerInputEvent::Transform {
                position,
                delta,
                phase,
            } => {
                if phase == TouchPhase::Started && self.hover_pointer.is_none() {
                    let Some((position, layout)) = position.zip(self.layout.as_ref()) else {
                        return;
                    };
                    self.input.pointer_move(
                        &self.scene,
                        layout,
                        Point {
                            x: position.x as f32,
                            y: position.y as f32,
                        },
                    );
                }
                self.update_trackpad_gesture(device_id, phase, delta);
            }
            PointerInputEvent::Scroll {
                position,
                delta,
                phase,
            } => {
                if delta.x != 0.0 || delta.y != 0.0 {
                    // A trackpad pan's gesture location can move with the scroll.
                    // Keep routing from the last actual pointer location when available.
                    if self.hover_pointer.is_none()
                        && let Some(layout) = &self.layout
                    {
                        self.input.pointer_move(
                            &self.scene,
                            layout,
                            Point {
                                x: position.x as f32,
                                y: position.y as f32,
                            },
                        );
                    }
                    self.window_event(
                        id,
                        WindowEvent::MouseWheel {
                            device_id,
                            delta: MouseScrollDelta::PixelDelta(delta.to_physical(scale)),
                            phase,
                        },
                    );
                } else if matches!(phase, TouchPhase::Started | TouchPhase::Cancelled) {
                    self.input.reset_wheel_adjustment();
                }
            }
        }
    }

    #[cfg(target_os = "ios")]
    fn cancel_ios_pointer_button(&mut self) {
        let Some(button) = self.ios_pressed_button.take() else {
            return;
        };
        if let Some(contact) = PointerContact::for_mouse(button) {
            self.cancel_pointer_contact(contact);
        }
        if button == MouseButton::Left {
            self.inspection_press = false;
            self.cancel_drag();
            self.cancel_direct_scroll();
            self.input.cancel_press();
        }
    }

    fn handle_window_event(&mut self, event: WindowEvent, at: Instant) {
        if self.state.is_none() {
            return;
        }
        match event {
            WindowEvent::ThemeChanged(theme) => {
                self.update_system_appearance(color_scheme(Some(theme)), None);
            }
            change @ (WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) => {
                if matches!(change, WindowEvent::ScaleFactorChanged { .. }) {
                    self.cancel_scale_rotate();
                }
                if let Some(state) = self.state.as_mut()
                    && let Err(error) = state.resize()
                {
                    self.fail(error);
                    return;
                }
                self.publish_viewport();
                self.relayout();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw() {
                    self.fail(error);
                }
            }
            #[cfg(not(target_os = "android"))]
            WindowEvent::Touch(touch) => {
                #[cfg(target_os = "ios")]
                if touch.phase == TouchPhase::Started {
                    self.cancel_ios_pointer_button();
                }
                self.touch_event(touch);
            }
            #[cfg(not(target_os = "android"))]
            WindowEvent::PinchGesture {
                device_id,
                delta,
                phase,
            } => {
                self.update_trackpad_gesture(
                    device_id,
                    phase,
                    scale_rotate::TrackpadDelta::Scale(delta),
                );
            }
            #[cfg(not(target_os = "android"))]
            WindowEvent::RotationGesture {
                device_id,
                delta,
                phase,
            } => {
                self.update_trackpad_gesture(
                    device_id,
                    phase,
                    scale_rotate::TrackpadDelta::Rotation(f64::from(delta)),
                );
            }
            WindowEvent::CursorMoved { position, .. } => {
                #[cfg(target_os = "macos")]
                if let Some(state) = self.state.as_mut()
                    && state.resize_drag.is_some()
                    && state.update_client_resize(self.layout.as_ref().is_some_and(|layout| {
                        self.input.window_gestures_allowed(&self.scene, layout)
                    }))
                {
                    return;
                }
                let Some(state) = self.state.as_ref() else {
                    return;
                };
                let p = position.to_logical::<f32>(state.window.scale_factor());
                let point = Point { x: p.x, y: p.y };
                if let Some(layout) = &self.layout
                    && self.input.take_window_move(&self.scene, layout, point)
                {
                    #[cfg(not(target_os = "android"))]
                    let _ = state.window.drag_window();
                    return;
                }
                match &mut self.scroll_contact {
                    Some(ScrollContact::Touch(..)) => return,
                    Some(
                        ScrollContact::MousePan(last)
                        | ScrollContact::MouseControl(last)
                        | ScrollContact::MouseViewport(last),
                    ) => {
                        *last = point;
                        self.move_direct_scroll(point, Instant::now());
                        return;
                    }
                    None => {
                        if self.move_direct_scroll(point, Instant::now()) {
                            self.scroll_contact = Some(ScrollContact::MouseViewport(point));
                            return;
                        }
                    }
                }
                if self.move_drag(point, Instant::now()) {
                    return;
                }
                let Some(state) = self.state.as_ref() else {
                    return;
                };
                if let Some(layout) = &self.layout {
                    let before_resize = self.window_resize_cursor();
                    let changed = self.input.pointer_move(&self.scene, layout, point);
                    if self.input.table_row_move_drag(layout, Instant::now()) {
                        state.window.request_redraw();
                        return;
                    }
                    let drag_was_claimed = self.input.table_drag_selection_claimed();
                    if self.input.move_table_drag_selection(layout, Instant::now()) {
                        if drag_was_claimed {
                            state.window.request_redraw();
                        } else {
                            self.update_editor();
                        }
                        return;
                    }
                    if changed || before_resize != self.window_resize_cursor() {
                        state.window.request_redraw();
                    }
                }
                if let Some(update) = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.scroll_bar_drag(&self.scene, layout))
                {
                    self.apply_scroll_bar(update, false);
                    return;
                }
                if let Some(input) = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.split_drag(&self.scene, layout))
                {
                    self.apply_split_resize(input);
                    return;
                }
                if let Some(input) = self.layout.as_ref().and_then(|layout| {
                    self.input
                        .table_resize_drag(&self.scene, layout)
                        .or_else(|| self.input.table_row_resize_drag(&self.scene, layout))
                }) {
                    self.apply_control(input);
                    return;
                }
                if self
                    .layout
                    .as_ref()
                    .is_some_and(|layout| self.input.table_column_move_drag(layout, Instant::now()))
                {
                    if let Some(state) = &self.state {
                        state.window.request_redraw();
                    }
                    return;
                }
                if self.input.pressed.is_some()
                    && self.input.pressed == self.input.focused
                    && self.input.focused.as_ref().is_some_and(|target| {
                        self.scene
                            .at_path(&target.path)
                            .is_some_and(|node| self.input.is_text_editor(node, target))
                    })
                {
                    self.place_text_caret(point, true, None, true);
                    self.refresh_editor(false);
                }
                if let Some(input) = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.slider_drag(&self.scene, layout))
                {
                    self.adjust_range(input);
                } else if self.input.has_slider_preview() {
                    if let Some(state) = &self.state {
                        state.window.request_redraw();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if matches!(self.scroll_contact, Some(ScrollContact::Touch(..))) {
                    return;
                }
                self.last_button_click = None;
                self.cancel_drag();
                self.input.pointer_leave();
                if let Some(state) = &self.state {
                    state.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                if matches!(self.scroll_contact, Some(ScrollContact::Touch(..))) {
                    return;
                }
                self.cancel_direct_scroll();
                if matches!(phase, TouchPhase::Started | TouchPhase::Cancelled) {
                    self.input.reset_wheel_adjustment();
                }
                if self.input.cancel_delay() {
                    self.update_editor();
                }
                self.dismiss_tooltip();
                let Some(state) = self.state.as_ref() else {
                    return;
                };
                let scale = state.window.scale_factor();
                let (delta, lines) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (
                        Point {
                            x: -x * 28.0,
                            y: -y * 28.0,
                        },
                        Some(Point { x: -x, y: -y }),
                    ),
                    MouseScrollDelta::PixelDelta(position) => (
                        Point {
                            x: -(position.x / scale) as f32,
                            y: -(position.y / scale) as f32,
                        },
                        None,
                    ),
                };
                let rows = if delta.y == 0.0 {
                    0
                } else {
                    delta.y.signum() as isize * ((delta.y.abs() / 28.0).round() as isize).max(1)
                };
                if self.layout.as_ref().is_some_and(|layout| {
                    self.input.menu_scroll(&self.scene, layout, rows)
                        || self.input.choice_scroll(&self.scene, layout, rows)
                }) {
                    self.input.reset_wheel_adjustment();
                    self.update_editor();
                } else if !self.input.pointer_on_stack_slide() {
                    let event = self.wheel_event(delta, lines);
                    if self.dispatch_wheel(event) {
                        self.input.reset_wheel_adjustment();
                        return;
                    }
                    if self.adjust_wheel(event, Instant::now()) {
                        return;
                    }
                    if !self.modifiers.shift_key()
                        && delta.y.abs() > delta.x.abs()
                        && let Some(layout) = &self.layout
                    {
                        let distance = lines.map_or(delta.y, |lines| lines.y);
                        match self.input.tumbler_scroll(
                            &self.scene,
                            layout,
                            distance,
                            lines.is_some(),
                        ) {
                            TumblerScroll::Select(action) => {
                                self.apply_control(action);
                                return;
                            }
                            TumblerScroll::Pending => return,
                            TumblerScroll::Unhandled => {}
                        }
                    }
                    let remaining = self.scroll_text_area(delta);
                    if self.layout.as_ref().is_some_and(|layout| {
                        self.input.scroll_wheel(
                            layout,
                            remaining,
                            self.modifiers.shift_key() && delta.x == 0.0,
                        )
                    }) {
                        self.relayout_with_caret(false);
                    } else if remaining != delta {
                        self.refresh_editor(false);
                    }
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state: ElementState::Released,
                ..
            } => {
                if self.input.pointer_on_stack_slide()
                    || matches!(self.scroll_contact, Some(ScrollContact::Touch(..)))
                {
                    return;
                }
                self.cancel_direct_scroll();
                self.dismiss_tooltip();
                if self.layout.as_ref().is_some_and(|layout| {
                    self.input
                        .pointer_context_menu_changes_focus(&self.scene, layout)
                }) {
                    self.commit_spin();
                }
                if self
                    .layout
                    .as_ref()
                    .is_some_and(|layout| self.input.open_context_menu(&self.scene, layout, false))
                {
                    self.pending_clipboard = None;
                    self.update_editor();
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Back,
                state: ElementState::Released,
                ..
            } => {
                self.cancel_direct_scroll();
                self.back_in_stack();
            }
            WindowEvent::MouseInput {
                button: MouseButton::Middle,
                state,
                ..
            } => match (state, self.scroll_contact) {
                (ElementState::Pressed, None) if self.input.drag_position().is_none() => {
                    if let Some(point) = self.input.pointer_position() {
                        self.begin_direct_scroll(
                            ScrollContact::MousePan(point),
                            point,
                            Instant::now(),
                        );
                    }
                }
                (ElementState::Released, Some(ScrollContact::MousePan(point))) => {
                    self.end_direct_scroll(point, Instant::now());
                }
                _ => {}
            },
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } => match (state, self.scroll_contact) {
                (_, Some(ScrollContact::Touch(..))) => {}
                (
                    ElementState::Released,
                    Some(ScrollContact::MouseControl(point) | ScrollContact::MouseViewport(point)),
                ) => {
                    self.end_direct_scroll(point, Instant::now());
                }
                (ElementState::Pressed, None) => {
                    if self.window_chrome_press() {
                        return;
                    }
                    if let Some(point) = self.input.pointer_position() {
                        self.begin_direct_scroll(
                            ScrollContact::MouseControl(point),
                            point,
                            Instant::now(),
                        );
                    }
                    if self.scroll_contact.is_none() {
                        let click = self.input.pointer_position().map(|point| PointerClick {
                            touch_device: None,
                            point,
                            time: at,
                        });
                        self.left_pointer(state, click);
                        if !self.input.table_drag_selection_armed()
                            && let (Some(layout), Some(point)) =
                                (&self.layout, self.input.pointer_position())
                        {
                            self.input.arm_mouse_viewport_pan(
                                &self.scene,
                                layout,
                                point,
                                Instant::now(),
                            );
                        }
                    }
                }
                _ => {
                    if state == ElementState::Released
                        && (self.input.cancel_window_move()
                            || self
                                .state
                                .as_mut()
                                .is_some_and(WindowState::finish_client_resize))
                    {
                        self.layout_changed(false);
                        return;
                    }
                    self.cancel_direct_scroll();
                    let click = self.input.pointer_position().map(|point| PointerClick {
                        touch_device: None,
                        point,
                        time: at,
                    });
                    self.left_pointer(state, click);
                }
            },
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                if self.input.drag_action().is_some()
                    && let Some(point) = self.input.drag_position()
                {
                    self.move_drag(point, Instant::now());
                }
            }
            #[cfg(not(target_os = "android"))]
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                let event = KeyEvent::from(event);
                self.keyboard_input(&event, is_synthetic);
            }
            WindowEvent::Ime(Ime::Commit(value)) if !self.input.menu_open() => {
                if let Some(expected) = self.list_ime_focus.take() {
                    if self.input.list_typeahead_focus(&self.scene) == Some(&expected) {
                        self.list_typeahead_text(&value);
                    }
                } else if !self.list_typeahead_text(&value) {
                    self.edit_text_from(EditAction::Insert(&value), EditSource::Ime);
                }
            }
            WindowEvent::Ime(Ime::Preedit(value, cursor)) if !self.input.menu_open() => {
                self.pending_clipboard = None;
                if !value.is_empty() {
                    self.input.cancel_shortcut_sequence();
                    self.list_ime_focus = self.input.list_typeahead_focus(&self.scene).cloned();
                }
                if !value.is_empty() && self.input.editor.is_some() {
                    self.input.dismiss_choice();
                }
                if let Some(editor) = &mut self.input.editor {
                    editor.set_preedit(value, cursor);
                }
                self.update_editor();
            }
            WindowEvent::Ime(Ime::Disabled) => {
                self.list_ime_focus = None;
                if let Some(editor) = &mut self.input.editor {
                    editor.set_preedit(String::new(), None);
                }
                if let Some(state) = &self.state {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => {
                self.focus_active = focused;
                self.input.set_editor_window_active(focused);
                if !focused {
                    #[cfg(target_os = "ios")]
                    {
                        self.pending_text_drop = None;
                    }
                    #[cfg(target_os = "android")]
                    {
                        self.modifiers = self.android_keyboard.clear();
                    }
                    self.cancel_scale_rotate();
                    if let Some(state) = &mut self.state {
                        state.finish_client_resize();
                    }
                    self.cancel_drag();
                    self.input.cancel_shortcut_sequence();
                    self.input.set_painted_combo_completion(None);
                    self.cancel_direct_scroll();
                    self.dismiss_tooltip();
                    #[cfg(target_os = "ios")]
                    let preserve_paste = self
                        .pending_clipboard
                        .as_ref()
                        .is_some_and(PendingClipboard::reads_text);
                    #[cfg(not(target_os = "ios"))]
                    let preserve_paste = false;
                    if !preserve_paste {
                        self.pending_clipboard = None;
                        self.commit_spin();
                        self.input.cancel_table_edit();
                    }
                    self.input.dismiss_choice();
                    self.input.dismiss_menu();
                    self.input.cancel_press();
                    self.input.pointer_leave();
                    if let Some(editor) = &mut self.input.editor {
                        editor.set_preedit(String::new(), None);
                    }
                }
                self.update_editor();
                if focused {
                    self.finish_deferred_paste();
                }
            }
            WindowEvent::Occluded(occluded) => {
                if occluded {
                    self.cancel_scale_rotate();
                    if let Some(state) = &mut self.state {
                        state.finish_client_resize();
                    }
                    self.input.cancel_window_move();
                    self.cancel_drag();
                    self.input.set_painted_combo_completion(None);
                    self.cancel_direct_scroll();
                    self.input.finish_scroll_bar();
                } else {
                    if let Some(state) = &self.state {
                        state.window.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Open a native window and run a generated component on the main thread.
pub fn run<C: Component + 'static>(component: C) -> Result<(), String> {
    desktop::run_window(component, None)
}

/// Run one window and report its logical content size before the initial layout
/// and after a native resize or scale change. On iOS this also reports available
/// space when a docked keyboard appears or disappears.
pub fn run_with_viewport<C: Component + 'static>(
    component: C,
    handler: impl FnMut(&mut C, Size) + 'static,
) -> Result<(), String> {
    desktop::run_window(component, Some(Box::new(handler)))
}
