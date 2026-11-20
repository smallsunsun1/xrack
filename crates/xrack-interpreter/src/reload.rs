use crate::{Callback, Instance, Node, Program, Result};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use xrack_runtime::{Component, ComponentLifetime, LocaleContext, WeakComponent, WindowStatus};

/// A UI-thread component handle that can replace its interpreted implementation.
///
/// Keep weak handles to this wrapper in application callbacks. Take a fresh
/// `current()` snapshot when an asynchronous result is ready.
#[derive(Clone)]
pub struct ReloadableInstance(Rc<State>);

struct State {
    current: RefCell<Instance>,
    lifetime: ComponentLifetime,
    callbacks: RefCell<BTreeMap<String, Callback>>,
    locale: RefCell<Option<LocaleContext>>,
    window_status: Cell<WindowStatus>,
    replaced: Cell<bool>,
    generation: Cell<u64>,
    #[cfg(all(feature = "live-reload", unix))]
    inspector: xrack_runtime::WindowInspector,
}

impl ReloadableInstance {
    pub fn new(instance: Instance) -> Self {
        Self(Rc::new(State {
            current: RefCell::new(instance),
            lifetime: ComponentLifetime::default(),
            callbacks: RefCell::default(),
            locale: RefCell::default(),
            window_status: Cell::default(),
            replaced: Cell::default(),
            generation: Cell::default(),
            #[cfg(all(feature = "live-reload", unix))]
            inspector: xrack_runtime::WindowInspector::default(),
        }))
    }

    /// Get the instance used now. Use the wrapper itself to observe later replacements.
    pub fn current(&self) -> Instance {
        self.0.current.borrow().clone()
    }

    pub fn as_weak(&self) -> WeakComponent<Self> {
        WeakComponent::new(&self.0, &self.0.lifetime, Self)
    }

    #[cfg(all(feature = "live-reload", unix))]
    pub(crate) fn inspector(&self) -> &xrack_runtime::WindowInspector {
        &self.0.inspector
    }

    /// Install a callback on the current and subsequent instances.
    pub fn on(&self, name: &str, callback: Callback) -> Result<()> {
        self.current().on(name, callback.clone())?;
        self.0.callbacks.borrow_mut().insert(name.into(), callback);
        Ok(())
    }

    /// Validate and render a compatible replacement before publishing it.
    ///
    /// Explicit writable root properties and named timers' running overrides
    /// carry over. Default bindings and derived values use the new program.
    pub fn reload(&self, program: &Program) -> Result<()> {
        self.reload_if_current(program, || true).map(|_| ())
    }

    pub(crate) fn reload_if_current(
        &self,
        program: &Program,
        is_current: impl FnOnce() -> bool,
    ) -> Result<bool> {
        let next = self.current().reloaded_instance(program)?;
        for (name, callback) in self.0.callbacks.borrow().iter() {
            next.on(name, callback.clone())?;
        }
        if let Some(context) = self.0.locale.borrow().clone() {
            next.set_locale_context(context);
        }
        next.publish_window_status(self.0.window_status.get());
        next.try_render()?;
        if !is_current() {
            return Ok(false);
        }
        #[cfg(all(feature = "live-reload", unix))]
        if let Some(trace) = self.current().trace() {
            trace.reloaded();
            next.attach_trace(trace.clone());
        }
        #[cfg(all(feature = "live-reload", unix))]
        self.inspector().advance_generation();
        let previous = self.0.current.replace(next);
        self.0.replaced.set(true);
        let generation = self.0.generation.get() + 1;
        self.0.generation.set(generation);
        xrack_runtime::ui_trace::reload_committed(generation);
        drop(previous);
        Ok(true)
    }
}

// Acquire the current instance before calling into it, with the slot's borrow
// released. Event delivery can call Rust code that also uses the wrapper.
#[doc(hidden)]
#[macro_export]
macro_rules! __xr_forward_component_methods {
    ($this:ident, $access:ident) => {
        fn source_generation(&$this) -> ::core::primitive::u64 {
            $this.$access().source_generation()
        }
        fn window_inspector(
            &$this,
            scene: &::std::rc::Rc<$crate::__live_runtime::Node>,
        ) -> ::core::option::Option<$crate::__live_runtime::WindowInspector> {
            $this.$access().window_inspector(scene)
        }
        $crate::__xr_forward_component_methods!(@forward $this, $access);
    };
    (@forward $this:ident, $access:ident) => {
        $crate::__xr_forward_component_methods!(@methods $this, $access;
        [&mut] close_requested() -> ::core::primitive::bool;
        [&mut] take_window_close_request() -> ::core::primitive::bool;
        [&mut] take_window_commands() -> ::std::vec::Vec<$crate::__live_runtime::WindowCommand>;
        [&] has_window_commands() -> ::core::primitive::bool;
        [&] secondary_windows() -> ::core::option::Option<::std::rc::Rc<[$crate::__live_runtime::SecondaryWindowRegistration]>>;
        [&] list_rows(path: &[::core::primitive::usize], rows: ::std::ops::Range<::core::primitive::usize>) -> ::std::vec::Vec<$crate::__live_runtime::Node>;
        [&] next_table_edit_cell(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], origin: $crate::__live_runtime::TableEditPosition, columns: &[::core::primitive::usize], backward: ::core::primitive::bool) -> ::core::option::Option<$crate::__live_runtime::TableEditPosition>;
        [&] copy_table_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], columns: &[::core::primitive::usize]) -> ::core::option::Option<::std::string::String>;
        [&] route_table_query(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], input: $crate::__live_runtime::InputRoute<'_>) -> ::core::option::Option<$crate::__live_runtime::TableQueryResult>;
        [&] prepare_table_paste(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], row: ::core::primitive::usize, columns: ::std::vec::Vec<::core::primitive::usize>) -> ::core::option::Option<$crate::__live_runtime::TablePaste>;
        [&] table_paste_stamp(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize]) -> ::core::option::Option<(::core::primitive::u64, ::core::primitive::u64)>;
        [&] replay_table_paste_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], redo: ::core::primitive::bool) -> ::core::primitive::bool;
        [&] replay_table_edit_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], redo: ::core::primitive::bool) -> ::core::primitive::bool;
        [&mut] dispatch(element_id: &::core::primitive::str, event: &::core::primitive::str) -> ::core::primitive::bool;
        [&mut] edit_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], text: ::std::string::String) -> ::core::primitive::bool;
        [&mut] commit_table_cell(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], original: &::core::primitive::str, text: ::std::string::String) -> ::core::primitive::bool;
        [&mut] adjust_range_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], action: $crate::__live_runtime::RangeAction) -> ::core::primitive::bool;
        [&mut] control_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], action: $crate::__live_runtime::ControlAction) -> ::core::primitive::bool;
        [&mut] drag_event_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], event: &$crate::__live_runtime::DragEvent) -> ::core::primitive::bool;
        [&mut] key_event_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], phase: $crate::__live_runtime::KeyPhase, event: &$crate::__live_runtime::KeyEvent) -> $crate::__live_runtime::EventResult;
        [&mut] focus_event_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], event: $crate::__live_runtime::FocusEvent) -> ::core::primitive::bool;
        [&mut] wheel_event_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], event: &$crate::__live_runtime::WheelEvent) -> $crate::__live_runtime::EventResult;
        [&mut] pointer_event_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], event: &$crate::__live_runtime::PointerEvent) -> ::core::primitive::bool;
        [&mut] route_input(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], input: $crate::__live_runtime::InputRoute<'_>) -> ::core::primitive::bool;
        [&mut] resize_split_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], weights: &[(::core::primitive::usize, ::core::primitive::f32)]) -> ::core::primitive::bool;
        [&mut] resize_split_sizes_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], sizes: &[(::core::primitive::usize, ::core::primitive::f32)]) -> ::core::primitive::bool;
        [&mut] dispatch_at(scene: &$crate::__live_runtime::Node, path: &[::core::primitive::usize], event: &::core::primitive::str) -> ::core::primitive::bool;
        );
    };
    (@methods $this:ident, $access:ident; $( [$($receiver:tt)+] $method:ident(
        $($arg:ident: $ty:ty),*
    ) $(-> $result:ty)?; )*) => {
        $(
            fn $method($($receiver)+ $this $(, $arg: $ty)*) $(-> $result)? {
                $this.$access().$method($($arg),*)
            }
        )*
    };
}

impl Component for ReloadableInstance {
    fn render(&self) -> Node {
        self.render_shared().as_ref().clone()
    }

    fn source_generation(&self) -> u64 {
        self.0.generation.get()
    }

    #[cfg(all(feature = "live-reload", unix))]
    fn window_inspector(&self, scene: &Rc<Node>) -> Option<xrack_runtime::WindowInspector> {
        let inspector = self.inspector();
        if !inspector.attached() {
            return inspector.observing().then(|| inspector.clone());
        }
        if !inspector.matches_scene(scene) {
            crate::live_inspect::sync_targets(&self.current(), scene, inspector);
        }
        Some(inspector.clone())
    }

    fn render_shared(&self) -> Rc<Node> {
        self.0.replaced.set(false);
        self.current().render_shared()
    }

    fn scene_dirty(&self) -> bool {
        self.0.replaced.get() || self.current().scene_dirty()
    }

    fn set_locale_context(&self, context: LocaleContext) {
        self.0.locale.replace(Some(context.clone()));
        self.current().set_locale_context(context);
    }

    fn publish_window_status(&self, status: WindowStatus) {
        self.0.window_status.set(status);
        self.current().publish_window_status(status);
    }

    crate::__xr_forward_component_methods!(@forward self, current);
}
