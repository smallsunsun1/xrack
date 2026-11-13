use crate::{Element, Error, Instance, Result};
use std::sync::Arc;
use xrack_runtime::{StackNavigation, StackSnapshot, Value};

#[derive(Clone, Copy)]
pub(crate) enum Read {
    Current,
    Depth,
    CanGoBack,
    Active,
    Index,
}

impl Read {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "current-page" => Self::Current,
            "depth" => Self::Depth,
            "can-go-back" => Self::CanGoBack,
            "active" => Self::Active,
            "stack-index" => Self::Index,
            _ => return None,
        })
    }

    pub(crate) fn read(self, snapshot: &StackSnapshot, route: Option<Value>) -> Result<Value> {
        Ok(match (self, route) {
            (Self::Current, None) => Value::String(snapshot.current().unwrap_or("").to_owned()),
            (Self::Depth, None) => Value::Int(snapshot.depth() as i64),
            (Self::CanGoBack, None) => Value::Bool(snapshot.depth() > 1),
            (Self::Active, Some(Value::String(route))) => {
                Value::Bool(snapshot.current() == Some(route.as_str()))
            }
            (Self::Index, Some(Value::String(route))) => {
                Value::Int(snapshot.index_of(&route).map_or(-1, |index| index as i64))
            }
            _ => return Err(Error::Evaluation("invalid stack property arguments".into())),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Push,
    Pop,
    Replace,
    PopTo,
    Reset,
    PushPages,
    ResetTo,
}

impl Command {
    pub(crate) fn parse(method: &str) -> Option<Self> {
        Some(match method {
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
}

fn routes(view: &Element) -> Result<&[String]> {
    view.stack_routes
        .as_deref()
        .ok_or_else(|| Error::Invalid(format!("{} is not a StackView", view.key)))
}

impl Instance {
    pub fn stack_snapshot(&self, id: &str) -> Result<Arc<StackSnapshot>> {
        let (owner, view) = self.element_owner(id)?;
        owner.stack_snapshot_for(view)
    }

    pub fn navigate_stack(&mut self, id: &str, action: StackNavigation<'_>) -> Result<bool> {
        let (owner, view) = self.element_owner(id)?;
        owner.navigate_stack_view(view, None, action, None)
    }

    fn initial_route(&self, view: &Element) -> Result<String> {
        match self.binding_value(view, "initial-page")? {
            Some(Value::String(route)) => Ok(route),
            None => Ok(String::new()),
            _ => Err(Error::Evaluation("expected an initial stack route".into())),
        }
    }

    pub(crate) fn stack_snapshot_for(&self, view: &Element) -> Result<Arc<StackSnapshot>> {
        let routes = routes(view)?;
        self.stack_state(view)
            .try_snapshot(routes, || self.initial_route(view))
    }

    pub(crate) fn navigate_stack_command(
        &self,
        id: &str,
        command: Command,
        argument: Option<Value>,
    ) -> Result<bool> {
        let (owner, view) = self.element_owner(id)?;
        let path;
        let action = match (command, argument.as_ref()) {
            (Command::Push, Some(Value::String(route))) => StackNavigation::Push(route),
            (Command::Pop, None) => StackNavigation::Pop,
            (Command::Replace, Some(Value::String(route))) => StackNavigation::Replace(route),
            (Command::PopTo, Some(Value::String(route))) => StackNavigation::PopTo(route),
            (Command::Reset, Some(Value::String(route))) => StackNavigation::Reset(route),
            (Command::PushPages | Command::ResetTo, Some(Value::StringModel(model))) => {
                path = model.snapshot();
                if command == Command::PushPages {
                    StackNavigation::PushPages(&path)
                } else {
                    StackNavigation::ResetTo(&path)
                }
            }
            _ => {
                return Err(Error::Evaluation(
                    "invalid stack navigation arguments".into(),
                ));
            }
        };
        owner.navigate_stack_view(view, None, action, None)
    }

    pub(crate) fn navigate_stack_view(
        &self,
        view: &Element,
        expected: Option<u64>,
        action: StackNavigation<'_>,
        input: Option<xrack_runtime::InputRoute<'_>>,
    ) -> Result<bool> {
        let routes = routes(view)?;
        let page = |route: &str| {
            routes
                .iter()
                .position(|name| name == route)
                .and_then(|index| view.children.get(index))
                .expect("a stack only contains declared pages")
        };
        self.stack_state(view).try_navigate_guarded(
            routes,
            || self.initial_route(view),
            expected,
            action,
            |route, destination, removing| {
                if input.is_some_and(|input| !input.is_current()) {
                    return Ok(false);
                }
                let Some(handler) = page(route).handlers.get("leave-requested") else {
                    return Ok(true);
                };
                let arguments = [Value::String(destination.into()), Value::Bool(removing)];
                match self.execute(handler, &arguments, None)? {
                    Some(Value::Bool(allow)) => {
                        Ok(allow && input.is_none_or(|input| input.is_current()))
                    }
                    _ => Err(Error::Evaluation(
                        "leave-requested must return a boolean".into(),
                    )),
                }
            },
            |change| {
                if let Some(route) = change.deactivated() {
                    self.handler(page(route), "deactivated", &[])?;
                }
                for route in change.removed() {
                    self.handler(page(route), "removed", &[])?;
                }
                if let Some(route) = change.activated() {
                    self.handler(page(route), "activated", &[])?;
                }
                let current = change.after.current().unwrap_or("");
                self.handler(
                    view,
                    "changed",
                    &[
                        Value::String(current.into()),
                        Value::Int(change.after.depth() as i64),
                    ],
                )?;
                Ok(())
            },
        )
    }
}
