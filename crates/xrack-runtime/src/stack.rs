use crate::Property;
use std::{
    borrow::Cow,
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackNavigation<'a> {
    Push(&'a str),
    Pop,
    Replace(&'a str),
    PopTo(&'a str),
    Reset(&'a str),
    PushPages(&'a [String]),
    ResetTo(&'a [String]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub enum StackNavigationMethod {
    Push,
    Pop,
    Replace,
    PopTo,
    Reset,
    PushPages,
    ResetTo,
}

#[doc(hidden)]
pub enum StackNavigationArgument<'a> {
    None,
    Route(&'a str),
    Path(&'a [String]),
}

impl StackNavigationMethod {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "push" => Self::Push,
            "pop" => Self::Pop,
            "replace" => Self::Replace,
            "pop-to" => Self::PopTo,
            "reset" => Self::Reset,
            "push-pages" => Self::PushPages,
            "reset-to" => Self::ResetTo,
            _ => return None,
        })
    }

    pub fn uses_path(self) -> bool {
        matches!(self, Self::PushPages | Self::ResetTo)
    }

    pub fn action(self, argument: StackNavigationArgument<'_>) -> Option<StackNavigation<'_>> {
        use StackNavigationArgument::{Path, Route};
        Some(match (self, argument) {
            (Self::Push, Route(route)) => StackNavigation::Push(route),
            (Self::Pop, StackNavigationArgument::None) => StackNavigation::Pop,
            (Self::Replace, Route(route)) => StackNavigation::Replace(route),
            (Self::PopTo, Route(route)) => StackNavigation::PopTo(route),
            (Self::Reset, Route(route)) => StackNavigation::Reset(route),
            (Self::PushPages, Path(path)) => StackNavigation::PushPages(path),
            (Self::ResetTo, Path(path)) => StackNavigation::ResetTo(path),
            _ => return None,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct StackSnapshot {
    routes: Vec<Cow<'static, str>>,
    revision: u64,
}

impl StackSnapshot {
    fn new(routes: Vec<Cow<'static, str>>) -> Self {
        let revision = NEXT_REVISION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("navigation revision exhausted");
        Self { routes, revision }
    }

    pub fn routes(&self) -> &[Cow<'static, str>] {
        &self.routes
    }

    pub fn current(&self) -> Option<&str> {
        self.routes.last().map(Cow::as_ref)
    }

    pub fn index_of(&self, route: &str) -> Option<usize> {
        self.routes.iter().position(|page| page == route)
    }

    pub fn depth(&self) -> usize {
        self.routes.len()
    }

    #[doc(hidden)]
    pub fn current_page(&self) -> String {
        self.current().unwrap_or_default().to_owned()
    }

    #[doc(hidden)]
    pub fn can_go_back(&self) -> bool {
        self.depth() > 1
    }

    #[doc(hidden)]
    pub fn page_active(&self, route: &str) -> bool {
        self.current() == Some(route)
    }

    #[doc(hidden)]
    pub fn page_index(&self, route: &str) -> i64 {
        self.index_of(route).map_or(-1, |index| index as i64)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// The route changes associated with a single successful navigation command.
#[doc(hidden)]
pub struct StackChange<'a> {
    pub before: &'a StackSnapshot,
    pub after: &'a StackSnapshot,
}

#[doc(hidden)]
pub enum StackEvent<'a> {
    Deactivated(&'a str),
    Removed(&'a str),
    Activated(&'a str),
    Changed { current: &'a str, depth: i64 },
}

impl StackChange<'_> {
    pub fn deactivated(&self) -> Option<&str> {
        self.before
            .current()
            .filter(|page| Some(*page) != self.after.current())
    }

    pub fn activated(&self) -> Option<&str> {
        self.after
            .current()
            .filter(|page| Some(*page) != self.before.current())
    }

    pub fn removed(&self) -> impl Iterator<Item = &str> + '_ {
        self.before
            .routes
            .iter()
            .map(Cow::as_ref)
            .rev()
            .filter(|route| self.after.index_of(route).is_none())
    }

    pub fn try_notify<E>(
        &self,
        mut notify: impl FnMut(StackEvent<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        if let Some(route) = self.deactivated() {
            notify(StackEvent::Deactivated(route))?;
        }
        for route in self.removed() {
            notify(StackEvent::Removed(route))?;
        }
        if let Some(route) = self.activated() {
            notify(StackEvent::Activated(route))?;
        }
        notify(StackEvent::Changed {
            current: self.after.current().unwrap_or(""),
            depth: self.after.depth() as i64,
        })
    }

    pub fn notify(&self, mut notify: impl FnMut(StackEvent<'_>)) {
        self.try_notify(|event| {
            notify(event);
            Ok::<_, std::convert::Infallible>(())
        })
        .unwrap_or_else(|error| match error {})
    }
}

#[derive(Default)]
#[doc(hidden)]
pub struct StackState {
    snapshot: Property<Arc<StackSnapshot>>,
    notifying: Cell<bool>,
}

struct NavigationGuard<'a>(&'a Cell<bool>);

impl Drop for NavigationGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl StackState {
    pub fn snapshot(
        &self,
        pages: &[&'static str],
        initial: impl FnOnce() -> String,
    ) -> Arc<StackSnapshot> {
        self.try_snapshot(pages, || Ok::<_, std::convert::Infallible>(initial()))
            .unwrap_or_else(|error| match error {})
    }

    pub fn try_snapshot<R, E>(
        &self,
        pages: &[R],
        initial: impl FnOnce() -> Result<String, E>,
    ) -> Result<Arc<StackSnapshot>, E>
    where
        R: Clone + AsRef<str> + Into<Cow<'static, str>>,
    {
        self.snapshot.try_get(|| {
            let initial = initial()?;
            let route = pages
                .iter()
                .find(|page| page.as_ref() == initial)
                .or_else(|| pages.first())
                .cloned()
                .map(Into::into);
            Ok(Arc::new(StackSnapshot::new(route.into_iter().collect())))
        })
    }

    pub fn navigate(
        &self,
        pages: &[&'static str],
        initial: impl FnOnce() -> String,
        expected: Option<u64>,
        action: StackNavigation<'_>,
        changed: impl FnOnce(&StackSnapshot),
    ) -> bool {
        self.navigate_guarded(
            pages,
            initial,
            expected,
            action,
            |_, _, _| true,
            |change| {
                changed(change.after);
            },
        )
    }

    pub fn navigate_guarded(
        &self,
        pages: &[&'static str],
        initial: impl FnOnce() -> String,
        expected: Option<u64>,
        action: StackNavigation<'_>,
        mut can_leave: impl FnMut(&str, &str, bool) -> bool,
        changed: impl FnOnce(StackChange<'_>),
    ) -> bool {
        self.try_navigate_guarded(
            pages,
            || Ok::<_, std::convert::Infallible>(initial()),
            expected,
            action,
            |route, destination, removing| Ok(can_leave(route, destination, removing)),
            |change| {
                changed(change);
                Ok(())
            },
        )
        .unwrap_or_else(|error| match error {})
    }

    pub fn try_navigate_guarded<R, E>(
        &self,
        pages: &[R],
        initial: impl FnOnce() -> Result<String, E>,
        expected: Option<u64>,
        action: StackNavigation<'_>,
        mut can_leave: impl FnMut(&str, &str, bool) -> Result<bool, E>,
        changed: impl FnOnce(StackChange<'_>) -> Result<(), E>,
    ) -> Result<bool, E>
    where
        R: Clone + AsRef<str> + Into<Cow<'static, str>>,
    {
        if self.notifying.replace(true) {
            return Ok(false);
        }
        let _guard = NavigationGuard(&self.notifying);
        let current = self.try_snapshot(pages, initial)?;
        if expected.is_some_and(|revision| revision != current.revision()) {
            return Ok(false);
        }
        let resolve = |route| {
            pages
                .iter()
                .find(|page| page.as_ref() == route)
                .cloned()
                .map(Into::into)
        };
        let mut routes = current.routes.clone();
        match action {
            StackNavigation::Push(route) | StackNavigation::Replace(route) => {
                let Some(page) = resolve(route).filter(|page| !routes.contains(page)) else {
                    return Ok(false);
                };
                if matches!(action, StackNavigation::Replace(_)) {
                    routes.pop();
                }
                routes.push(page);
            }
            StackNavigation::Pop if routes.len() > 1 => {
                routes.pop();
            }
            StackNavigation::Pop => return Ok(false),
            StackNavigation::PopTo(route) => {
                let Some(index) = routes.iter().position(|page| page == route) else {
                    return Ok(false);
                };
                routes.truncate(index + 1);
            }
            StackNavigation::Reset(route) => {
                let Some(page) = resolve(route) else {
                    return Ok(false);
                };
                routes.clear();
                routes.push(page);
            }
            StackNavigation::PushPages(path) | StackNavigation::ResetTo(path) => {
                if path.is_empty() {
                    return Ok(false);
                }
                if matches!(action, StackNavigation::ResetTo(_)) {
                    routes.clear();
                }
                for route in path {
                    let Some(page) = resolve(route.as_str()).filter(|page| !routes.contains(page))
                    else {
                        return Ok(false);
                    };
                    routes.push(page);
                }
            }
        }
        if routes == current.routes {
            return Ok(false);
        }
        let next = Arc::new(StackSnapshot::new(routes));
        let change = StackChange {
            before: &current,
            after: &next,
        };
        let outgoing = change.deactivated();
        let leaving = outgoing
            .into_iter()
            .chain(change.removed().filter(|route| Some(*route) != outgoing));
        let destination = next.current().unwrap_or("");
        for route in leaving {
            if !can_leave(route, destination, next.index_of(route).is_none())? {
                return Ok(false);
            }
        }
        // An effectful guard can change an input to the still-implicit initial route.
        if !self
            .snapshot
            .matches_current(|latest| Arc::ptr_eq(latest, &current))
        {
            return Ok(false);
        }
        self.snapshot.set(next.clone());
        changed(change)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_routes_recover_from_initialization_and_guard_errors() {
        let pages = ["home".to_owned(), "editor".to_owned()];
        let state = StackState::default();
        assert!(matches!(
            state.try_snapshot(&pages, || Err("initial route")),
            Err("initial route")
        ));
        let initial = || Ok::<_, &str>("home".to_owned());
        let before = state.try_snapshot(&pages, initial).unwrap();
        assert!(matches!(
            state.try_navigate_guarded(
                &pages,
                initial,
                Some(before.revision()),
                StackNavigation::Push("editor"),
                |_, _, _| Err("guard failed"),
                |_| unreachable!(),
            ),
            Err("guard failed")
        ));
        assert!(Arc::ptr_eq(
            &before,
            &state.try_snapshot(&pages, initial).unwrap()
        ));
        assert!(
            state
                .try_navigate_guarded(
                    &pages,
                    initial,
                    Some(before.revision()),
                    StackNavigation::Push("editor"),
                    |_, _, _| Ok(true),
                    |_| Ok(()),
                )
                .unwrap()
        );
        assert_eq!(
            state.try_snapshot(&pages, initial).unwrap().routes(),
            ["home", "editor"]
        );
    }

    #[test]
    fn navigation_rejects_stale_actions_and_reentrant_notifications() {
        let stack = StackState::default();
        let pages = ["home", "detail", "preview"];
        let initial = || "home".to_owned();
        let initial_revision = stack.snapshot(&pages, initial).revision();
        let calls = Cell::new(0);
        assert!(stack.navigate(
            &pages,
            initial,
            Some(initial_revision),
            StackNavigation::Push("detail"),
            |snapshot| {
                assert_eq!(snapshot.routes(), ["home", "detail"]);
                calls.set(calls.get() + 1);
                assert!(!stack.navigate(
                    &pages,
                    initial,
                    None,
                    StackNavigation::Pop,
                    |_| unreachable!(),
                ));
            },
        ));
        assert!(!stack.navigate(
            &pages,
            initial,
            Some(initial_revision),
            StackNavigation::Pop,
            |_| unreachable!(),
        ));
        assert!(!stack.navigate(
            &pages,
            initial,
            None,
            StackNavigation::Push("home"),
            |_| unreachable!(),
        ));
        assert!(stack.navigate(
            &pages,
            initial,
            None,
            StackNavigation::Replace("preview"),
            |_| calls.set(calls.get() + 1),
        ));
        assert_eq!(
            stack.snapshot(&pages, initial).routes(),
            ["home", "preview"]
        );
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn guarded_navigation_keeps_its_revision_when_a_hidden_route_vetoes() {
        let stack = StackState::default();
        let pages = ["home", "editor", "preview", "other"];
        let initial = || "home".to_owned();
        let open = ["home", "editor", "preview"].map(str::to_owned);
        assert!(stack.navigate(
            &pages,
            initial,
            None,
            StackNavigation::ResetTo(&open),
            |_| {}
        ));
        let before = stack.snapshot(&pages, initial);
        let mut guards = Vec::new();
        assert!(!stack.navigate_guarded(
            &pages,
            initial,
            Some(before.revision()),
            StackNavigation::Reset("home"),
            |route, destination, removing| {
                assert_eq!(destination, "home");
                assert!(removing);
                guards.push(route.to_owned());
                route != "editor"
            },
            |_| panic!("a veto must not emit a change"),
        ));
        assert_eq!(guards, ["preview", "editor"]);
        assert!(Arc::ptr_eq(&before, &stack.snapshot(&pages, initial)));

        let keep_top = ["home", "other", "preview"].map(str::to_owned);
        assert!(stack.navigate_guarded(
            &pages,
            initial,
            Some(before.revision()),
            StackNavigation::ResetTo(&keep_top),
            |route, destination, removing| {
                assert_eq!((route, destination, removing), ("editor", "preview", true));
                true
            },
            |change| {
                assert_eq!(change.deactivated(), None);
                assert_eq!(change.activated(), None);
                assert_eq!(change.removed().collect::<Vec<_>>(), ["editor"]);
                assert_eq!(
                    stack.snapshot(&pages, initial).routes(),
                    ["home", "other", "preview"]
                );
            },
        ));
    }

    #[test]
    fn a_guard_cannot_commit_against_an_initial_route_it_invalidated() {
        let stack = StackState::default();
        let seed = Property::default();
        let pages = ["home", "editor", "preview"];
        let initial = || seed.get(|| "home".to_owned());
        assert!(!stack.navigate_guarded(
            &pages,
            initial,
            None,
            StackNavigation::Push("preview"),
            |route, _, _| {
                assert_eq!(route, "home");
                seed.set("editor".to_owned());
                assert_eq!(stack.snapshot(&pages, initial).current(), Some("editor"));
                true
            },
            |_| panic!("the route checked before the guard is stale"),
        ));
        assert_eq!(stack.snapshot(&pages, initial).routes(), ["editor"]);
    }

    #[test]
    fn navigation_paths_commit_atomically_and_notify_once() {
        let stack = StackState::default();
        let pages = ["home", "detail", "preview", "summary"];
        let initial = || "home".to_owned();
        let before = stack.snapshot(&pages, initial);
        let bad = ["detail", "missing"].map(str::to_owned);
        assert!(!stack.navigate(
            &pages,
            initial,
            None,
            StackNavigation::PushPages(&bad),
            |_| unreachable!()
        ));
        let duplicate = ["detail", "home", "detail"].map(str::to_owned);
        assert!(!stack.navigate(
            &pages,
            initial,
            None,
            StackNavigation::ResetTo(&duplicate),
            |_| unreachable!()
        ));
        assert!(Arc::ptr_eq(&before, &stack.snapshot(&pages, initial)));

        let calls = Cell::new(0);
        let path = ["detail", "preview"].map(str::to_owned);
        assert!(stack.navigate(
            &pages,
            initial,
            None,
            StackNavigation::PushPages(&path),
            |next| {
                assert_eq!(next.routes(), ["home", "detail", "preview"]);
                calls.set(calls.get() + 1);
            }
        ));
        let reset = ["home", "summary"].map(str::to_owned);
        assert!(stack.navigate(
            &pages,
            initial,
            None,
            StackNavigation::ResetTo(&reset),
            |next| {
                assert_eq!(next.routes(), ["home", "summary"]);
                calls.set(calls.get() + 1);
            }
        ));
        assert_eq!(calls.get(), 2);
    }
}
