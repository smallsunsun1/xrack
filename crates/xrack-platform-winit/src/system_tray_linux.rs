//! Export tray snapshots to Linux desktop panels over the session bus.
#[cfg(test)]
mod tests;

use crate::{
    LocalTask,
    desktop::{TrayEvent, TrayEvents},
    spawn_local,
    system_tray::{TrayActivation, TrayMenuItem, TrayPresentation},
};
use futures_lite::{StreamExt, future};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::watch;
use xrack_runtime::ImageAsset;
use zbus::{
    Proxy,
    connection::Builder,
    fdo,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, OwnedValue, Value},
};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

pub(crate) struct NativeTray {
    task: LocalTask<()>,
    updates: watch::Sender<Snapshot>,
    next_menu_id: i32,
}

impl NativeTray {
    pub fn new(events: TrayEvents, mount: u64) -> Result<Self, String> {
        static NEXT_SERVICE: AtomicU64 = AtomicU64::new(1);
        let instance = NEXT_SERVICE.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "org.kde.StatusNotifierItem-{}-{instance}",
            std::process::id()
        );
        let executable = std::env::current_exe().ok();
        let app = executable
            .as_deref()
            .and_then(std::path::Path::file_stem)
            .map_or_else(|| "xrack".into(), |name| name.to_string_lossy());
        let id = format!("{app}-{}", events.key());
        let (updates, receiver) = watch::channel(Snapshot::default());
        let task = spawn_local(async move {
            let result = async {
                let builder = Builder::session()?;
                let activations = events.clone();
                serve(
                    builder,
                    &name,
                    id,
                    receiver,
                    move |activation| {
                        activations.post(TrayEvent::Activated(activation));
                    },
                    mount,
                )
                .await
            };
            if let Err(error) = result.await {
                events.post(TrayEvent::Failed {
                    mount,
                    error: format!("Linux system tray: {error}"),
                });
            }
        })
        .map_err(|error| error.to_string())?;
        Ok(Self {
            task,
            updates,
            next_menu_id: 1,
        })
    }

    pub fn update(
        &mut self,
        current: &TrayPresentation,
        previous: Option<&TrayPresentation>,
        rebuild: Option<u64>,
    ) -> Result<(), String> {
        let mut snapshot = self.updates.borrow().clone();
        snapshot.title = dbus_text(&current.title);
        snapshot.tooltip = dbus_text(&current.tooltip)
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        if previous.is_none_or(|previous| previous.icon != current.icon) {
            snapshot.pixmaps = Arc::new(
                [16, 22, 32, 48, 64]
                    .into_iter()
                    .map(|size| pixmap(&current.icon, size))
                    .collect::<Result<_, _>>()?,
            );
        }
        if let Some(revision) = rebuild {
            snapshot.menu = Arc::new(MenuLayout::new(
                &current.menu.items,
                revision,
                &mut self.next_menu_id,
            )?);
        }
        self.updates.send_replace(snapshot);
        Ok(())
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn dbus_text(text: &str) -> String {
    text.replace('\0', "")
}

type Pixmap = (i32, i32, Vec<u8>);
type Tooltip = (String, Vec<Pixmap>, String, String);

fn pixmap(image: &ImageAsset, size: u32) -> Result<Pixmap, String> {
    let mut raster =
        xrack_assets::rasterize_icon_asset(image, [size; 2])?.ok_or("empty tray icon")?;
    for pixel in raster.rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = (u16::from(*channel) * 255 + alpha / 2)
                .checked_div(alpha)
                .unwrap_or_default()
                .min(255) as u8;
        }
        pixel.rotate_right(1);
    }
    Ok((size as i32, size as i32, raster.rgba))
}

#[derive(Clone, Default)]
struct Snapshot {
    title: String,
    tooltip: String,
    pixmaps: Arc<Vec<Pixmap>>,
    menu: Arc<MenuLayout>,
}

type Shared = Arc<Mutex<Snapshot>>;
type Activate = Arc<dyn Fn(TrayActivation) + Send + Sync>;

async fn serve(
    builder: Builder<'_>,
    name: &str,
    id: String,
    mut updates: watch::Receiver<Snapshot>,
    activate: impl Fn(TrayActivation) + Send + Sync + 'static,
    mount: u64,
) -> zbus::Result<()> {
    let shared = Arc::new(Mutex::new(updates.borrow_and_update().clone()));
    let activate: Activate = Arc::new(activate);
    let connection = builder
        .method_timeout(Duration::from_secs(5))
        .serve_at(
            ITEM_PATH,
            Item {
                id,
                shared: shared.clone(),
                activate: activate.clone(),
                mount,
            },
        )?
        .serve_at(
            MENU_PATH,
            Menu {
                shared: shared.clone(),
                activate,
            },
        )?
        .name(name)?
        .build()
        .await?;

    // Subscribe before registering so a watcher replacement cannot be missed.
    let dbus = fdo::DBusProxy::new(&connection).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, WATCHER)])
        .await?;
    let watcher = Proxy::new(&connection, WATCHER, "/StatusNotifierWatcher", WATCHER).await?;
    register(&watcher, name).await?;

    let item = connection
        .object_server()
        .interface::<_, Item>(ITEM_PATH)
        .await?;
    let menu = connection
        .object_server()
        .interface::<_, Menu>(MENU_PATH)
        .await?;
    loop {
        enum Event {
            Update(Result<(), watch::error::RecvError>),
            Owner(Option<fdo::NameOwnerChanged>),
        }
        match future::or(async { Event::Update(updates.changed().await) }, async {
            Event::Owner(owners.next().await)
        })
        .await
        {
            Event::Update(Ok(())) => {
                let next = updates.borrow_and_update().clone();
                let previous = std::mem::replace(&mut *shared.lock().unwrap(), next.clone());
                let icon_changed = !Arc::ptr_eq(&previous.pixmaps, &next.pixmaps);
                let title_changed = previous.title != next.title;
                if icon_changed {
                    Item::new_icon(item.signal_emitter()).await?;
                }
                if title_changed {
                    Item::new_title(item.signal_emitter()).await?;
                }
                if icon_changed || title_changed || previous.tooltip != next.tooltip {
                    Item::new_tool_tip(item.signal_emitter()).await?;
                }
                if !Arc::ptr_eq(&previous.menu, &next.menu) {
                    Menu::layout_updated(menu.signal_emitter(), next.menu.revision, 0).await?;
                }
            }
            Event::Update(Err(_)) => return connection.close().await,
            Event::Owner(Some(event)) if event.args()?.new_owner().is_some() => {
                register(&watcher, name).await?;
            }
            Event::Owner(Some(_)) => {}
            Event::Owner(None) => {
                return Err(zbus::Error::Failure("session bus disconnected".into()));
            }
        }
    }
}

async fn register(watcher: &Proxy<'_>, name: &str) -> zbus::Result<()> {
    match watcher.call("RegisterStatusNotifierItem", &(name,)).await {
        Err(zbus::Error::MethodError(error, _, _))
            if matches!(
                error.as_str(),
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
            ) =>
        {
            Ok(())
        }
        result => result,
    }
}

struct Item {
    id: String,
    shared: Shared,
    activate: Activate,
    mount: u64,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }
    #[zbus(property)]
    fn id(&self) -> &str {
        &self.id
    }
    #[zbus(property)]
    fn title(&self) -> String {
        self.shared.lock().unwrap().title.clone()
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }
    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        (*self.shared.lock().unwrap().pixmaps).clone()
    }
    #[zbus(property)]
    fn overlay_icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Vec<Pixmap> {
        Vec::new()
    }
    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Vec<Pixmap> {
        Vec::new()
    }
    #[zbus(property)]
    fn attention_movie_name(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn tool_tip(&self) -> Tooltip {
        let snapshot = self.shared.lock().unwrap();
        (
            String::new(),
            (*snapshot.pixmaps).clone(),
            snapshot.title.clone(),
            snapshot.tooltip.clone(),
        )
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn menu(&self) -> ObjectPath<'static> {
        ObjectPath::from_static_str_unchecked(MENU_PATH)
    }

    fn activate(&self, _x: i32, _y: i32) {
        (self.activate)(TrayActivation::Clicked { mount: self.mount });
    }

    // The panel renders the DBusMenu tree at the Menu property.
    fn context_menu(&self, _x: i32, _y: i32) {}
    fn secondary_activate(&self, _x: i32, _y: i32) {}
    fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(signal)]
    async fn new_title(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

type Properties = HashMap<String, Value<'static>>;
type Layout = (i32, Properties, Vec<Value<'static>>);

#[derive(Default)]
struct MenuLayout {
    revision: u32,
    activation_revision: u64,
    entries: BTreeMap<i32, MenuEntry>,
}

#[derive(Default)]
struct MenuEntry {
    properties: Properties,
    children: Vec<i32>,
    command: Option<usize>,
}

impl MenuLayout {
    fn new(items: &[TrayMenuItem], revision: u64, next_id: &mut i32) -> Result<Self, String> {
        let mut layout = Self {
            revision: u32::try_from(revision).map_err(|_| "DBusMenu revisions exhausted")?,
            activation_revision: revision,
            entries: BTreeMap::new(),
        };
        let children = layout.add_items(items, next_id)?;
        let mut root = MenuEntry {
            children,
            ..Default::default()
        };
        root.properties
            .insert("children-display".into(), "submenu".into());
        layout.entries.insert(0, root);
        Ok(layout)
    }

    fn add_items(&mut self, items: &[TrayMenuItem], next_id: &mut i32) -> Result<Vec<i32>, String> {
        items
            .iter()
            .map(|item| {
                // Allocate fresh wire IDs even when the menu shape is unchanged. A hidden
                // action replaced by another in the same slot must invalidate the old ID.
                let id = *next_id;
                *next_id = id.checked_add(1).ok_or("DBusMenu item IDs exhausted")?;
                let mut entry = MenuEntry::default();
                match item {
                    TrayMenuItem::Separator => {
                        entry.properties.insert("type".into(), "separator".into());
                    }
                    TrayMenuItem::Submenu {
                        label,
                        enabled,
                        items,
                    } => {
                        entry.set_label(label, *enabled);
                        entry
                            .properties
                            .insert("children-display".into(), "submenu".into());
                        entry.children = self.add_items(items, next_id)?;
                    }
                    TrayMenuItem::Command {
                        index,
                        label,
                        enabled,
                        checked,
                        icon,
                    } => {
                        entry.set_label(label, *enabled);
                        entry.command = enabled.then_some(*index);
                        if let Some(checked) = checked {
                            entry
                                .properties
                                .insert("toggle-type".into(), "checkmark".into());
                            entry
                                .properties
                                .insert("toggle-state".into(), i32::from(*checked).into());
                        }
                        let icon = xrack_assets::rasterize_icon_asset_png(icon, [24; 2])?;
                        if !icon.is_empty() {
                            entry.properties.insert("icon-data".into(), icon.into());
                        }
                    }
                }
                self.entries.insert(id, entry);
                Ok(id)
            })
            .collect()
    }

    fn entry(&self, id: i32) -> fdo::Result<&MenuEntry> {
        self.entries
            .get(&id)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown menu item {id}")))
    }

    fn layout(&self, id: i32, depth: i32, filter: &[String]) -> fdo::Result<Layout> {
        let entry = self.entry(id)?;
        let children = if depth == 0 {
            Vec::new()
        } else {
            let depth = if depth < 0 { -1 } else { depth - 1 };
            entry
                .children
                .iter()
                .map(|&id| self.layout(id, depth, filter).map(Value::from))
                .collect::<fdo::Result<_>>()?
        };
        Ok((id, entry.properties(filter), children))
    }
}

impl MenuEntry {
    fn set_label(&mut self, text: &str, enabled: bool) {
        self.properties
            .insert("label".into(), dbus_text(text).replace('_', "__").into());
        if !enabled {
            self.properties.insert("enabled".into(), false.into());
        }
    }

    fn properties(&self, filter: &[String]) -> Properties {
        self.properties
            .iter()
            .filter(|(key, _)| filter.is_empty() || filter.contains(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
}

struct Menu {
    shared: Shared,
    activate: Activate,
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        4
    }
    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }
    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(out_args("revision", "layout"))]
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> fdo::Result<(u32, Layout)> {
        let snapshot = self.shared.lock().unwrap();
        let menu = &snapshot.menu;
        Ok((
            menu.revision,
            menu.layout(parent_id, recursion_depth, &property_names)?,
        ))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, Properties)> {
        let snapshot = self.shared.lock().unwrap();
        let entries = &snapshot.menu.entries;
        if ids.is_empty() {
            entries
                .iter()
                .map(|(&id, entry)| (id, entry.properties(&property_names)))
                .collect()
        } else {
            ids.into_iter()
                .filter_map(|id| Some((id, entries.get(&id)?.properties(&property_names))))
                .collect()
        }
    }

    fn get_property(&self, id: i32, name: &str) -> fdo::Result<Value<'static>> {
        let snapshot = self.shared.lock().unwrap();
        let entry = snapshot.menu.entry(id)?;
        if let Some(value) = entry.properties.get(name) {
            return Ok(value.clone());
        }
        Ok(match name {
            "enabled" | "visible" => true.into(),
            "type" => "standard".into(),
            "toggle-state" => (-1i32).into(),
            "icon-data" => Vec::<u8>::new().into(),
            "shortcut" => Vec::<Vec<String>>::new().into(),
            "disposition" => "normal".into(),
            _ => "".into(),
        })
    }

    fn event(
        &self,
        id: i32,
        event_id: &str,
        _data: OwnedValue,
        _timestamp: u32,
    ) -> fdo::Result<()> {
        let command = {
            let snapshot = self.shared.lock().unwrap();
            let menu = &snapshot.menu;
            menu.entry(id)?.command.map(|item| TrayActivation::Menu {
                revision: menu.activation_revision,
                item,
            })
        };
        if event_id == "clicked"
            && let Some(command) = command
        {
            (self.activate)(command);
        }
        Ok(())
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        events
            .into_iter()
            .filter_map(|(id, event, data, timestamp)| {
                self.event(id, &event, data, timestamp).err().map(|_| id)
            })
            .collect()
    }

    fn about_to_show(&self, id: i32) -> fdo::Result<bool> {
        self.shared.lock().unwrap().menu.entry(id)?;
        Ok(false)
    }

    #[zbus(out_args("updates_needed", "id_errors"))]
    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        let snapshot = self.shared.lock().unwrap();
        (
            Vec::new(),
            ids.into_iter()
                .filter(|id| !snapshot.menu.entries.contains_key(id))
                .collect(),
        )
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}
