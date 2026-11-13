use crate::{Element, Error, Instance, Result};
use xrack_runtime::{
    CalendarAction, CalendarCell, CalendarCells, CalendarUpdate, ControlAction, Node, Property,
    Tumbler, TumblerItems, Value,
};

pub(crate) enum State {
    Cells(CalendarCells),
    Tumbler(Property<Option<TumblerItems>>),
}

#[derive(Clone, Copy)]
pub(crate) enum Parameter {
    Date,
    Index,
    Weekday,
    Week,
    WeekYear,
    Selected,
    Today,
    InMonth,
    Enabled,
}

impl Parameter {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "cell-date" => Self::Date,
            "cell-index" => Self::Index,
            "cell-weekday" => Self::Weekday,
            "cell-week" => Self::Week,
            "cell-week-year" => Self::WeekYear,
            "cell-selected" => Self::Selected,
            "cell-today" => Self::Today,
            "cell-in-month" => Self::InMonth,
            "cell-enabled" => Self::Enabled,
            _ => return None,
        })
    }

    pub(crate) fn read(self, cell: CalendarCell) -> Value {
        match self {
            Self::Date => cell.date.into(),
            Self::Index => cell.index.into(),
            Self::Weekday => cell.weekday.into(),
            Self::Week => cell.week.into(),
            Self::WeekYear => cell.week_year.into(),
            Self::Selected => cell.selected.into(),
            Self::Today => cell.today.into(),
            Self::InMonth => cell.in_month.into(),
            Self::Enabled => cell.enabled.into(),
        }
    }
}

impl Instance {
    pub(crate) fn populate_calendar_cells(
        &self,
        element: &Element,
        template: &Element,
        node: &mut Node,
    ) -> Result<()> {
        let state = self.calendar_state(element);
        let State::Cells(cells) = state.as_ref() else {
            unreachable!("calendar cell state")
        };
        let rendered = cells.try_render(node, |cell| self.render_calendar_cell(template, cell))?;
        node.children.push(rendered);
        Ok(())
    }

    pub(crate) fn calendar_action(
        &self,
        element: &Element,
        node: &Node,
        action: CalendarAction,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        if !xrack_runtime::try_calendar_input_is_current(
            node,
            input,
            |name, default| {
                let value = match name {
                    "date" | "display-month" => self.control_value(element, name)?,
                    _ => self.binding_value(element, name)?,
                };
                match value {
                    Some(Value::Date(date)) => Ok(date),
                    None => Ok(default),
                    _ => Err(Error::Evaluation(format!("expected a date for {name}"))),
                }
            },
            || match self.binding_value(element, "week-start")? {
                Some(Value::Int(value)) => Ok(value),
                None => Ok(1),
                _ => Err(Error::Evaluation(
                    "expected an integer for week-start".into(),
                )),
            },
        )? {
            return Ok(false);
        }
        xrack_runtime::try_apply_calendar_action(node, action, |update| {
            match update {
                CalendarUpdate::SetDate(date) => {
                    self.set_control_value(element, "date", date.into())?
                }
                CalendarUpdate::SetMonth(month) => {
                    self.set_control_value(element, "display-month", month.into())?
                }
                CalendarUpdate::DateChanged(date) => {
                    self.handler(element, "changed", &[date.into()])?
                }
                CalendarUpdate::MonthChanged(month) => {
                    self.handler(element, "month-changed", &[month.into()])?
                }
                CalendarUpdate::Activated(date) => {
                    self.handler(element, "activated", &[date.into()])?
                }
            };
            Ok(())
        })
    }

    pub(crate) fn tumbler_items(&self, element: &Element) -> Result<Option<TumblerItems>> {
        let state = self.calendar_state(element);
        let State::Tumbler(items) = state.as_ref() else {
            unreachable!("tumbler state")
        };
        items.try_get(|| match self.binding_value(element, "model")? {
            Some(Value::StringModel(model)) => Ok(Some(TumblerItems::new(model))),
            None => Ok(None),
            _ => Err(Error::Evaluation("expected a tumbler string model".into())),
        })
    }

    pub(crate) fn tumbler_action(
        &self,
        element: &Element,
        node: &Node,
        action: ControlAction,
        input: xrack_runtime::InputRoute<'_>,
    ) -> Result<bool> {
        let Some(index) = Tumbler::try_prepare_selection(
            node,
            action,
            || self.element_allows_editing(element),
            || self.tumbler_items(element),
            || match self.binding_value(element, "count")? {
                Some(Value::Int(count)) => Ok(count),
                None => Ok(0),
                _ => Err(Error::Evaluation("expected a tumbler count".into())),
            },
            || self.current_index(element),
        )?
        else {
            return Ok(false);
        };
        if !input.is_current() || !self.set_control_value(element, "current-index", index.into())? {
            return Ok(false);
        }
        self.handler(element, "changed", &[index.into()])?;
        Ok(true)
    }
}
