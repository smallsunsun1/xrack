use crate::keyboard::KeyEvent;
use android_activity::{
    AndroidApp, AndroidAppWaker, InputStatus, MainEvent, PollEvent, Rect,
    input::{InputEvent, MotionAction},
    ndk::native_window::NativeWindow,
};
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{TouchPhase, WindowEvent},
    event_loop::ControlFlow,
    keyboard::PhysicalKey,
    raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
    },
    window::WindowId,
};

const MAX_DISPATCH_BATCH: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeviceId(i32);

#[cfg(test)]
impl DeviceId {
    pub(crate) const fn dummy() -> Self {
        Self(0)
    }
}

pub(crate) struct Touch {
    pub(crate) device_id: DeviceId,
    pub(crate) id: u64,
    pub(crate) phase: TouchPhase,
    pub(crate) location: PhysicalPosition<f64>,
}

pub(crate) enum AndroidEvent {
    Touch(Touch),
    Key(PhysicalKey, KeyEvent),
    ConfigurationChanged,
    ContentChanged,
}

pub(crate) trait ApplicationHandler<T> {
    fn new_events(&mut self);
    fn resumed(&mut self, event_loop: &ActiveEventLoop);
    fn suspended(&mut self, event_loop: &ActiveEventLoop);
    fn exiting(&mut self, event_loop: &ActiveEventLoop);
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: T);
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent);
    fn android_event(&mut self, event_loop: &ActiveEventLoop, event: AndroidEvent);
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop);
}

struct Signals {
    waker: AndroidAppWaker,
    redraw: AtomicBool,
    focused: AtomicBool,
}

pub(crate) struct Window {
    app: AndroidApp,
    native: NativeWindow,
    signals: Arc<Signals>,
}

impl Window {
    pub(crate) fn id(&self) -> WindowId {
        WindowId::dummy()
    }

    pub(crate) fn inner_size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(
            self.native.width().max(0) as u32,
            self.native.height().max(0) as u32,
        )
    }

    pub(crate) fn scale_factor(&self) -> f64 {
        f64::from(
            self.app
                .config()
                .density()
                .filter(|density| *density > 0)
                .unwrap_or(160),
        ) / 160.0
    }

    pub(crate) fn content_rect(&self) -> Rect {
        self.app.content_rect()
    }

    pub(crate) fn has_focus(&self) -> bool {
        self.signals.focused.load(Ordering::Relaxed)
    }

    pub(crate) fn request_redraw(&self) {
        if !self.signals.redraw.swap(true, Ordering::AcqRel) {
            self.signals.waker.wake();
        }
    }

    pub(crate) fn set_ime_allowed(&self, allowed: bool) {
        if allowed {
            self.app.show_soft_input(true);
        } else {
            self.app.hide_soft_input(true);
        }
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.native.window_handle()
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}

pub(crate) struct ActiveEventLoop {
    app: AndroidApp,
    signals: Arc<Signals>,
    control_flow: Cell<ControlFlow>,
    exiting: Cell<bool>,
    window: RefCell<Weak<Window>>,
}

impl ActiveEventLoop {
    pub(crate) fn android_app(&self) -> &AndroidApp {
        &self.app
    }

    pub(crate) fn create_window(&self) -> Result<Arc<Window>, String> {
        if self.window.borrow().strong_count() > 0 {
            return Err("Android supports one XRack window per Activity".into());
        }
        let native = self
            .app
            .native_window()
            .ok_or("Android has not created a surface")?;
        let window = Arc::new(Window {
            app: self.app.clone(),
            native,
            signals: self.signals.clone(),
        });
        self.window.replace(Arc::downgrade(&window));
        Ok(window)
    }

    pub(crate) fn set_control_flow(&self, flow: ControlFlow) {
        self.control_flow.set(flow);
    }

    pub(crate) fn exit(&self) {
        self.exiting.set(true);
    }

    fn window_event<T>(&self, handler: &mut impl ApplicationHandler<T>, event: WindowEvent) {
        if self.window.borrow().strong_count() > 0 {
            handler.window_event(self, WindowId::dummy(), event);
        }
    }
}

pub(crate) struct EventLoopProxy<T> {
    sender: Sender<T>,
    signals: Arc<Signals>,
}

impl<T> Clone for EventLoopProxy<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            signals: self.signals.clone(),
        }
    }
}

impl<T> EventLoopProxy<T> {
    pub(crate) fn send_event(&self, event: T) -> Result<(), mpsc::SendError<T>> {
        self.sender.send(event)?;
        self.signals.waker.wake();
        Ok(())
    }
}

pub(crate) struct EventLoop<T> {
    active: ActiveEventLoop,
    proxy: EventLoopProxy<T>,
    receiver: Receiver<T>,
}

impl<T> EventLoop<T> {
    pub(crate) fn new(app: AndroidApp) -> Self {
        let (sender, receiver) = mpsc::channel();
        let signals = Arc::new(Signals {
            waker: app.create_waker(),
            redraw: AtomicBool::new(false),
            focused: AtomicBool::new(false),
        });
        Self {
            active: ActiveEventLoop {
                app,
                signals: signals.clone(),
                control_flow: Cell::new(ControlFlow::Wait),
                exiting: Cell::new(false),
                window: RefCell::new(Weak::new()),
            },
            proxy: EventLoopProxy { sender, signals },
            receiver,
        }
    }

    pub(crate) fn create_proxy(&self) -> EventLoopProxy<T> {
        self.proxy.clone()
    }

    pub(crate) fn android_app(&self) -> &AndroidApp {
        self.active.android_app()
    }

    pub(crate) fn run_app(self, handler: &mut impl ApplicationHandler<T>) -> Result<(), String> {
        let active = &self.active;
        let mut accent = None;
        let mut foreground = false;
        while !active.exiting.get() {
            let timeout = match active.control_flow.get() {
                ControlFlow::Poll => Some(Duration::ZERO),
                ControlFlow::Wait => None,
                ControlFlow::WaitUntil(deadline) => {
                    Some(deadline.saturating_duration_since(Instant::now()))
                }
            };
            active.app.poll_events(timeout, |event| {
                handler.new_events();
                match event {
                    PollEvent::Main(MainEvent::InitWindow { .. }) => handler.resumed(active),
                    PollEvent::Main(MainEvent::Resume { .. }) => {
                        foreground = true;
                        active.signals.redraw.store(true, Ordering::Release);
                    }
                    PollEvent::Main(MainEvent::Pause) => foreground = false,
                    PollEvent::Main(MainEvent::TerminateWindow { .. }) => {
                        // Drop the GPU surface before the native-window callback returns.
                        handler.suspended(active);
                    }
                    PollEvent::Main(MainEvent::Destroy) => active.exit(),
                    PollEvent::Main(MainEvent::GainedFocus | MainEvent::LostFocus) => {
                        let focused = matches!(event, PollEvent::Main(MainEvent::GainedFocus));
                        active.signals.focused.store(focused, Ordering::Relaxed);
                        if !focused {
                            accent = None;
                        }
                        active.window_event(handler, WindowEvent::Focused(focused));
                    }
                    PollEvent::Main(MainEvent::WindowResized { .. }) => {
                        if let Some(window) = active.window.borrow().upgrade() {
                            handler.window_event(
                                active,
                                window.id(),
                                WindowEvent::Resized(window.inner_size()),
                            );
                        }
                    }
                    PollEvent::Main(MainEvent::RedrawNeeded { .. }) => {
                        active.signals.redraw.store(true, Ordering::Release);
                    }
                    PollEvent::Main(MainEvent::ConfigChanged { .. }) => {
                        handler.android_event(active, AndroidEvent::ConfigurationChanged);
                    }
                    PollEvent::Main(
                        MainEvent::ContentRectChanged { .. } | MainEvent::InsetsChanged { .. },
                    ) => {
                        handler.android_event(active, AndroidEvent::ContentChanged);
                    }
                    _ => {}
                }
                if active.exiting.get() {
                    return;
                }
                // The timer or proxy can wake the loop before InputAvailable.
                // Deliver queued input first so a ready hold timer cannot outrun it.
                if let Ok(mut inputs) = active.app.input_events_iter() {
                    while inputs.next(|input| dispatch_input(active, handler, input, &mut accent)) {
                    }
                }
                let mut delivered = 0;
                for event in self.receiver.try_iter().take(MAX_DISPATCH_BATCH) {
                    handler.user_event(active, event);
                    if active.exiting.get() {
                        return;
                    }
                    delivered += 1;
                }
                if foreground && active.signals.redraw.swap(false, Ordering::AcqRel) {
                    active.window_event(handler, WindowEvent::RedrawRequested);
                }
                handler.about_to_wait(active);
                if delivered == MAX_DISPATCH_BATCH {
                    // Android can coalesce wakes, so explicitly leave work for the next poll.
                    active.set_control_flow(ControlFlow::Poll);
                }
            });
        }
        handler.exiting(active);
        Ok(())
    }
}

fn dispatch_input<T>(
    active: &ActiveEventLoop,
    handler: &mut impl ApplicationHandler<T>,
    input: &InputEvent<'_>,
    accent: &mut Option<char>,
) -> InputStatus {
    match input {
        InputEvent::MotionEvent(motion) => {
            let phase = match motion.action() {
                MotionAction::Down | MotionAction::PointerDown => TouchPhase::Started,
                MotionAction::Up | MotionAction::PointerUp => TouchPhase::Ended,
                MotionAction::Move => TouchPhase::Moved,
                MotionAction::Cancel => TouchPhase::Cancelled,
                _ => return InputStatus::Unhandled,
            };
            let mut send = |pointer: android_activity::input::Pointer<'_>| {
                handler.android_event(
                    active,
                    AndroidEvent::Touch(Touch {
                        device_id: DeviceId(motion.device_id()),
                        id: pointer.pointer_id() as u64,
                        phase,
                        location: PhysicalPosition::new(
                            f64::from(pointer.x()),
                            f64::from(pointer.y()),
                        ),
                    }),
                );
            };
            if matches!(phase, TouchPhase::Started | TouchPhase::Ended) {
                send(motion.pointer_at_index(motion.pointer_index()));
            } else {
                motion.pointers().for_each(send);
            }
            InputStatus::Handled
        }
        InputEvent::KeyEvent(key) => {
            if let Some((physical, event)) =
                crate::android_keyboard::native_key(&active.app, key, accent)
            {
                handler.android_event(active, AndroidEvent::Key(physical, event));
                InputStatus::Handled
            } else {
                InputStatus::Unhandled
            }
        }
        _ => InputStatus::Unhandled,
    }
}
