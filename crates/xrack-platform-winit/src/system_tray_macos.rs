use crate::{
    desktop::{TrayEvent, TrayEvents},
    system_tray::{TrayActivation, TrayMenuItem, TrayPresentation},
};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject},
    sel,
};
use objc2_app_kit::{
    NSBitmapFormat, NSBitmapImageRep, NSCellImagePosition, NSControlStateValueOff,
    NSControlStateValueOn, NSDeviceRGBColorSpace, NSImage, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusBarButton, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize, NSString};
use xrack_runtime::{Image, ImageFormat};

struct ActionState {
    events: TrayEvents,
    generation: u64,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "XRackSystemTrayTarget"]
    #[ivars = ActionState]
    struct ActionTarget;

    impl ActionTarget {
        #[unsafe(method(clicked:))]
        fn clicked(&self, _: &AnyObject) {
            self.ivars().events.post(TrayEvent::Activated(TrayActivation::Clicked { mount: self.ivars().generation }));
        }

        #[unsafe(method(selected:))]
        fn selected(&self, sender: &NSMenuItem) {
            if let Ok(item) = usize::try_from(sender.tag()) {
                self.ivars().events.post(TrayEvent::Activated(TrayActivation::Menu { revision: self.ivars().generation, item }));
            }
        }
    }
);

impl ActionTarget {
    fn new(mtm: MainThreadMarker, events: TrayEvents, generation: u64) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ActionState { events, generation });
        // NSObject initializes the retained target without changing its identity.
        unsafe { msg_send![super(object), init] }
    }
}

struct NativeMenu {
    menu: Retained<NSMenu>,
    _target: Retained<ActionTarget>,
}

pub(crate) struct NativeTray {
    bar: Retained<NSStatusBar>,
    item: Retained<NSStatusItem>,
    button: Retained<NSStatusBarButton>,
    target: Retained<ActionTarget>,
    events: TrayEvents,
    menu: Option<NativeMenu>,
}

impl NativeTray {
    pub fn new(events: TrayEvents, mount: u64) -> Result<Self, String> {
        let mtm =
            MainThreadMarker::new().ok_or("SystemTrayIcon requires the AppKit main thread")?;
        let bar = NSStatusBar::systemStatusBar();
        let item = bar.statusItemWithLength(NSVariableStatusItemLength);
        let Some(button) = item.button(mtm) else {
            bar.removeStatusItem(&item);
            return Err("AppKit returned a status item without a button".into());
        };
        let target = ActionTarget::new(mtm, events.clone(), mount);
        button.setImagePosition(NSCellImagePosition::ImageLeading);
        // The tray owns the action receiver until the status item is removed.
        unsafe {
            button.setTarget(Some(&target));
            button.setAction(Some(sel!(clicked:)));
        }
        Ok(Self {
            bar,
            item,
            button,
            target,
            events,
            menu: None,
        })
    }

    pub fn update(
        &mut self,
        current: &TrayPresentation,
        previous: Option<&TrayPresentation>,
        rebuild: Option<u64>,
    ) -> Result<(), String> {
        if previous
            .is_none_or(|last| last.icon != current.icon || last.template != current.template)
        {
            let image = native_image(current.icon, 18.0, current.template)?;
            self.button.setImage(image.as_deref());
        }
        if previous.is_none_or(|last| last.title != current.title) {
            self.button.setTitle(&NSString::from_str(&current.title));
        }
        if previous.is_none_or(|last| last.tooltip != current.tooltip) {
            let tooltip =
                (!current.tooltip.is_empty()).then(|| NSString::from_str(&current.tooltip));
            self.button.setToolTip(tooltip.as_deref());
        }
        if let Some(revision) = rebuild {
            let mtm = self.button.mtm();
            let menu = if current.menu.items.is_empty() {
                None
            } else {
                let target = ActionTarget::new(mtm, self.events.clone(), revision);
                let menu = native_menu(mtm, "", &current.menu.items, &target)?;
                Some(NativeMenu {
                    menu,
                    _target: target,
                })
            };
            self.item.setMenu(menu.as_ref().map(|menu| &*menu.menu));
            if menu.is_none() {
                // Restore the button route after detaching AppKit's menu.
                unsafe {
                    self.button.setTarget(Some(&self.target));
                    self.button.setAction(Some(sel!(clicked:)));
                }
            }
            self.menu = menu;
        }
        Ok(())
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        self.item.setMenu(None);
        // Clear the unretained target before releasing it.
        unsafe {
            self.button.setTarget(None);
        }
        self.bar.removeStatusItem(&self.item);
    }
}

fn native_menu(
    mtm: MainThreadMarker,
    label: &str,
    entries: &[TrayMenuItem],
    target: &ActionTarget,
) -> Result<Retained<NSMenu>, String> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(label));
    menu.setAutoenablesItems(false);
    let no_shortcut = NSString::from_str("");
    for entry in entries {
        let item = match entry {
            TrayMenuItem::Separator => NSMenuItem::separatorItem(mtm),
            TrayMenuItem::Submenu {
                label,
                enabled,
                items,
            } => {
                let title = NSString::from_str(label);
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &title,
                        None,
                        &no_shortcut,
                    )
                };
                let submenu = native_menu(mtm, label, items, target)?;
                item.setSubmenu(Some(&submenu));
                item.setEnabled(*enabled);
                item
            }
            TrayMenuItem::Command {
                index,
                label,
                enabled,
                checked,
                icon,
            } => {
                let title = NSString::from_str(label);
                // Every command uses the same retained receiver for this menu revision.
                let item = unsafe {
                    let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &title,
                        Some(sel!(selected:)),
                        &no_shortcut,
                    );
                    item.setTarget(Some(target));
                    item
                };
                item.setTag(
                    isize::try_from(*index)
                        .map_err(|_| "system tray menu contains too many commands")?,
                );
                item.setEnabled(*enabled);
                if let Some(checked) = checked {
                    item.setState(if *checked {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
                }
                if let Some(image) = native_image(*icon, 16.0, false)? {
                    item.setImage(Some(&image));
                }
                item
            }
        };
        menu.addItem(&item);
    }
    Ok(menu)
}

fn native_image(
    image: Image,
    points: f64,
    template: bool,
) -> Result<Option<Retained<NSImage>>, String> {
    let (format, bytes, [width, height]) = match image.frame(0) {
        Image::Embedded {
            format,
            bytes,
            size,
        }
        | Image::NineSliced {
            format,
            bytes,
            size,
            ..
        } => (format, bytes, size),
        Image::Empty | Image::Animated(_) => return Ok(None),
    };
    xrack_assets::validate_image_size([width, height])?;
    let scale = points / f64::from(width.max(height));
    let size = NSSize::new(f64::from(width) * scale, f64::from(height) * scale);
    let native = match format {
        ImageFormat::Png => NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))
            .ok_or("AppKit could not load the tray icon PNG")?,
        ImageFormat::Svg => {
            let pixels = [
                (size.width * 2.0).ceil().max(1.0) as u32,
                (size.height * 2.0).ceil().max(1.0) as u32,
            ];
            let raster = xrack_assets::rasterize_svg(bytes, pixels)?;
            let [width, height] = pixels.map(|value| value as isize);
            // A null planes pointer asks AppKit to allocate the specified interleaved RGBA buffer.
            let bitmap = unsafe { NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(), std::ptr::null_mut(), width, height, 8, 4, true, false,
                NSDeviceRGBColorSpace, NSBitmapFormat::empty(), width * 4, 32,
            ) }.ok_or("AppKit could not allocate the tray SVG bitmap")?;
            let data = bitmap.bitmapData();
            if data.is_null() {
                return Err("AppKit returned an empty tray SVG bitmap".into());
            }
            // The allocation above is width × height × 4 bytes, the format returned by rasterize_svg.
            unsafe {
                std::ptr::copy_nonoverlapping(raster.rgba.as_ptr(), data, raster.rgba.len());
            }
            bitmap.setSize(size);
            let native = NSImage::initWithSize(NSImage::alloc(), size);
            native.addRepresentation(&bitmap);
            native
        }
    };
    native.setSize(size);
    native.setTemplate(template);
    Ok(Some(native))
}
