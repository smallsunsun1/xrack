use crate::{Point, Rect};
use xrack_runtime::{CalendarAction, CalendarState, Date, Node};

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

pub struct CalendarGeometry {
    bounds: Rect,
    pub state: CalendarState,
    cells: [Option<Date>; 42],
    week_start: usize,
}

#[derive(Clone, Copy)]
pub struct CalendarDay {
    pub date: Date,
    pub bounds: Rect,
    pub enabled: bool,
    pub selected: bool,
    pub in_month: bool,
}

impl CalendarGeometry {
    pub fn new(node: &Node, bounds: Rect) -> Self {
        let state = CalendarState::from_node(node);
        let week_start = node.int("week-start").unwrap_or(1).clamp(1, 7);
        Self {
            bounds,
            state,
            cells: state.cells(week_start),
            week_start: week_start as usize - 1,
        }
    }

    fn cell(&self, column: usize, row: usize, span: usize) -> Rect {
        let width = self.bounds.width / 7.0;
        let height = self.bounds.height / 8.0;
        Rect::new(
            self.bounds.x + width * column as f32,
            self.bounds.y + height * row as f32,
            width * span as f32,
            height,
        )
    }

    pub fn title(&self) -> Rect {
        self.cell(1, 0, 5)
    }

    pub fn navigation(&self) -> [(Rect, Option<CalendarAction>); 2] {
        [
            (
                self.cell(0, 0, 1),
                self.state.adjacent_month(-1).map(CalendarAction::Browse),
            ),
            (
                self.cell(6, 0, 1),
                self.state.adjacent_month(1).map(CalendarAction::Browse),
            ),
        ]
    }

    pub fn weekdays<'a>(&'a self, node: &'a Node) -> impl Iterator<Item = (Rect, &'a str)> + 'a {
        let labels = node.strings("weekday-labels");
        (0..7).map(move |column| {
            let day = (self.week_start + column) % 7;
            let label = labels.get(day).map_or(WEEKDAYS[day], String::as_str);
            (self.cell(column, 1, 1), label)
        })
    }

    pub fn days(&self) -> impl Iterator<Item = CalendarDay> + '_ {
        self.cells.iter().enumerate().filter_map(|(index, &date)| {
            let date = date?;
            Some(CalendarDay {
                date,
                bounds: self.cell(index % 7, 2 + index / 7, 1),
                enabled: self.state.allows(date),
                selected: self.state.selected == date,
                in_month: self.state.month == date.first_of_month(),
            })
        })
    }

    pub fn action_at(&self, point: Point) -> Option<CalendarAction> {
        for (bounds, action) in self.navigation() {
            if bounds.contains(point) {
                return action;
            }
        }
        self.days()
            .find(|day| day.enabled && day.bounds.contains(point))
            .map(|day| CalendarAction::Select(day.date))
    }

    pub fn action_bounds(&self, action: CalendarAction) -> Option<Rect> {
        match action {
            CalendarAction::Select(date) => self
                .days()
                .find(|day| day.date == date && day.enabled)
                .map(|day| day.bounds),
            CalendarAction::Browse(_) => self
                .navigation()
                .into_iter()
                .find_map(|(rect, item)| (item == Some(action)).then_some(rect)),
        }
    }
}
