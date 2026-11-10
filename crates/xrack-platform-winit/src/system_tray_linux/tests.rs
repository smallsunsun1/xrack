use super::*;
use tokio::{sync::mpsc, time::timeout};
use xrack_runtime::ImageFormat;
use zbus::Connection;

const SVG: Image = Image::Embedded {
    format: ImageFormat::Svg,
    bytes: br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1">
        <rect width="2" height="1" fill="red" fill-opacity="0.5"/>
    </svg>"#,
    size: [2, 1],
};

fn command(label: &str, checked: bool, enabled: bool) -> TrayMenuItem {
    TrayMenuItem::Command {
        index: 0,
        label: label.into(),
        enabled,
        checked: Some(checked),
        icon: SVG,
    }
}

#[test]
fn sni_pixmap_encodes_straight_argb_in_network_order() {
    let (width, height, pixels) = pixmap(SVG, 4).unwrap();
    assert_eq!((width, height), (4, 4));
    assert_eq!(&pixels[..16], &[0; 16]);
    assert_eq!(&pixels[16..20], &[128, 255, 0, 0]);
}

#[test]
fn dbus_menu_revision_invalidates_same_shape_commands() {
    let mut next = 1;
    let initial = MenuLayout::new(&[command("Open_window", false, true)], 1, &mut next).unwrap();
    let replacement = MenuLayout::new(&[command("Quit", true, true)], 2, &mut next).unwrap();
    assert_eq!(initial.entries[&0].children, [1]);
    assert_eq!(replacement.entries[&0].children, [2]);
    assert!(replacement.entry(1).is_err());

    let mut properties = initial.entries[&1].properties(&["label".into(), "icon-data".into()]);
    assert_eq!(
        String::try_from(properties.remove("label").unwrap()).unwrap(),
        "Open__window"
    );
    let png = Vec::<u8>::try_from(properties.remove("icon-data").unwrap()).unwrap();
    let icon = xrack_assets::decode_png(&png).unwrap();
    assert_eq!(icon.size, [24, 24]);
    assert_eq!(&icon.rgba[24 * 4 * 6..][..4], &[255, 0, 0, 128]);
    assert!(properties.is_empty());

    let disabled = MenuLayout::new(&[command("Disabled", false, false)], 3, &mut next).unwrap();
    assert_eq!(disabled.entries[&3].command, None);
}

struct FakeWatcher(mpsc::UnboundedSender<String>);

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl FakeWatcher {
    fn register_status_notifier_item(&self, service: &str) {
        self.0.send(service.into()).unwrap();
    }
}

async fn fake_watcher() -> (Connection, mpsc::UnboundedReceiver<String>) {
    let (send, recv) = mpsc::unbounded_channel();
    let watcher = Builder::session()
        .unwrap()
        .serve_at("/StatusNotifierWatcher", FakeWatcher(send))
        .unwrap()
        .name(WATCHER)
        .unwrap()
        .build()
        .await
        .unwrap();
    (watcher, recv)
}

#[test]
#[ignore = "run in a private D-Bus session via dbus-run-session"]
fn status_notifier_round_trip() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            timeout(Duration::from_secs(12), panel_round_trip())
                .await
                .expect("D-Bus panel test timed out");
        });
}

async fn panel_round_trip() {
    const SERVICE: &str = "org.kde.StatusNotifierItem-xrack-test";
    let mut next = 1;
    let menu = MenuLayout::new(
        &[
            TrayMenuItem::Submenu {
                label: "Session".into(),
                enabled: true,
                items: vec![command("Open_window", false, true)],
            },
            TrayMenuItem::Separator,
            command("Disabled", false, false),
        ],
        1,
        &mut next,
    )
    .unwrap();
    let initial = Snapshot {
        title: "XRack".into(),
        tooltip: "Running".into(),
        pixmaps: Arc::new(vec![pixmap(SVG, 4).unwrap()]),
        menu: Arc::new(menu),
    };
    let (updates, receiver) = watch::channel(initial.clone());
    let (activated, mut activations) = mpsc::unbounded_channel();
    let client = Connection::session().await.unwrap();
    let dbus = fdo::DBusProxy::new(&client).await.unwrap();
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, SERVICE)])
        .await
        .unwrap();
    let running = tokio::spawn(serve(
        Builder::session().unwrap(),
        SERVICE,
        "xrack-wire-test".into(),
        receiver,
        move |action| activated.send(action).unwrap(),
        9,
    ));

    // Start the panel after the service, then restart it while the tray stays mounted.
    assert!(
        owners
            .next()
            .await
            .unwrap()
            .args()
            .unwrap()
            .new_owner()
            .is_some()
    );
    let (watcher, mut registered) = fake_watcher().await;
    assert_eq!(registered.recv().await.as_deref(), Some(SERVICE));
    let item = zbus::proxy::Builder::<Proxy<'_>>::new(&client)
        .destination(SERVICE)
        .unwrap()
        .path(ITEM_PATH)
        .unwrap()
        .interface("org.kde.StatusNotifierItem")
        .unwrap()
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .unwrap();
    assert_eq!(item.get_property::<String>("Title").await.unwrap(), "XRack");
    assert_eq!(
        item.get_property::<String>("Id").await.unwrap(),
        "xrack-wire-test"
    );
    assert_eq!(
        item.get_property::<Vec<Pixmap>>("IconPixmap")
            .await
            .unwrap()[0]
            .2[16..20],
        [128, 255, 0, 0]
    );
    let tooltip = item.get_property::<Tooltip>("ToolTip").await.unwrap();
    assert_eq!(
        (tooltip.2.as_str(), tooltip.3.as_str()),
        ("XRack", "Running")
    );

    let menu_path = item
        .get_property::<ObjectPath<'static>>("Menu")
        .await
        .unwrap();
    assert_eq!(menu_path.as_str(), MENU_PATH);
    let menu = Proxy::new(&client, SERVICE, menu_path, "com.canonical.dbusmenu")
        .await
        .unwrap();
    let mut layouts = menu.receive_signal("LayoutUpdated").await.unwrap();
    type WireLayout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);
    let (revision, root): (u32, WireLayout) = menu
        .call("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
        .await
        .unwrap();
    assert_eq!((revision, root.0, root.2.len()), (1, 0, 3));
    let mut children = root.2.into_iter();
    let submenu: WireLayout = Value::from(children.next().unwrap()).try_into().unwrap();
    let leaf: WireLayout = Value::from(submenu.2.into_iter().next().unwrap())
        .try_into()
        .unwrap();
    let disabled: WireLayout = Value::from(children.last().unwrap()).try_into().unwrap();
    assert_eq!(i32::try_from(&leaf.1["toggle-state"]).unwrap(), 0);
    assert!(!bool::try_from(&disabled.1["enabled"]).unwrap());

    item.call::<_, _, ()>("Activate", &(0i32, 0i32))
        .await
        .unwrap();
    assert_eq!(
        activations.recv().await,
        Some(TrayActivation::Clicked { mount: 9 })
    );
    let click = |id| (id, "clicked", Value::from(0i32), 0u32);
    menu.call::<_, _, ()>("Event", &click(disabled.0))
        .await
        .unwrap();
    menu.call::<_, _, ()>("Event", &click(leaf.0))
        .await
        .unwrap();
    assert_eq!(
        activations.recv().await,
        Some(TrayActivation::Menu {
            revision: 1,
            item: 0
        })
    );
    assert!(activations.try_recv().is_err());

    let mut tooltips = item.receive_signal("NewToolTip").await.unwrap();
    let next_menu = MenuLayout::new(&[command("Quit", true, true)], 2, &mut next).unwrap();
    let replacement_id = next_menu.entries[&0].children[0];
    updates.send_replace(Snapshot {
        tooltip: "Paused".into(),
        menu: Arc::new(next_menu),
        ..initial
    });
    assert_eq!(
        layouts
            .next()
            .await
            .unwrap()
            .body()
            .deserialize::<(u32, i32)>()
            .unwrap(),
        (2, 0)
    );
    tooltips.next().await.unwrap();
    assert_eq!(
        item.get_property::<Tooltip>("ToolTip").await.unwrap().3,
        "Paused"
    );
    assert!(
        menu.call::<_, _, ()>("Event", &click(leaf.0))
            .await
            .is_err()
    );
    assert!(activations.try_recv().is_err());
    menu.call::<_, _, ()>("Event", &click(replacement_id))
        .await
        .unwrap();
    assert_eq!(
        activations.recv().await,
        Some(TrayActivation::Menu {
            revision: 2,
            item: 0
        })
    );

    watcher.close().await.unwrap();
    let (watcher, mut registered) = fake_watcher().await;
    assert_eq!(registered.recv().await.as_deref(), Some(SERVICE));

    // Hiding a tray cancels its service task and releases its bus name.
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    let release = owners.next().await.unwrap();
    assert!(release.args().unwrap().new_owner().is_none());
    drop(updates);
    watcher.close().await.unwrap();
}
