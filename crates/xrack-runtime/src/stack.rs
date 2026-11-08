use crate::Property;
use std::{
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

#[derive(Debug, PartialEq, Eq)]
pub struct StackSnapshot {
    routes: Vec<&'static str>,
    revision: u64,
}

impl StackSnapshot {
    fn new(routes: Vec<&'static str>) -> Self {
        let revision = NEXT_REVISION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("navigation revision exhausted");
        Self { routes, revision }
    }

    pub fn routes(&self) -> &[&'static str] {
        &self.routes
    }

    pub fn current(&self) -> Option<&'static str> {
        self.routes.last().copied()
    }

    pub fn depth(&self) -> usize {
        self.routes.len()
    }

    pub fn revision(&self) -> u64 {
        self.revision
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
        self.snapshot.get(|| {
            let initial = initial();
            let route = pages
                .iter()
                .copied()
                .find(|&page| page == initial)
                .or_else(|| pages.first().copied());
            Arc::new(StackSnapshot::new(route.into_iter().collect()))
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
        if self.notifying.replace(true) {
            return false;
        }
        let _guard = NavigationGuard(&self.notifying);
        let current = self.snapshot(pages, initial);
        if expected.is_some_and(|revision| revision != current.revision()) {
            return false;
        }
        let resolve = |route| pages.iter().copied().find(|&page| page == route);
        let mut routes = current.routes.clone();
        match action {
            StackNavigation::Push(route) | StackNavigation::Replace(route) => {
                let Some(page) = resolve(route).filter(|page| !routes.contains(page)) else {
                    return false;
                };
                if matches!(action, StackNavigation::Replace(_)) {
                    routes.pop();
                }
                routes.push(page);
            }
            StackNavigation::Pop if routes.len() > 1 => {
                routes.pop();
            }
            StackNavigation::Pop => return false,
            StackNavigation::PopTo(route) => {
                let Some(index) = routes.iter().position(|&page| page == route) else {
                    return false;
                };
                routes.truncate(index + 1);
            }
            StackNavigation::Reset(route) => {
                let Some(page) = resolve(route) else {
                    return false;
                };
                routes.clear();
                routes.push(page);
            }
            StackNavigation::PushPages(path) | StackNavigation::ResetTo(path) => {
                if path.is_empty() {
                    return false;
                }
                if matches!(action, StackNavigation::ResetTo(_)) {
                    routes.clear();
                }
                for route in path {
                    let Some(page) = resolve(route.as_str()).filter(|page| !routes.contains(page))
                    else {
                        return false;
                    };
                    routes.push(page);
                }
            }
        }
        if routes == current.routes {
            return false;
        }
        let next = Arc::new(StackSnapshot::new(routes));
        self.snapshot.set(next.clone());
        changed(&next);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
