use crate::{Element, Error, Instance, Result};
use std::sync::Arc;
use xrack_mlir::StackRead;
use xrack_runtime::{
    StackEvent, StackNavigation, StackNavigationArgument, StackNavigationMethod, StackSnapshot,
    Value,
};

pub(crate) fn read(snapshot: &StackSnapshot, read: &StackRead) -> Value {
    match read {
        StackRead::CurrentPage => snapshot.current_page().into(),
        StackRead::Depth => (snapshot.depth() as i64).into(),
        StackRead::CanGoBack => snapshot.can_go_back().into(),
        StackRead::PageActive(route) => snapshot.page_active(route).into(),
        StackRead::PageIndex(route) => snapshot.page_index(route).into(),
    }
}

fn routes(view: &Element) -> Result<&[String]> {
    view.stack_routes
        .as_deref()
        .ok_or_else(|| Error::Invalid(format!("{} is not a StackView", view.key)))
}

impl Instance {
    pub fn stack_snapshot(&self, id: &str) -> Result<Arc<StackSnapshot>> {
        let view = self.host_element(id, |view| view.stack_routes.is_some())?;
        self.stack_snapshot_for(view)
    }

    pub fn navigate_stack(&mut self, id: &str, action: StackNavigation<'_>) -> Result<bool> {
        let view = self.host_element(id, |view| view.stack_routes.is_some())?;
        self.navigate_stack_view(view, None, action, None)
    }

    pub(crate) fn stack_snapshot_by_key(&self, key: &str) -> Result<Arc<StackSnapshot>> {
        let (owner, view) = self.element_owner(key)?;
        owner.stack_snapshot_for(view)
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
        method: StackNavigationMethod,
        argument: Option<Value>,
    ) -> Result<bool> {
        let (owner, view) = self.element_owner(id)?;
        let path;
        let argument = match argument.as_ref() {
            None => StackNavigationArgument::None,
            Some(Value::String(route)) => StackNavigationArgument::Route(route),
            Some(Value::StringModel(model)) => {
                path = model.snapshot();
                StackNavigationArgument::Path(&path)
            }
            _ => {
                return Err(Error::Evaluation(
                    "invalid stack navigation arguments".into(),
                ));
            }
        };
        let action = method
            .action(argument)
            .ok_or_else(|| Error::Evaluation("invalid stack navigation arguments".into()))?;
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
                    return Ok(input.is_none_or(|input| input.is_current()));
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
                change.try_notify(|event| {
                    match event {
                        StackEvent::Deactivated(route) => {
                            self.handler(page(route), "deactivated", &[])
                        }
                        StackEvent::Removed(route) => self.handler(page(route), "removed", &[]),
                        StackEvent::Activated(route) => self.handler(page(route), "activated", &[]),
                        StackEvent::Changed { current, depth } => self.handler(
                            view,
                            "changed",
                            &[Value::String(current.into()), Value::Int(depth)],
                        ),
                    }
                    .map(|_| ())
                })
            },
        )
    }
}
