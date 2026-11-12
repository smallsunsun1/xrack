use crate::{Element, Error, Instance, Result, Visual};
use xrack_runtime::{
    Color, ColorPopupState, ControlAction, Date, DateBounds, DatePopupState, DialogRole, Font,
    FontPopupState, Node, RangeAction, Time, TimePickerOptions, TimePopupState, Value,
};

pub(crate) enum State {
    Date(DatePopupState),
    Time(TimePopupState),
    Color(ColorPopupState),
    Font(FontPopupState),
}

impl State {
    pub(crate) fn new(element: &Element) -> Self {
        match element.visual {
            Visual::Builtin("DatePickerPopup") => Self::Date(DatePopupState::default()),
            Visual::Builtin("TimePickerPopup") => Self::Time(TimePopupState::default()),
            Visual::Builtin("ColorPickerPopup") => Self::Color(ColorPopupState::default()),
            Visual::Builtin("FontPickerPopup") => Self::Font(FontPopupState::default()),
            _ => unreachable!("picker control"),
        }
    }

    pub(crate) fn close(&self) {
        match self {
            Self::Date(picker) => picker.close(),
            Self::Time(picker) => picker.close(),
            Self::Color(picker) => picker.close(),
            Self::Font(picker) => picker.close(),
        }
    }
}

pub(crate) fn property(element: &Element) -> Option<&'static str> {
    match element.visual {
        Visual::Builtin("DatePickerPopup") => Some("date"),
        Visual::Builtin("TimePickerPopup") => Some("time"),
        Visual::Builtin("ColorPickerPopup") => Some("color"),
        Visual::Builtin("FontPickerPopup") => Some("font"),
        _ => None,
    }
}

pub(crate) enum Input {
    Control(ControlAction),
    Range(RangeAction),
    Edit(String),
}

impl Instance {
    fn picker_value<T>(
        &self,
        element: &Element,
        name: &str,
        extract: impl FnOnce(Value) -> Option<T>,
    ) -> Result<T> {
        let value = self
            .control_value(element, name)?
            .map_or_else(|| self.default_control_value(element, name), Ok)?;
        extract(value).ok_or_else(|| Error::Evaluation(format!("invalid picker value for {name}")))
    }

    fn date_value(&self, element: &Element) -> Result<Date> {
        self.picker_value(element, "date", |value| match value {
            Value::Date(date) => Some(date),
            _ => None,
        })
    }

    fn time_value(&self, element: &Element) -> Result<Time> {
        self.picker_value(element, "time", |value| match value {
            Value::Time(time) => Some(time),
            _ => None,
        })
    }

    fn color_value(&self, element: &Element) -> Result<Color> {
        self.picker_value(element, "color", |value| match value {
            Value::Color(color) => Some(color),
            _ => None,
        })
    }

    fn font_value(&self, element: &Element) -> Result<Font> {
        self.picker_value(element, "font", |value| match value {
            Value::Font(font) => Some(font),
            _ => None,
        })
    }

    fn date_bounds(&self, element: &Element) -> Result<DateBounds> {
        let date = |name, fallback| match self.binding_value(element, name)? {
            Some(Value::Date(value)) => Ok(value),
            None => Ok(fallback),
            _ => Err(Error::Evaluation(format!("expected a date for {name}"))),
        };
        Ok(DateBounds::new(
            date("minimum-date", Date::MIN)?,
            date("maximum-date", Date::MAX)?,
        ))
    }

    fn time_options(&self, element: &Element) -> Result<TimePickerOptions> {
        let use_24 = self.binding_value(element, "use-24-hour-format")? != Some(Value::Bool(false));
        let seconds = self.binding_value(element, "show-seconds")? == Some(Value::Bool(true));
        Ok(TimePickerOptions::new(use_24, seconds))
    }

    fn show_alpha(&self, element: &Element) -> Result<bool> {
        Ok(self.binding_value(element, "show-alpha")? == Some(Value::Bool(true)))
    }

    pub(crate) fn populate_picker(&self, element: &Element, node: &mut Node) -> Result<()> {
        match self.picker_state(element).as_ref() {
            State::Date(picker) => picker.try_populate(node, || self.date_value(element)),
            State::Time(picker) => picker.try_populate(node, || self.time_value(element)),
            State::Color(picker) => picker.try_populate_editor(node, || self.color_value(element)),
            State::Font(picker) => picker.try_populate(node, || self.font_value(element)),
        }
    }

    pub(crate) fn picker_control(
        &self,
        element: &Element,
        node: &Node,
        action: ControlAction,
    ) -> Result<bool> {
        let ControlAction::Dialog(role) = action else {
            return Ok(false);
        };
        if self.control_value(element, "open")? != Some(Value::Bool(true)) {
            return Ok(false);
        }
        let value = match self.picker_state(element).as_ref() {
            State::Date(picker) => picker
                .finish(self.date_bounds(element)?, node)
                .map(Value::Date),
            State::Time(picker) => picker.finish(node).map(Value::Time),
            State::Color(picker) => picker
                .finish_editor(self.show_alpha(element)?, node, role)
                .map(Value::Color),
            State::Font(picker) => picker.finish(node).map(Value::Font),
        };
        let Some(value) = value else { return Ok(false) };
        self.set_control_value(element, "open", false.into())?;
        match role {
            DialogRole::Accept => {
                self.set_control_value(
                    element,
                    property(element).expect("picker property"),
                    value.clone(),
                )?;
                self.handler(element, "accepted", &[value])?;
            }
            DialogRole::Reject => {
                self.handler(element, "canceled", &[])?;
            }
        }
        Ok(true)
    }

    pub(crate) fn picker_child_input(
        &self,
        scene: &Node,
        path: &[usize],
        node: &Node,
        input: Input,
    ) -> Result<bool> {
        let mut target = None;
        for depth in (0..path.len()).rev() {
            if let Some((owner, element)) = self.try_target(scene, &path[..depth])? {
                target = Some((owner, element, &path[depth..]));
                break;
            }
        }
        let Some((owner, element, tail)) = target else {
            return Ok(false);
        };
        if matches!(element.visual, Visual::Builtin("TimePicker")) {
            let ([slot], Input::Control(action)) = (tail, input) else {
                return Ok(false);
            };
            let Some(time) = owner.time_options(element)?.select(
                owner.time_value(element)?,
                *slot,
                node,
                action,
            ) else {
                return Ok(false);
            };
            owner.set_control_value(element, "time", time.into())?;
            owner.handler(element, "changed", &[time.into()])?;
            return Ok(true);
        }
        if property(element).is_none()
            || owner.control_value(element, "open")? != Some(Value::Bool(true))
        {
            return Ok(false);
        }
        Ok(match (owner.picker_state(element).as_ref(), input, tail) {
            (State::Date(picker), Input::Control(action), [0]) => {
                picker.edit(owner.date_bounds(element)?, node, action)
            }
            (State::Time(picker), Input::Control(action), [0, slot]) => {
                picker.edit(owner.time_options(element)?, *slot, node, action)
            }
            (State::Font(picker), Input::Control(action), [0, ..]) => picker.edit(node, action),
            (State::Color(picker), Input::Control(ControlAction::Spin(action)), [0, ..]) => {
                picker.spin(owner.show_alpha(element)?, node, action)
            }
            (State::Color(picker), Input::Range(action), [0, 5, ..]) => {
                owner.binding_value(element, "show-color-plane")? == Some(Value::Bool(true))
                    && picker.adjust_plane(node, action)
            }
            (State::Color(picker), Input::Range(action), [0, ..]) => {
                picker.slide(owner.show_alpha(element)?, node, action)
            }
            (State::Color(picker), Input::Edit(text), [0, 0, 1]) => {
                picker.edit_hex(owner.show_alpha(element)?, node, text)
            }
            _ => false,
        })
    }
}
