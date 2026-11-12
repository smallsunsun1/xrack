use crate::{
    LocalExecutor,
    desktop::{TrayEvent, TrayEvents},
};
use std::{borrow::Cow, rc::Rc};
use xrack_runtime::{ActionKey, Component, ControlAction, ImageAsset, Node};

#[cfg(target_os = "linux")]
use crate::system_tray_linux::NativeTray;
#[cfg(target_os = "macos")]
use crate::system_tray_macos::NativeTray;
#[cfg(target_os = "windows")]
use crate::system_tray_windows::NativeTray;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
struct NativeTray;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
impl NativeTray {
    fn new(_: TrayEvents, _: u64) -> Result<Self, String> {
        Err(unsupported())
    }
    fn update(
        &mut self,
        _: &TrayPresentation,
        _: Option<&TrayPresentation>,
        _: Option<u64>,
    ) -> Result<(), String> {
        Err(unsupported())
    }
}

pub(crate) fn unsupported() -> String {
    "SystemTrayIcon is available on the XRack Linux, macOS and Windows desktop backends".into()
}

#[cfg(any(target_os = "windows", test))]
pub(crate) mod windows_payload {
    pub fn notification(wparam: usize, lparam: isize, icon: u32) -> Option<(u32, (i32, i32))> {
        let payload = lparam as u32;
        (payload >> 16 == icon).then(|| {
            (
                payload & 0xffff,
                (
                    i32::from(wparam as u16 as i16),
                    i32::from((wparam >> 16) as u16 as i16),
                ),
            )
        })
    }

    pub fn tooltip(text: &str) -> [u16; 128] {
        let mut buffer = [0; 128];
        let mut end = 0;
        for character in text.chars().take_while(|character| *character != '\0') {
            let mut encoded = [0; 2];
            let units = character.encode_utf16(&mut encoded);
            if end + units.len() >= buffer.len() {
                break;
            }
            buffer[end..end + units.len()].copy_from_slice(units);
            end += units.len();
        }
        buffer
    }

    pub fn menu_label(text: &str) -> Vec<u16> {
        let text = text
            .split('\0')
            .next()
            .unwrap_or_default()
            .replace('&', "&&");
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
    allow(dead_code)
)]
pub(crate) enum TrayActivation {
    Clicked { mount: u64 },
    Menu { revision: u64, item: usize },
}

#[derive(Debug, PartialEq, Eq)]
enum ResolvedActivation<'a> {
    Clicked,
    Menu(&'a [usize]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrayPresentation {
    pub icon: ImageAsset,
    pub template: bool,
    pub title: String,
    pub tooltip: String,
    pub menu: MenuProjection,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuProjection {
    pub items: Vec<TrayMenuItem>,
    routes: Vec<MenuRoute>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TrayMenuItem {
    Separator,
    Submenu {
        label: String,
        enabled: bool,
        items: Vec<TrayMenuItem>,
    },
    Command {
        index: usize,
        label: String,
        enabled: bool,
        checked: Option<bool>,
        icon: ImageAsset,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MenuRoute {
    path: Vec<usize>,
    id: Option<Cow<'static, str>>,
    action: Option<ActionKey>,
}

impl TrayPresentation {
    fn project(scene: &Node) -> Option<Self> {
        let icon = scene
            .image_asset("icon")
            .filter(|icon| !icon.size().contains(&0))?;
        if !scene.is_visible() {
            return None;
        }
        let mut menu = MenuProjection::default();
        if let Some((index, node)) = scene
            .children
            .iter()
            .enumerate()
            .find(|(_, node)| node.kind == "Menu" && node.is_visible())
        {
            menu.items = project_menu(
                node,
                &mut vec![index],
                node.boolean("enabled", true),
                &mut menu.routes,
            );
        }
        Some(Self {
            icon,
            menu,
            template: scene.boolean("icon-template", false),
            title: scene.text("title").unwrap_or_default().to_owned(),
            tooltip: scene.text("tooltip").unwrap_or_default().to_owned(),
        })
    }

    fn resolve_activation(
        &self,
        scene: &Node,
        event: TrayActivation,
        mount: u64,
        revision: u64,
    ) -> Option<ResolvedActivation<'_>> {
        match event {
            TrayActivation::Clicked { mount: source } if source == mount => {
                Some(ResolvedActivation::Clicked)
            }
            TrayActivation::Menu {
                revision: source,
                item,
            } if source == revision => {
                let route = self.menu.routes.get(item)?;
                let node = scene.interactive_at_path(&route.path)?;
                (node.kind == "MenuItem"
                    && node.element_id() == route.id.as_deref()
                    && node.action == route.action)
                    .then_some(ResolvedActivation::Menu(&route.path))
            }
            _ => None,
        }
    }
}

fn project_menu(
    menu: &Node,
    path: &mut Vec<usize>,
    enabled: bool,
    routes: &mut Vec<MenuRoute>,
) -> Vec<TrayMenuItem> {
    let mut items = Vec::new();
    for (index, node) in menu
        .children
        .iter()
        .enumerate()
        .filter(|(_, node)| node.is_visible())
    {
        path.push(index);
        let enabled = enabled && node.boolean("enabled", true);
        let entry = match node.kind {
            "MenuSeparator" => Some(TrayMenuItem::Separator),
            "Menu" => Some(TrayMenuItem::Submenu {
                label: node.text("title").unwrap_or_default().to_owned(),
                enabled,
                items: project_menu(node, path, enabled, routes),
            }),
            "MenuItem" => {
                let index = routes.len();
                routes.push(MenuRoute {
                    path: path.clone(),
                    id: node.element_id_owned(),
                    action: node.action,
                });
                Some(TrayMenuItem::Command {
                    index,
                    enabled,
                    label: node.text("text").unwrap_or_default().to_owned(),
                    checked: node
                        .boolean("checkable", false)
                        .then(|| node.boolean("checked", false)),
                    icon: node.image_asset("icon").unwrap_or_default(),
                })
            }
            _ => None,
        };
        path.pop();
        items.extend(entry);
    }
    items
}

pub(crate) struct TrayHost {
    component: Box<dyn Component>,
    scene: Rc<Node>,
    presentation: Option<TrayPresentation>,
    native: Option<NativeTray>,
    mount: u64,
    revision: u64,
    events: TrayEvents,
    tasks: Rc<LocalExecutor>,
}

impl TrayHost {
    pub fn new(component: Box<dyn Component>, events: TrayEvents) -> Result<Self, String> {
        let scene = component.render_shared();
        if scene.kind != "SystemTrayIcon" {
            return Err("register_tray requires a component rooted in SystemTrayIcon".into());
        }
        let wake = events.clone();
        let tasks = LocalExecutor::new(move || {
            wake.post(TrayEvent::PollTasks);
        });
        Ok(Self {
            component,
            scene,
            presentation: None,
            native: None,
            mount: 0,
            revision: 0,
            events,
            tasks,
        })
    }

    #[cfg(target_os = "linux")]
    pub fn has_mount(&self, mount: u64) -> bool {
        self.native.is_some() && self.mount == mount
    }

    pub fn suspend(&mut self) {
        self.native = None;
        self.presentation = None;
    }

    pub fn sync(&mut self) -> Result<(), String> {
        let scene = self.component.render_shared();
        if self.native.is_some() && Rc::ptr_eq(&self.scene, &scene) {
            return Ok(());
        }
        self.scene = scene;
        let Some(presentation) = TrayPresentation::project(&self.scene) else {
            self.suspend();
            return Ok(());
        };
        if self.presentation.as_ref() == Some(&presentation) {
            return Ok(());
        }
        let rebuild = self
            .presentation
            .as_ref()
            .is_none_or(|last| last.menu != presentation.menu);
        if rebuild {
            self.revision = self
                .revision
                .checked_add(1)
                .ok_or("tray menu revisions exhausted")?;
        }
        if self.native.is_none() {
            self.mount = self
                .mount
                .checked_add(1)
                .ok_or("tray mount identities exhausted")?;
            #[cfg(target_os = "linux")]
            let _scope = self.tasks.enter();
            self.native = Some(NativeTray::new(self.events.clone(), self.mount)?);
        }
        self.native.as_mut().unwrap().update(
            &presentation,
            self.presentation.as_ref(),
            rebuild.then_some(self.revision),
        )?;
        self.presentation = Some(presentation);
        Ok(())
    }

    pub fn activate(&mut self, event: TrayActivation) -> bool {
        let _scope = self.tasks.enter();
        let Some(presentation) = &self.presentation else {
            return false;
        };
        match presentation.resolve_activation(&self.scene, event, self.mount, self.revision) {
            Some(ResolvedActivation::Clicked) => {
                self.component.dispatch_at(&self.scene, &[], "clicked")
            }
            Some(ResolvedActivation::Menu(path)) => {
                self.component
                    .control_at(&self.scene, path, ControlAction::ActivateMenuItem)
            }
            None => false,
        }
    }

    pub fn poll_tasks(&self) {
        let _scope = self.tasks.enter();
        self.tasks.poll_ready();
    }

    #[cfg(target_os = "windows")]
    pub fn request_menu(&self, mount: u64, anchor: (i32, i32)) {
        if mount == self.mount
            && let Some(tray) = &self.native
        {
            tray.request_menu(anchor);
        }
    }

    #[cfg(target_os = "windows")]
    pub fn restore(&mut self, mount: u64, taskbar_created: bool) -> Result<(), String> {
        if mount == self.mount
            && let (Some(tray), Some(presentation)) = (&mut self.native, &self.presentation)
        {
            tray.restore(presentation, taskbar_created)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_runtime::{Image, ImageFormat, Value};

    #[test]
    fn windows_payload_preserves_signed_anchors_and_unicode_boundaries() {
        use windows_payload::{menu_label, notification, tooltip};
        let wparam = usize::from((-1600i16) as u16) | (usize::from((-30i16) as u16) << 16);
        assert_eq!(
            notification(wparam, 0x0001_007b, 1),
            Some((0x7b, (-1600, -30)))
        );
        assert_eq!(notification(wparam, 0x0002_007b, 1), None);
        let fits = "a".repeat(125) + "😀";
        let buffer = tooltip(&fits);
        assert_eq!(String::from_utf16(&buffer[..127]).unwrap(), fits);
        assert_eq!(buffer[127], 0);
        let too_long = "a".repeat(126) + "😀";
        assert_eq!(&tooltip(&too_long)[126..], &[0, 0]);
        assert_eq!(
            String::from_utf16(&menu_label("A & B\0C")).unwrap(),
            "A && B\0"
        );
    }

    fn tray() -> Node {
        let mut tray = Node::new("SystemTrayIcon", None);
        tray.properties.insert(
            "icon",
            Value::Image(Image::Embedded {
                format: ImageFormat::Png,
                bytes: b"projection only",
                size: [1, 1],
            }),
        );
        tray
    }

    fn command(id: &'static str) -> Node {
        let mut command = Node::new("MenuItem", Some(id));
        command
            .properties
            .insert("text", "Toggle".to_owned().into());
        command.properties.insert("checkable", true.into());
        command.properties.insert("checked", true.into());
        command
    }

    #[test]
    fn revisions_distinguish_identical_labels_and_reject_disabled_routes() {
        let mut tray = tray();
        let mut menu = Node::new("Menu", None);
        let mut next = command("next");
        next.properties.insert("visible", false.into());
        let mut disabled = Node::new("Menu", Some("disabled"));
        disabled.properties.insert("enabled", false.into());
        disabled.children.push(command("disabled-command"));
        menu.children.extend([command("previous"), next, disabled]);
        tray.children.push(menu);
        let first = TrayPresentation::project(&tray).unwrap();
        assert_eq!(
            first.resolve_activation(&tray, TrayActivation::Clicked { mount: 1 }, 1, 10),
            Some(ResolvedActivation::Clicked)
        );
        assert!(matches!(
            &first.menu.items[0],
            TrayMenuItem::Command {
                checked: Some(true),
                enabled: true,
                ..
            }
        ));
        let activate = |item| TrayActivation::Menu { revision: 10, item };
        assert_eq!(
            first.resolve_activation(&tray, activate(0), 1, 10),
            Some(ResolvedActivation::Menu(&[0, 0]))
        );
        assert_eq!(first.resolve_activation(&tray, activate(1), 1, 10), None);

        tray.children[0].children[0]
            .properties
            .insert("visible", false.into());
        tray.children[0].children[1]
            .properties
            .insert("visible", true.into());
        let second = TrayPresentation::project(&tray).unwrap();
        assert_eq!(first.menu.items, second.menu.items);
        assert_ne!(first.menu, second.menu);
        assert_eq!(second.resolve_activation(&tray, activate(0), 1, 11), None);
        assert_eq!(
            second.resolve_activation(
                &tray,
                TrayActivation::Menu {
                    revision: 11,
                    item: 0
                },
                1,
                11
            ),
            Some(ResolvedActivation::Menu(&[0, 1]))
        );
    }

    #[test]
    fn a_stale_native_menu_route_rejects_a_different_runtime_id_at_the_same_path() {
        let mut tray = tray();
        let mut menu = Node::new("Menu", None);
        let mut item = command("generated");
        item.set_owned_id(Some("original".into()));
        menu.children.push(item);
        tray.children.push(menu);
        let presentation = TrayPresentation::project(&tray).unwrap();
        let event = || TrayActivation::Menu {
            revision: 10,
            item: 0,
        };
        assert_eq!(
            presentation.resolve_activation(&tray, event(), 1, 10),
            Some(ResolvedActivation::Menu(&[0, 0]))
        );

        tray.children[0].children[0].set_owned_id(Some("replacement".into()));
        assert_eq!(presentation.resolve_activation(&tray, event(), 1, 10), None);
    }

    #[test]
    fn tray_projection_accepts_loaded_root_and_menu_icons() {
        use xrack_runtime::LoadedImage;

        let mut tray = tray();
        let mut menu = Node::new("Menu", None);
        menu.children.push(command("toggle"));
        tray.children.push(menu);
        let loaded = ImageAsset::Loaded(LoadedImage::new(
            ImageFormat::Png,
            b"projection only".to_vec(),
            [1, 1],
        ));
        let mut compiled = tray.clone();
        let embedded = compiled.image("icon").unwrap();
        compiled.children[0].children[0]
            .properties
            .insert("icon", Value::Image(embedded));
        tray.properties
            .insert("icon", Value::ImageAsset(loaded.clone()));
        tray.children[0].children[0]
            .properties
            .insert("icon", Value::ImageAsset(loaded));
        assert_eq!(
            TrayPresentation::project(&tray),
            TrayPresentation::project(&compiled)
        );
    }

    #[test]
    fn click_only_tray_requires_a_visible_icon_and_the_current_mount() {
        let mut tray = tray();
        let projection = TrayPresentation::project(&tray).unwrap();
        assert_eq!(
            projection.resolve_activation(&tray, TrayActivation::Clicked { mount: 4 }, 4, 1),
            Some(ResolvedActivation::Clicked)
        );
        assert_eq!(
            projection.resolve_activation(&tray, TrayActivation::Clicked { mount: 3 }, 4, 1),
            None
        );
        tray.properties.insert("visible", false.into());
        assert!(TrayPresentation::project(&tray).is_none());
        assert!(TrayPresentation::project(&Node::new("SystemTrayIcon", None)).is_none());
    }
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        self.suspend();
        self.tasks.close();
    }
}
