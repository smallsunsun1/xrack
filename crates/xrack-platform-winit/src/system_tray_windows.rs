use crate::{
    desktop::{TrayEvent, TrayEvents},
    system_tray::{TrayActivation, TrayMenuItem, TrayPresentation, windows_payload},
};
use std::{
    cell::{Cell, RefCell},
    io, mem, ptr,
    rc::Rc,
    sync::OnceLock,
};
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
            DeleteObject, HBITMAP,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NIM_SETFOCUS, NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4,
                NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, EndMenu, GWLP_USERDATA,
                GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, HICON, HMENU, ICONINFO,
                InsertMenuItemW, KillTimer, MENUITEMINFOW, MFS_CHECKED, MFS_DISABLED,
                MFT_SEPARATOR, MFT_STRING, MIIM_BITMAP, MIIM_FTYPE, MIIM_ID, MIIM_STATE,
                MIIM_STRING, MIIM_SUBMENU, PostMessageW, RegisterClassW, RegisterWindowMessageW,
                SM_CXMENUCHECK, SM_CXSMICON, SM_CYMENUCHECK, SM_CYSMICON, SetForegroundWindow,
                SetTimer, SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
                TrackPopupMenuEx, WM_APP, WM_CONTEXTMENU, WM_NCCREATE, WM_NCDESTROY, WM_NULL,
                WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::w,
};
use xrack_runtime::Image;

const ICON_ID: u32 = 1;
const TRAY_MESSAGE: u32 = WM_APP + 1;
const POPUP_MESSAGE: u32 = WM_APP + 2;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const RETRY_TIMER: usize = 1;

struct WindowTarget {
    events: TrayEvents,
    mount: u64,
    taskbar_created: u32,
    menu: RefCell<Option<Rc<NativeMenu>>>,
    popup_active: Cell<bool>,
    pending_menu: Cell<Option<(i32, i32)>>,
    alive: Cell<bool>,
}

pub(crate) struct NativeTray {
    window: HWND,
    target: Rc<WindowTarget>,
    icon: Option<Icon>,
    tooltip: [u16; 128],
    registered: bool,
}

impl NativeTray {
    pub fn new(events: TrayEvents, mount: u64) -> Result<Self, String> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err(win32_error("GetModuleHandleW"));
        }
        static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
        REGISTERED
            .get_or_init(|| {
                let class = WNDCLASSW {
                    hInstance: instance,
                    lpszClassName: w!("XRackSystemTraySink"),
                    lpfnWndProc: Some(window_proc),
                    ..Default::default()
                };
                if unsafe { RegisterClassW(&class) } == 0 {
                    return Err(win32_error("RegisterClassW"));
                }
                Ok(())
            })
            .clone()?;
        let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        if taskbar_created == 0 {
            return Err(win32_error("RegisterWindowMessageW"));
        }
        let target = Rc::new(WindowTarget {
            events,
            mount,
            taskbar_created,
            menu: RefCell::default(),
            popup_active: Cell::new(false),
            pending_menu: Cell::new(None),
            alive: Cell::new(true),
        });
        // A hidden top-level window receives both the Shell callback and the TaskbarCreated broadcast.
        // The target lives until after DestroyWindow clears GWLP_USERDATA.
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("XRackSystemTraySink"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                Rc::as_ptr(&target).cast(),
            )
        };
        if window.is_null() {
            return Err(win32_error("CreateWindowExW"));
        }
        Ok(Self {
            window,
            target,
            icon: None,
            tooltip: [0; 128],
            registered: false,
        })
    }

    pub fn update(
        &mut self,
        current: &TrayPresentation,
        previous: Option<&TrayPresentation>,
        rebuild: Option<u64>,
    ) -> Result<(), String> {
        if let Some(revision) = rebuild {
            self.target
                .menu
                .replace(NativeMenu::build(current, revision)?.map(Rc::new));
        }
        let icon_changed = previous.is_none_or(|last| last.icon != current.icon);
        if icon_changed {
            self.icon = Some(Icon::new(current.icon)?);
        }
        let tooltip = windows_payload::tooltip(if current.tooltip.is_empty() {
            &current.title
        } else {
            &current.tooltip
        });
        if icon_changed || tooltip != self.tooltip || !self.registered {
            self.tooltip = tooltip;
            self.publish()?;
        }
        Ok(())
    }

    pub fn restore(
        &mut self,
        current: &TrayPresentation,
        taskbar_created: bool,
    ) -> Result<(), String> {
        if taskbar_created {
            // Explorer sends this message after both a restart and a primary-display DPI change.
            self.icon = Some(Icon::new(current.icon)?);
            let revision = self.target.menu.borrow().as_ref().map(|menu| menu.revision);
            if let Some(revision) = revision {
                self.target
                    .menu
                    .replace(NativeMenu::build(current, revision)?.map(Rc::new));
            }
            self.registered = false;
        }
        if !self.registered {
            self.publish()?;
        }
        Ok(())
    }

    fn identity(&self) -> NOTIFYICONDATAW {
        icon_identity(self.window)
    }

    fn publish(&mut self) -> Result<(), String> {
        let Some(icon) = &self.icon else {
            return Ok(());
        };
        let data = NOTIFYICONDATAW {
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: TRAY_MESSAGE,
            hIcon: icon.0,
            szTip: self.tooltip,
            ..self.identity()
        };
        if self.registered && unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } != 0 {
            return Ok(());
        }
        self.registered = false;
        // The icon may still exist after a DPI broadcast, so a duplicate add can fall back to modify.
        if unsafe {
            Shell_NotifyIconW(NIM_ADD, &data) != 0 || Shell_NotifyIconW(NIM_MODIFY, &data) != 0
        } {
            let mut version = self.identity();
            version.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            if unsafe { Shell_NotifyIconW(NIM_SETVERSION, &version) } == 0 {
                unsafe {
                    Shell_NotifyIconW(NIM_DELETE, &self.identity());
                }
                return Err("Shell_NotifyIconW could not negotiate version 4".into());
            }
            self.registered = true;
            unsafe {
                KillTimer(self.window, RETRY_TIMER);
            }
        } else if unsafe { SetTimer(self.window, RETRY_TIMER, 500, None) } == 0 {
            return Err(win32_error("SetTimer for notification area registration"));
        }
        Ok(())
    }

    pub fn request_menu(&self, anchor: (i32, i32)) {
        // Defer the native modal loop until winit's ApplicationHandler has returned.
        if self.target.menu.borrow().is_some()
            && !self.target.popup_active.get()
            && self.target.pending_menu.replace(Some(anchor)).is_none()
            && unsafe { PostMessageW(self.window, POPUP_MESSAGE, 0, 0) } == 0
        {
            self.target.pending_menu.set(None);
        }
    }
}

fn keyboard_anchor(window: HWND) -> Option<(i32, i32)> {
    let identifier = NOTIFYICONIDENTIFIER {
        cbSize: mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: window,
        uID: ICON_ID,
        ..Default::default()
    };
    let mut rect = RECT::default();
    if unsafe { Shell_NotifyIconGetRect(&identifier, &mut rect) } >= 0 {
        Some((rect.left + (rect.right - rect.left) / 2, rect.bottom))
    } else {
        let mut point = POINT::default();
        (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
    }
}

fn icon_identity(window: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: ICON_ID,
        ..Default::default()
    }
}

fn display_popup(window: HWND, target: &WindowTarget, anchor: (i32, i32)) {
    let Some(menu) = target.menu.borrow().clone() else {
        return;
    };
    let Some((x, y)) = (if anchor == (-1, -1) {
        keyboard_anchor(window)
    } else {
        Some(anchor)
    }) else {
        return;
    };
    if !target.alive.get() || target.popup_active.replace(true) {
        return;
    }
    // The popup keeps this menu revision and its bitmaps alive while processing native events.
    let command = unsafe {
        SetForegroundWindow(window);
        if target.alive.get() {
            TrackPopupMenuEx(
                menu.handle.0,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                x,
                y,
                window,
                ptr::null(),
            )
        } else {
            0
        }
    };
    target.popup_active.set(false);
    if !target.alive.get() {
        return;
    }
    unsafe {
        PostMessageW(window, WM_NULL, 0, 0);
        Shell_NotifyIconW(NIM_SETFOCUS, &icon_identity(window));
    }
    if let Some(item) = usize::try_from(command)
        .ok()
        .and_then(|id| id.checked_sub(1))
    {
        target
            .events
            .post(TrayEvent::Activated(TrayActivation::Menu {
                revision: menu.revision,
                item,
            }));
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        // Remove the shell icon before destroying its callback window and releasing the bitmap.
        self.target.alive.set(false);
        unsafe {
            if self.target.popup_active.get() {
                EndMenu();
            }
            KillTimer(self.window, RETRY_TIMER);
            Shell_NotifyIconW(NIM_DELETE, &self.identity());
            DestroyWindow(self.window);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // CreateWindowExW supplies CREATESTRUCTW with the WindowTarget as lpCreateParams.
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    } else if message == WM_NCDESTROY {
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        }
    } else {
        let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const WindowTarget;
        // Only this window procedure sets the pointer, which stays valid through DestroyWindow.
        if let Some(target) = unsafe { pointer.as_ref() } {
            if message == POPUP_MESSAGE {
                let Some(anchor) = target.pending_menu.take() else {
                    return 0;
                };
                // The native loop can close this tray before it returns. Take a temporary strong reference.
                let target = unsafe {
                    Rc::increment_strong_count(pointer);
                    Rc::from_raw(pointer)
                };
                display_popup(window, &target, anchor);
                return 0;
            }
            if message == target.taskbar_created || message == WM_TIMER && wparam == RETRY_TIMER {
                target.events.post(TrayEvent::Restore {
                    mount: target.mount,
                    taskbar_created: message == target.taskbar_created,
                });
                return 0;
            }
            if message == TRAY_MESSAGE {
                if let Some((notification, anchor)) =
                    windows_payload::notification(wparam, lparam, ICON_ID)
                {
                    match notification {
                        NIN_SELECT | NIN_KEYSELECT => {
                            target
                                .events
                                .post(TrayEvent::Activated(TrayActivation::Clicked {
                                    mount: target.mount,
                                }))
                        }
                        WM_CONTEXTMENU => target.events.post(TrayEvent::ContextMenu {
                            mount: target.mount,
                            anchor,
                        }),
                        _ => {}
                    }
                }
                return 0;
            }
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

struct Icon(HICON);

impl Icon {
    fn new(image: Image) -> Result<Self, String> {
        let size = unsafe { [GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)] }
            .map(|dimension| dimension.clamp(16, 64) as u32);
        let raster = xrack_assets::rasterize_icon(image, size)?.ok_or("empty notification icon")?;
        let bitmap = Bitmap::new(&raster)?;
        let stride = size[0].div_ceil(16) as usize * 2;
        let mut mask_bits = vec![0u8; stride * size[1] as usize];
        for (y, row) in raster.rgba.chunks_exact(size[0] as usize * 4).enumerate() {
            for (x, pixel) in row.chunks_exact(4).enumerate() {
                if pixel[3] == 0 {
                    mask_bits[y * stride + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        // The monochrome mask has word-aligned rows and one bit per pixel.
        let mask = Bitmap::from_handle(unsafe {
            CreateBitmap(
                size[0] as i32,
                size[1] as i32,
                1,
                1,
                mask_bits.as_ptr().cast(),
            )
        })?;
        let info = ICONINFO {
            fIcon: 1,
            hbmMask: mask.0,
            hbmColor: bitmap.0,
            ..Default::default()
        };
        let icon = unsafe { CreateIconIndirect(&info) };
        if icon.is_null() {
            Err(win32_error("CreateIconIndirect"))
        } else {
            Ok(Self(icon))
        }
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.0);
        }
    }
}

struct Bitmap(HBITMAP);

impl Bitmap {
    fn from_handle(handle: HBITMAP) -> Result<Self, String> {
        if handle.is_null() {
            Err(win32_error("creating tray bitmap"))
        } else {
            Ok(Self(handle))
        }
    }

    fn new(raster: &xrack_assets::Raster) -> Result<Self, String> {
        let [width, height] = raster.size;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = ptr::null_mut();
        let bitmap = Self::from_handle(unsafe {
            CreateDIBSection(
                ptr::null_mut(),
                &info,
                DIB_RGB_COLORS,
                &mut pixels,
                ptr::null_mut(),
                0,
            )
        })?;
        if pixels.is_null() {
            return Err("CreateDIBSection returned an empty pixel buffer".into());
        }
        // A top-down 32-bit DIB has four bytes per pixel with no row padding.
        let output = unsafe {
            std::slice::from_raw_parts_mut(
                pixels.cast::<u8>(),
                width as usize * height as usize * 4,
            )
        };
        for (source, target) in raster.rgba.chunks_exact(4).zip(output.chunks_exact_mut(4)) {
            target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
        }
        Ok(bitmap)
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}

struct Menu(HMENU);

impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            DestroyMenu(self.0);
        }
    }
}

struct NativeMenu {
    handle: Menu,
    revision: u64,
    _bitmaps: Vec<Bitmap>,
}

impl NativeMenu {
    fn build(presentation: &TrayPresentation, revision: u64) -> Result<Option<Self>, String> {
        if presentation.menu.items.is_empty() {
            return Ok(None);
        }
        let mut bitmaps = Vec::new();
        let size = unsafe {
            [
                GetSystemMetrics(SM_CXMENUCHECK),
                GetSystemMetrics(SM_CYMENUCHECK),
            ]
        }
        .map(|dimension| dimension.clamp(12, 48) as u32);
        let handle = popup_menu(&presentation.menu.items, size, &mut bitmaps)?;
        Ok(Some(Self {
            handle,
            revision,
            _bitmaps: bitmaps,
        }))
    }
}

fn popup_menu(
    items: &[TrayMenuItem],
    icon_size: [u32; 2],
    bitmaps: &mut Vec<Bitmap>,
) -> Result<Menu, String> {
    let menu = Menu(unsafe { CreatePopupMenu() });
    if menu.0.is_null() {
        return Err(win32_error("CreatePopupMenu"));
    }
    for (position, entry) in items.iter().enumerate() {
        let mut info = MENUITEMINFOW {
            cbSize: mem::size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE,
            ..Default::default()
        };
        let mut label = Vec::new();
        let mut submenu = None;
        match entry {
            TrayMenuItem::Separator => info.fType = MFT_SEPARATOR,
            TrayMenuItem::Submenu {
                label: title,
                enabled,
                items,
            } => {
                label = windows_payload::menu_label(title);
                let nested = popup_menu(items, icon_size, bitmaps)?;
                info.fMask |= MIIM_SUBMENU;
                info.hSubMenu = nested.0;
                info.fState = if *enabled { 0 } else { MFS_DISABLED };
                submenu = Some(nested);
            }
            TrayMenuItem::Command {
                index,
                label: title,
                enabled,
                checked,
                icon,
            } => {
                label = windows_payload::menu_label(title);
                info.fMask |= MIIM_ID;
                info.wID = u32::try_from(*index)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                    .filter(|id| *id <= i32::MAX as u32)
                    .ok_or("system tray menu contains too many commands")?;
                info.fState = if *enabled { 0 } else { MFS_DISABLED };
                if *checked == Some(true) {
                    info.fState |= MFS_CHECKED;
                }
                if let Some(raster) = xrack_assets::rasterize_icon(*icon, icon_size)? {
                    let bitmap = Bitmap::new(&raster)?;
                    info.fMask |= MIIM_BITMAP;
                    info.hbmpItem = bitmap.0;
                    bitmaps.push(bitmap);
                }
            }
        }
        if !label.is_empty() {
            info.fMask |= MIIM_STRING | MIIM_STATE;
            info.fType = MFT_STRING;
            info.dwTypeData = label.as_mut_ptr();
        }
        let position =
            u32::try_from(position).map_err(|_| "system tray menu contains too many items")?;
        if unsafe { InsertMenuItemW(menu.0, position, 1, &info) } == 0 {
            return Err(win32_error("InsertMenuItemW"));
        }
        if let Some(submenu) = submenu {
            // DestroyMenu recursively owns submenus once InsertMenuItemW succeeds.
            mem::forget(submenu);
        }
    }
    Ok(menu)
}

fn win32_error(operation: &str) -> String {
    format!("{operation}: {}", io::Error::last_os_error())
}
