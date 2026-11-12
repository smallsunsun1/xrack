use crate::{Element, Error, Instance, Result};
use std::{collections::BTreeMap, rc::Rc};
use xrack_runtime::{Component, Property, SecondaryWindowRegistration, SecondaryWindowSlot, Value};

#[derive(Clone, Copy)]
pub(crate) enum Command {
    Visible(bool),
    Close,
}

impl Command {
    pub(crate) fn parse(command: &str) -> Option<Self> {
        Some(match command {
            "show" => Self::Visible(true),
            "hide" => Self::Visible(false),
            "close" => Self::Close,
            _ => return None,
        })
    }
}

pub(crate) struct State {
    slots: BTreeMap<String, SecondaryWindowSlot>,
    registrations: Property<Result<Rc<[SecondaryWindowRegistration]>>>,
}

impl State {
    pub(crate) fn new(windows: &[Element]) -> Option<Self> {
        if windows.is_empty() {
            return None;
        }
        Some(Self {
            slots: windows
                .iter()
                .map(|window| (window.key.clone(), Default::default()))
                .collect(),
            registrations: Property::default(),
        })
    }
}

impl Instance {
    fn secondary_window_visible(&self, window: &Element) -> Result<bool> {
        Ok(self.control_value(window, "visible")? == Some(Value::Bool(true)))
    }

    fn secondary_window_child(&self, window: &Element) -> Result<&Instance> {
        window
            .children
            .first()
            .and_then(|child| self.children().get(&child.key))
            .ok_or_else(|| Error::Evaluation(format!("missing window instance for {}", window.key)))
    }

    pub(crate) fn secondary_window_command(&self, target: &str, command: Command) -> Result<()> {
        let (owner, window) = self.element_owner(target)?;
        match command {
            Command::Visible(visible) => {
                owner.set_control_value(window, "visible", visible.into())?;
            }
            Command::Close if owner.secondary_window_visible(window)? => {
                owner
                    .secondary_window_child(window)?
                    .0
                    .window
                    .request_close();
            }
            Command::Close => {}
        }
        Ok(())
    }

    pub(crate) fn secondary_window_registrations(
        &self,
    ) -> Result<Option<Rc<[SecondaryWindowRegistration]>>> {
        let Some(state) = &self.0.secondary_windows else {
            return Ok(None);
        };
        state
            .registrations
            .get(|| {
                let mut registrations = Vec::new();
                for window in &self.definition().root.secondary_windows {
                    if !self.secondary_window_visible(window)? {
                        continue;
                    }
                    let child = Rc::downgrade(&self.secondary_window_child(window)?.0);
                    let owner = Rc::downgrade(&self.0);
                    let key = window.key.clone();
                    registrations.push(state.slots[&key].register(
                        move || {
                            child
                                .upgrade()
                                .map(|child| Box::new(Instance(child)) as Box<dyn Component>)
                        },
                        move || {
                            if let Some(owner) = owner.upgrade() {
                                let owner = Instance(owner);
                                if let Err(error) =
                                    owner.secondary_window_command(&key, Command::Visible(false))
                                {
                                    owner.0.error.set(Some(error));
                                }
                            }
                        },
                    ));
                }
                Ok(registrations.into())
            })
            .map(Some)
    }
}
