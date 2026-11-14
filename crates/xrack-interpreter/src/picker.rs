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
    let Visual::Builtin(kind) = element.visual else {
        return None;
    };
    xrack_resources::visual::picker_value(kind).map(|(name, _)| name)
}

pub(crate) enum Input<'a> {
    Control(ControlAction, xrack_runtime::InputRoute<'a>),
    Range(RangeAction, xrack_runtime::InputRoute<'a>),
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
        route: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let ControlAction::Dialog(role) = action else {
            return Ok(false);
        };
        let value = match self.picker_state(element).as_ref() {
            State::Date(picker) => picker
                .try_finish_input(node, role, route, || self.date_bounds(element))?
                .map(Value::Date),
            State::Time(picker) => picker
                .try_finish_input(node, role, route, || self.time_options(element))?
                .map(Value::Time),
            State::Color(picker) => picker
                .try_finish_input(node, role, route, || self.show_alpha(element))?
                .map(Value::Color),
            State::Font(picker) if route.is_current() => {
                picker.finish_with_role(node, role).map(Value::Font)
            }
            _ => return Ok(false),
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
        element: &Element,
        tail: &[usize],
        node: &Node,
        input: Input<'_>,
    ) -> Result<bool> {
        if matches!(element.visual, Visual::Builtin("TimePicker")) {
            let ([slot], Input::Control(action, route)) = (tail, input) else {
                return Ok(false);
            };
            let Some(time) =
                self.time_options(element)?
                    .select(self.time_value(element)?, *slot, node, action)
            else {
                return Ok(false);
            };
            if !route.is_current() {
                return Ok(false);
            }
            self.set_control_value(element, "time", time.into())?;
            self.handler(element, "changed", &[time.into()])?;
            return Ok(true);
        }
        if property(element).is_none() {
            return Ok(false);
        }
        Ok(match (self.picker_state(element).as_ref(), input, tail) {
            (State::Date(picker), Input::Control(action, route), [0])
                if picker.accepts_input(node, route) =>
            {
                let bounds = self.date_bounds(element)?;
                route.is_current() && picker.edit(bounds, node, action)
            }
            (State::Time(picker), Input::Control(action, route), [0, slot])
                if picker.accepts_input(node, route) =>
            {
                let options = self.time_options(element)?;
                route.is_current() && picker.edit(options, *slot, node, action)
            }
            (State::Font(picker), Input::Control(action, route), [0, ..]) => {
                route.is_current() && picker.edit(node, action)
            }
            (State::Color(picker), Input::Control(ControlAction::Spin(action), route), [0, ..])
                if picker.accepts_input(node, route) =>
            {
                let show_alpha = self.show_alpha(element)?;
                route.is_current() && picker.spin(show_alpha, node, action)
            }
            (State::Color(picker), Input::Range(action, route), [0, 5, ..]) => {
                picker.accepts_input(node, route)
                    && self.binding_value(element, "show-color-plane")? == Some(Value::Bool(true))
                    && route.is_current()
                    && picker.adjust_plane(node, action)
            }
            (State::Color(picker), Input::Range(action, route), [0, ..])
                if picker.accepts_input(node, route) =>
            {
                let show_alpha = self.show_alpha(element)?;
                route.is_current() && picker.slide(show_alpha, node, action)
            }
            _ => false,
        })
    }

    pub(crate) fn edit_picker_hex(
        &self,
        element: &Element,
        tail: &[usize],
        node: &Node,
        text: &str,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        if tail != [0, 0, 1] || !matches!(element.visual, Visual::Builtin("ColorPickerPopup")) {
            return Ok(false);
        }
        let state = self.picker_state(element);
        let State::Color(picker) = state.as_ref() else {
            unreachable!("color picker state")
        };
        if !picker.accepts_input(node, input) {
            return Ok(false);
        }
        let show_alpha = self.show_alpha(element)?;
        Ok(input.is_current() && picker.edit_hex(show_alpha, node, text.into()))
    }
}
