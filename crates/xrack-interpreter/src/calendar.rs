use crate::{Element, Error, Instance, Result};
use xrack_runtime::{
    CalendarAction, CalendarCell, CalendarCells, CalendarState, ControlAction, Node, Property,
    Tumbler, TumblerItems, Value,
};

pub(crate) enum State {
    Cells(CalendarCells),
    Tumbler(Property<Result<Option<TumblerItems>>>),
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
    ) -> Result<bool> {
        let calendar = CalendarState::from_node(node);
        let week_start = (node.kind == "MonthGrid").then(|| node.int("week-start").unwrap_or(1));
        let Some((date, month)) = calendar.apply(action, week_start) else {
            return Ok(false);
        };
        let date = date.filter(|date| node.date("date") != Some(*date));
        let month_changed = month != calendar.month;
        if let Some(date) = date {
            self.set_control_value(element, "date", date.into())?;
        }
        if month_changed {
            self.set_control_value(element, "display-month", month.into())?;
        }
        if let Some(date) = date {
            self.handler(element, "changed", &[date.into()])?;
        }
        if month_changed {
            self.handler(element, "month-changed", &[month.into()])?;
        }
        if let CalendarAction::Activate(date) = action {
            self.handler(element, "activated", &[date.into()])?;
            return Ok(true);
        }
        Ok(date.is_some() || month_changed)
    }

    pub(crate) fn tumbler_items(&self, element: &Element) -> Result<Option<TumblerItems>> {
        let state = self.calendar_state(element);
        let State::Tumbler(items) = state.as_ref() else {
            unreachable!("tumbler state")
        };
        items.get(|| match self.binding_value(element, "model")? {
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
    ) -> Result<bool> {
        let ControlAction::PickTumbler { index, row } = action else {
            return Ok(false);
        };
        let index_value = index as i64;
        if self.tumbler_items(element)? != node.tumbler_items
            || Tumbler::from_node(node).row_key(index) != Some(row)
            || node.int("current-index") == Some(index_value)
        {
            return Ok(false);
        }
        self.set_control_value(element, "current-index", index_value.into())?;
        self.handler(element, "changed", &[index_value.into()])?;
        Ok(true)
    }
}
