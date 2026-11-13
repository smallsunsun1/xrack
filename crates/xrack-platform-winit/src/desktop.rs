use crate::system_tray::{self, TrayActivation, TrayHost};
use crate::{Application, ClipboardPort, LocalExecutor, UserEvent, queue::EventQueue};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    rc::{Rc, Weak},
    sync::{Arc, mpsc::TrySendError},
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};
use xrack_renderer_femtovg::{RenderDevice, Renderer};
use xrack_runtime::{
    Component, EventLoopError, EventLoopRegistration, LocaleContext, LocaleId,
    SecondaryWindowRegistration,
};

type WindowKey = u64;
type UiCallback = Box<dyn FnOnce() + Send>;
const CALLBACK_CAPACITY: usize = 256;

pub(crate) enum DesktopEvent {
    Commands,
    InvokeCallbacks,
    PollTasks,
    Window(WindowKey, UserEvent),
    Tray(WindowKey, TrayEvent),
}

pub(crate) enum TrayEvent {
    #[cfg_attr(
        not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
        allow(dead_code)
    )]
    Activated(TrayActivation),
    PollTasks,
    #[cfg(target_os = "linux")]
    Failed {
        mount: u64,
        error: String,
    },
    #[cfg(target_os = "windows")]
    ContextMenu {
        mount: u64,
        anchor: (i32, i32),
    },
    #[cfg(target_os = "windows")]
    Restore {
        mount: u64,
        taskbar_created: bool,
    },
}

#[derive(Clone)]
pub(crate) struct TrayEvents {
    key: WindowKey,
    proxy: EventLoopProxy<DesktopEvent>,
}

impl TrayEvents {
    #[cfg(target_os = "linux")]
    pub fn key(&self) -> WindowKey {
        self.key
    }

    pub fn post(&self, event: TrayEvent) {
        let _ = self.proxy.send_event(DesktopEvent::Tray(self.key, event));
    }
}

#[derive(Clone)]
pub(crate) struct WindowEvents {
    key: WindowKey,
    proxy: EventLoopProxy<DesktopEvent>,
}

impl WindowEvents {
    pub(crate) fn post(&self, event: UserEvent) -> bool {
        self.proxy
            .send_event(DesktopEvent::Window(self.key, event))
            .is_ok()
    }

    pub(crate) fn accessibility(
        &self,
        event_loop: &ActiveEventLoop,
        window: &winit::window::Window,
    ) -> accesskit_winit::Adapter {
        let handler = AccessibilityEvents {
            events: self.clone(),
            window_id: window.id(),
        };
        accesskit_winit::Adapter::with_direct_handlers(
            event_loop,
            window,
            handler.clone(),
            handler.clone(),
            handler,
        )
    }
}

#[derive(Clone)]
struct AccessibilityEvents {
    events: WindowEvents,
    window_id: WindowId,
}

impl AccessibilityEvents {
    fn post(&self, window_event: accesskit_winit::WindowEvent) {
        self.events
            .post(UserEvent::Accessibility(accesskit_winit::Event {
                window_id: self.window_id,
                window_event,
            }));
    }
}

impl accesskit::ActivationHandler for AccessibilityEvents {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        self.post(accesskit_winit::WindowEvent::InitialTreeRequested);
        None
    }
}

impl accesskit::ActionHandler for AccessibilityEvents {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        self.post(accesskit_winit::WindowEvent::ActionRequested(request));
    }
}

impl accesskit::DeactivationHandler for AccessibilityEvents {
    fn deactivate_accessibility(&mut self) {
        self.post(accesskit_winit::WindowEvent::AccessibilityDeactivated);
    }
}

pub(crate) struct DesktopGraphics {
    pub(crate) instance: wgpu::Instance,
    devices: Vec<RenderDevice>,
}

impl DesktopGraphics {
    fn new(event_loop: &ActiveEventLoop) -> Self {
        Self {
            instance: wgpu::Instance::new(
                wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
                    event_loop.owned_display_handle(),
                )),
            ),
            devices: Vec::new(),
        }
    }

    pub(crate) fn renderer(&mut self, surface: &wgpu::Surface<'_>) -> Result<Renderer, String> {
        if let Some(device) = self
            .devices
            .iter()
            .find(|device| device.supports_surface(surface))
        {
            return Renderer::with_device(device.clone());
        }
        let device =
            pollster::block_on(RenderDevice::request(self.instance.clone(), Some(surface)))?;
        let renderer = Renderer::with_device(device.clone())?;
        self.devices.push(device);
        Ok(renderer)
    }
}

pub(crate) trait HostedWindow {
    fn resume(
        &mut self,
        event_loop: &ActiveEventLoop,
        graphics: &mut DesktopGraphics,
        owner: Option<&Window>,
    ) -> Result<WindowId, String>;
    fn suspend(&mut self);
    fn window_id(&self) -> Option<WindowId>;
    fn native_window(&self) -> Option<Arc<Window>>;
    fn secondary_windows(&self) -> Option<Rc<[SecondaryWindowRegistration]>>;
    fn close(&mut self);
    fn request_close(&mut self) -> bool;
    fn take_close_request(&mut self) -> bool;
    fn tick(&mut self, now: Instant);
    fn refresh_deadline(&mut self, now: Instant) -> Option<Instant>;
    fn refresh_after_desktop_update(&mut self);
    fn user_event(&mut self, event: UserEvent);
    fn window_event(&mut self, id: WindowId, event: WindowEvent);
    fn take_error(&mut self) -> Option<String>;
}

type SharedClipboard = Rc<RefCell<ClipboardPort>>;
type WindowFactory = Box<dyn FnOnce(WindowEvents, SharedClipboard) -> Box<dyn HostedWindow>>;

enum Command {
    Open(WindowKey, WindowFactory),
    Close(WindowKey),
    OpenTray(WindowKey, TrayHost),
    CloseTray(WindowKey),
    SetLocale(LocaleId),
}

struct Commands {
    locale: LocaleContext,
    next: Cell<WindowKey>,
    closed: Cell<bool>,
    checking_close: Cell<Option<WindowKey>>,
    pending: RefCell<Vec<Command>>,
    proxy: EventLoopProxy<DesktopEvent>,
}

impl Commands {
    fn key(&self) -> WindowKey {
        let key = self.next.get();
        self.next.set(
            key.checked_add(1)
                .expect("desktop window identities exhausted"),
        );
        key
    }

    fn enqueue(&self, command: Command) -> Result<(), String> {
        if self.closed.get() {
            return Err("the XRack desktop is closed".into());
        }
        let mut pending = self.pending.borrow_mut();
        if let Command::Close(key) = &command
            && (self.checking_close.get() == Some(*key)
                || pending
                    .iter()
                    .any(|queued| matches!(queued, Command::Close(other) if other == key)))
        {
            return Ok(());
        }
        if pending.is_empty() {
            self.proxy
                .send_event(DesktopEvent::Commands)
                .map_err(|_| "the XRack desktop is closed")?;
        }
        pending.push(command);
        Ok(())
    }

    fn events(&self, key: WindowKey) -> WindowEvents {
        WindowEvents {
            key,
            proxy: self.proxy.clone(),
        }
    }
}

/// A UI-thread handle for opening additional windows in the same event loop.
#[derive(Clone)]
pub struct Desktop(Weak<Commands>);

impl Desktop {
    /// Select the UI language for windows and tray icons on this desktop.
    pub fn set_locale(&self, locale: &str) -> Result<(), String> {
        let locale = locale
            .parse::<LocaleId>()
            .map_err(|error| format!("invalid UI locale `{locale}`: {error}"))?;
        self.0
            .upgrade()
            .ok_or("the XRack desktop is closed")?
            .enqueue(Command::SetLocale(locale))
    }

    pub fn open<C: Component + 'static>(&self, component: C) -> Result<DesktopWindow, String> {
        let commands = self.0.upgrade().ok_or("the XRack desktop is closed")?;
        component.set_locale_context(commands.locale.clone());
        let key = commands.key();
        commands.enqueue(Command::Open(
            key,
            Box::new(move |events, clipboard| Box::new(make_window(component, events, clipboard))),
        ))?;
        Ok(DesktopWindow {
            desktop: self.clone(),
            key,
        })
    }

    /// Register a windowless SystemTrayIcon component with the desktop.
    pub fn register_tray<C: Component + 'static>(
        &self,
        component: C,
    ) -> Result<DesktopTray, String> {
        if !cfg!(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows"
        )) {
            return Err(system_tray::unsupported());
        }
        let commands = self.0.upgrade().ok_or("the XRack desktop is closed")?;
        component.set_locale_context(commands.locale.clone());
        let key = commands.key();
        let events = TrayEvents {
            key,
            proxy: commands.proxy.clone(),
        };
        commands.enqueue(Command::OpenTray(
            key,
            TrayHost::new(Box::new(component), events)?,
        ))?;
        Ok(DesktopTray {
            desktop: self.clone(),
            key,
        })
    }
}

/// A UI-thread handle for removing a system tray component from the desktop.
#[derive(Clone)]
pub struct DesktopTray {
    desktop: Desktop,
    key: WindowKey,
}

impl DesktopTray {
    pub fn close(&self) -> Result<(), String> {
        self.desktop
            .0
            .upgrade()
            .ok_or("the XRack desktop is closed")?
            .enqueue(Command::CloseTray(self.key))
    }
}

/// A UI-thread handle for requesting closure of one desktop window.
#[derive(Clone)]
pub struct DesktopWindow {
    desktop: Desktop,
    key: WindowKey,
}

impl DesktopWindow {
    /// Queue a close request for the window's close handler.
    pub fn close(&self) -> Result<(), String> {
        self.desktop
            .0
            .upgrade()
            .ok_or("the XRack desktop is closed")?
            .enqueue(Command::Close(self.key))
    }
}

fn make_window<C: Component>(
    component: C,
    events: WindowEvents,
    clipboard: SharedClipboard,
) -> Application<C> {
    let task_events = events.clone();
    let tasks = LocalExecutor::new(move || {
        task_events.post(UserEvent::PollTasks);
    });
    let _scope = tasks.enter();
    let mut app = Application::new(component, tasks.clone());
    app.events = Some(events);
    app.clipboard = Some(clipboard);
    app
}

struct DesktopRuntime {
    callbacks: Arc<EventQueue<UiCallback>>,
    tasks: Rc<LocalExecutor>,
    commands: Rc<Commands>,
    clipboard: SharedClipboard,
    windows: BTreeMap<WindowKey, Box<dyn HostedWindow>>,
    trays: BTreeMap<WindowKey, TrayHost>,
    owned: BTreeMap<WindowKey, OwnedWindow>,
    native: HashMap<WindowId, WindowKey>,
    graphics: Option<DesktopGraphics>,
    resumed: bool,
    error: Option<String>,
}

struct OwnedWindow {
    owner: WindowKey,
    registration: SecondaryWindowRegistration,
}

impl DesktopRuntime {
    fn new(proxy: EventLoopProxy<DesktopEvent>) -> Self {
        let callback_events = proxy.clone();
        let callbacks = Arc::new(EventQueue::new(CALLBACK_CAPACITY, move || {
            let _ = callback_events.send_event(DesktopEvent::InvokeCallbacks);
        }));
        let task_events = proxy.clone();
        let tasks = LocalExecutor::new(move || {
            let _ = task_events.send_event(DesktopEvent::PollTasks);
        });
        Self {
            callbacks,
            tasks,
            commands: Rc::new(Commands {
                locale: LocaleContext::default(),
                next: Cell::new(0),
                closed: Cell::new(false),
                checking_close: Cell::new(None),
                pending: RefCell::new(Vec::new()),
                proxy,
            }),
            clipboard: Rc::new(RefCell::new(ClipboardPort::new())),
            windows: BTreeMap::new(),
            trays: BTreeMap::new(),
            owned: BTreeMap::new(),
            native: HashMap::new(),
            graphics: None,
            resumed: false,
            error: None,
        }
    }

    fn register_event_loop(&self) -> Result<EventLoopRegistration, String> {
        let callbacks = self.callbacks.clone();
        EventLoopRegistration::new(move |callback| {
            callbacks.try_push(callback).map_err(|error| match error {
                TrySendError::Full(_) => EventLoopError::QueueFull,
                TrySendError::Disconnected(_) => EventLoopError::NotRunning,
            })
        })
        .map_err(|error| error.to_string())
    }

    fn invoke_callbacks(&mut self) {
        let _scope = self.tasks.enter();
        let callbacks = self.callbacks.take_batch();
        if callbacks.is_empty() {
            return;
        }
        for callback in callbacks {
            callback();
        }
        self.refresh_windows();
    }

    fn refresh_windows(&mut self) {
        for window in self.windows.values_mut() {
            window.refresh_after_desktop_update();
        }
    }

    fn retire(&mut self, key: WindowKey) {
        let children: Vec<_> = self
            .owned
            .iter()
            .filter_map(|(&child, owned)| (owned.owner == key).then_some(child))
            .collect();
        for child in children {
            self.retire(child);
        }
        self.owned.remove(&key);
        if let Some(mut window) = self.windows.remove(&key) {
            if let Some(id) = window.window_id() {
                self.native.remove(&id);
            }
            window.close();
        }
    }

    fn finish_close(&mut self, key: WindowKey) {
        let registration = self.owned.get(&key).map(|owned| owned.registration.clone());
        self.retire(key);
        if let Some(registration) = registration {
            registration.closed();
        }
    }

    fn request_close(&mut self, key: WindowKey) {
        let Some(window) = self.windows.get_mut(&key) else {
            return;
        };
        self.commands.checking_close.set(Some(key));
        let accepted = window.request_close();
        self.commands.checking_close.set(None);
        if accepted {
            self.finish_close(key);
        }
    }

    fn consume_close_requests(&mut self) -> bool {
        let pending: Vec<_> = self
            .windows
            .iter_mut()
            .filter_map(|(&key, window)| window.take_close_request().then_some(key))
            .collect();
        let consumed = !pending.is_empty();
        for key in pending {
            self.request_close(key);
        }
        consumed
    }

    fn exit_if_empty(&self, event_loop: &ActiveEventLoop) {
        if self.windows.is_empty()
            && self.trays.is_empty()
            && self.commands.pending.borrow().is_empty()
        {
            event_loop.exit();
        }
    }

    fn apply_commands(&mut self, event_loop: &ActiveEventLoop) {
        let pending = self.commands.pending.take();
        for command in pending {
            match command {
                Command::Open(key, make) => {
                    let window = make(self.commands.events(key), self.clipboard.clone());
                    self.windows.insert(key, window);
                }
                Command::Close(key) => self.request_close(key),
                Command::OpenTray(key, tray) => {
                    self.trays.insert(key, tray);
                }
                Command::CloseTray(key) => {
                    self.trays.remove(&key);
                }
                Command::SetLocale(locale) => self.commands.locale.set_language(locale),
            }
        }
        self.consume_close_requests();
        if self.resumed {
            let keys: Vec<_> = self.windows.keys().copied().collect();
            for key in keys {
                self.mount_window(event_loop, key);
            }
            self.sync_secondary_windows(event_loop);
            self.sync_trays();
        }
        self.exit_if_empty(event_loop);
    }

    fn sync_tray(&mut self, key: WindowKey) -> bool {
        let Some(tray) = self.trays.get_mut(&key) else {
            return false;
        };
        if let Err(error) = tray.sync() {
            self.error.get_or_insert(error);
            self.trays.remove(&key);
            false
        } else {
            true
        }
    }

    fn sync_trays(&mut self) {
        if self.resumed {
            let keys: Vec<_> = self.trays.keys().copied().collect();
            for key in keys {
                self.sync_tray(key);
            }
        }
    }

    fn mount_window(&mut self, event_loop: &ActiveEventLoop, key: WindowKey) {
        let owner = self
            .owned
            .get(&key)
            .and_then(|owned| self.windows.get(&owned.owner))
            .and_then(|window| window.native_window());
        let Some(window) = self
            .windows
            .get_mut(&key)
            .filter(|window| window.window_id().is_none())
        else {
            return;
        };
        let graphics = self
            .graphics
            .get_or_insert_with(|| DesktopGraphics::new(event_loop));
        match window.resume(event_loop, graphics, owner.as_deref()) {
            Ok(id) => {
                self.native.insert(id, key);
            }
            Err(error) => {
                self.error.get_or_insert(error);
                self.finish_close(key);
            }
        }
    }

    fn sync_secondary_windows(&mut self, event_loop: &ActiveEventLoop) {
        if !self.resumed {
            return;
        }
        let mut requested = BTreeMap::new();
        for (&owner, window) in &self.windows {
            if let Some(windows) = window.secondary_windows() {
                for registration in windows.iter() {
                    requested.insert((owner, registration.id()), registration.clone());
                }
            }
        }
        let hidden: Vec<_> = self
            .owned
            .iter()
            .filter_map(|(&key, owned)| {
                (!requested.contains_key(&(owned.owner, owned.registration.id()))).then_some(key)
            })
            .collect();
        for key in hidden {
            self.retire(key);
        }
        for owned in self.owned.values() {
            requested.remove(&(owned.owner, owned.registration.id()));
        }
        for ((owner, _), registration) in requested {
            if !self.windows.contains_key(&owner) {
                continue;
            }
            let Some(component) = registration.mount() else {
                continue;
            };
            let key = self.commands.key();
            let mut window =
                make_window(component, self.commands.events(key), self.clipboard.clone());
            let closing = window.take_close_request();
            self.windows.insert(key, Box::new(window));
            self.owned.insert(
                key,
                OwnedWindow {
                    owner,
                    registration,
                },
            );
            if closing {
                self.request_close(key);
            }
            self.mount_window(event_loop, key);
        }
    }
}

impl ApplicationHandler<DesktopEvent> for DesktopRuntime {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed = true;
        self.apply_commands(event_loop);
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.resumed = false;
        self.native.clear();
        for tray in self.trays.values_mut() {
            tray.suspend();
        }
        for window in self.windows.values_mut().rev() {
            window.suspend();
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.callbacks.close();
        self.tasks.close();
        self.commands.closed.set(true);
        self.commands.pending.take();
        self.trays.clear();
        for window in self.windows.values_mut().rev() {
            window.close();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DesktopEvent) {
        let (key, event) = match event {
            DesktopEvent::Commands => {
                self.apply_commands(event_loop);
                return;
            }
            DesktopEvent::InvokeCallbacks => {
                self.invoke_callbacks();
                return;
            }
            DesktopEvent::PollTasks => {
                let _scope = self.tasks.enter();
                if self.tasks.poll_ready() {
                    self.refresh_windows();
                }
                return;
            }
            DesktopEvent::Window(key, event) => (key, event),
            DesktopEvent::Tray(key, event) => {
                match event {
                    TrayEvent::Activated(event) if self.resumed && self.sync_tray(key) => {
                        self.trays.get_mut(&key).unwrap().activate(event);
                        self.sync_tray(key);
                    }
                    TrayEvent::PollTasks => {
                        if let Some(tray) = self.trays.get(&key) {
                            tray.poll_tasks();
                        }
                        if self.resumed {
                            self.sync_tray(key);
                        }
                    }
                    #[cfg(target_os = "linux")]
                    TrayEvent::Failed { mount, error }
                        if self
                            .trays
                            .get(&key)
                            .is_some_and(|tray| tray.has_mount(mount)) =>
                    {
                        self.error.get_or_insert(error);
                        self.trays.remove(&key);
                        self.exit_if_empty(event_loop);
                    }
                    #[cfg(target_os = "windows")]
                    TrayEvent::ContextMenu { mount, anchor }
                        if self.resumed && self.sync_tray(key) =>
                    {
                        self.trays.get(&key).unwrap().request_menu(mount, anchor);
                    }
                    #[cfg(target_os = "windows")]
                    TrayEvent::Restore {
                        mount,
                        taskbar_created,
                    } if self.resumed && self.sync_tray(key) => {
                        if let Err(error) = self
                            .trays
                            .get_mut(&key)
                            .unwrap()
                            .restore(mount, taskbar_created)
                        {
                            self.error.get_or_insert(error);
                            self.trays.remove(&key);
                        }
                    }
                    _ => {}
                }
                return;
            }
        };
        if let Some(window) = self.windows.get_mut(&key) {
            window.user_event(event);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(&key) = self.native.get(&id) else {
            return;
        };
        if matches!(event, WindowEvent::CloseRequested) {
            self.request_close(key);
        } else if matches!(event, WindowEvent::Destroyed) {
            self.finish_close(key);
        } else if let Some(window) = self.windows.get_mut(&key) {
            window.window_event(id, event);
            if let Some(error) = window.take_error() {
                self.error.get_or_insert(error);
                self.finish_close(key);
            }
        }
        self.sync_trays();
        self.exit_if_empty(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.consume_close_requests();
        let now = Instant::now();
        for window in self.windows.values_mut() {
            window.tick(now);
        }
        let closes = self.consume_close_requests();
        let deadline = self
            .windows
            .values_mut()
            .filter_map(|window| window.refresh_deadline(now))
            .min();
        self.sync_secondary_windows(event_loop);
        self.sync_trays();
        event_loop.set_control_flow(if closes {
            // A close handler can request closure of another window.
            ControlFlow::Poll
        } else {
            deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
        });
        self.exit_if_empty(event_loop);
    }
}

fn event_loop() -> Result<EventLoop<DesktopEvent>, String> {
    EventLoop::with_user_event()
        .build()
        .map_err(|error| error.to_string())
}

/// Run windows and system tray components on one UI thread.
pub fn run_desktop(setup: impl FnOnce(&Desktop) -> Result<(), String>) -> Result<(), String> {
    let event_loop = event_loop()?;
    let mut desktop = DesktopRuntime::new(event_loop.create_proxy());
    let _registration = desktop.register_event_loop()?;
    {
        let _scope = desktop.tasks.enter();
        setup(&Desktop(Rc::downgrade(&desktop.commands)))?;
    }
    event_loop
        .run_app(&mut desktop)
        .map_err(|error| error.to_string())?;
    desktop.error.take().map_or(Ok(()), Err)
}

pub(crate) fn run_window<C: Component + 'static>(component: C) -> Result<(), String> {
    let event_loop = event_loop()?;
    let mut desktop = DesktopRuntime::new(event_loop.create_proxy());
    let _registration = desktop.register_event_loop()?;
    let key = desktop.commands.key();
    let window = make_window(
        component,
        desktop.commands.events(key),
        desktop.clipboard.clone(),
    );
    desktop.windows.insert(key, Box::new(window));
    event_loop
        .run_app(&mut desktop)
        .map_err(|error| error.to_string())?;
    desktop.error.take().map_or(Ok(()), Err)
}

impl Drop for DesktopRuntime {
    fn drop(&mut self) {
        self.callbacks.close();
        self.tasks.close();
    }
}
