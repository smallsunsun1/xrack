use crate::{Point, Rect};
use xrack_runtime::{CalendarAction, CalendarState, Date, Node};

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalendarPart {
    Calendar,
    MonthGrid,
    DayOfWeekRow,
    WeekNumberColumn,
}

impl CalendarPart {
    fn cell(self, bounds: Rect, column: usize, row: usize, span: usize) -> Rect {
        let (columns, rows) = match self {
            Self::Calendar => (7.0, 8.0),
            Self::MonthGrid => (7.0, 6.0),
            Self::DayOfWeekRow => (7.0, 1.0),
            Self::WeekNumberColumn => (1.0, 6.0),
        };
        let width = bounds.width / columns;
        let height = bounds.height / rows;
        Rect::new(
            bounds.x + width * column as f32,
            bounds.y + height * row as f32,
            width * span as f32,
            height,
        )
    }
}

pub(crate) fn month_cell(bounds: Rect, slot: usize) -> Rect {
    CalendarPart::MonthGrid.cell(bounds, slot % 7, slot / 7, 1)
}

pub struct CalendarGeometry {
    bounds: Rect,
    part: CalendarPart,
    pub state: CalendarState,
    cells: [Option<Date>; 42],
    week_start: usize,
    today: Option<Date>,
}

#[derive(Clone, Copy)]
pub struct CalendarDay {
    pub date: Date,
    pub slot: usize,
    pub bounds: Rect,
    pub enabled: bool,
    pub selected: bool,
    pub in_month: bool,
    pub today: bool,
}

#[derive(Clone, Copy)]
pub struct CalendarWeek {
    pub year: i64,
    pub number: i64,
    pub bounds: Rect,
}

impl CalendarGeometry {
    pub fn new(node: &Node, bounds: Rect) -> Self {
        let state = CalendarState::from_node(node);
        let week_start = node.int("week-start").unwrap_or(1).clamp(1, 7);
        let part = match node.kind {
            "MonthGrid" => CalendarPart::MonthGrid,
            "DayOfWeekRow" => CalendarPart::DayOfWeekRow,
            "WeekNumberColumn" => CalendarPart::WeekNumberColumn,
            _ => CalendarPart::Calendar,
        };
        Self {
            bounds,
            part,
            state,
            cells: if part == CalendarPart::DayOfWeekRow {
                [None; 42]
            } else {
                state.cells(week_start)
            },
            week_start: week_start as usize - 1,
            today: node.date("today"),
        }
    }

    fn cell(&self, column: usize, row: usize, span: usize) -> Rect {
        self.part.cell(self.bounds, column, row, span)
    }

    pub fn has_header(&self) -> bool {
        self.part == CalendarPart::Calendar
    }

    pub fn title(&self) -> Rect {
        self.cell(1, 0, 5)
    }

    pub fn navigation(&self) -> [(Rect, Option<CalendarAction>); 2] {
        if !self.has_header() {
            return [(Rect::default(), None); 2];
        }
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
        let count = if matches!(
            self.part,
            CalendarPart::Calendar | CalendarPart::DayOfWeekRow
        ) {
            7
        } else {
            0
        };
        (0..count).map(move |column| {
            let day = (self.week_start + column) % 7;
            let label = labels.get(day).map_or(WEEKDAYS[day], String::as_str);
            (self.cell(column, usize::from(self.has_header()), 1), label)
        })
    }

    pub fn days(&self) -> impl Iterator<Item = CalendarDay> + '_ {
        let count = if matches!(self.part, CalendarPart::Calendar | CalendarPart::MonthGrid) {
            42
        } else {
            0
        };
        self.cells[..count]
            .iter()
            .enumerate()
            .filter_map(|(index, &date)| {
                let date = date?;
                Some(CalendarDay {
                    date,
                    slot: index,
                    bounds: self.cell(index % 7, 2 * usize::from(self.has_header()) + index / 7, 1),
                    enabled: self.state.allows(date),
                    selected: self.state.selected == date,
                    in_month: self.state.month == date.first_of_month(),
                    today: self.today == Some(date),
                })
            })
    }

    pub fn weeks(&self) -> impl Iterator<Item = CalendarWeek> + '_ {
        let count = if self.part == CalendarPart::WeekNumberColumn {
            42
        } else {
            0
        };
        let thursday = (10 - self.week_start) % 7;
        self.cells[..count]
            .chunks_exact(7)
            .enumerate()
            .filter_map(move |(row, week)| {
                // ISO week numbers follow the Thursday of the displayed row even
                // when the display starts the week on a day other than Monday.
                let date = week[thursday].or_else(|| week.iter().flatten().next().copied())?;
                let (year, number) = date.iso_week()?;
                Some(CalendarWeek {
                    year,
                    number,
                    bounds: self.cell(0, row, 1),
                })
            })
    }

    pub fn cursor(&self) -> Option<Date> {
        if self.has_header() || self.cells.contains(&Some(self.state.selected)) {
            return Some(if self.has_header() {
                self.state.cursor()
            } else {
                self.state.selected
            });
        }
        self.days()
            .find(|day| day.enabled && day.in_month)
            .or_else(|| self.days().find(|day| day.enabled))
            .map(|day| day.date)
    }

    pub fn day_action(&self, date: Date) -> CalendarAction {
        if self.part == CalendarPart::MonthGrid {
            CalendarAction::Activate(date)
        } else {
            CalendarAction::Select(date)
        }
    }

    pub fn action_at(&self, point: Point) -> Option<CalendarAction> {
        for (bounds, action) in self.navigation() {
            if bounds.contains(point) {
                return action;
            }
        }
        self.days()
            .find(|day| day.enabled && day.bounds.contains(point))
            .map(|day| self.day_action(day.date))
    }

    pub fn action_bounds(&self, action: CalendarAction) -> Option<Rect> {
        match action {
            CalendarAction::Select(date) | CalendarAction::Activate(date)
                if action == self.day_action(date) =>
            {
                self.days()
                    .find(|day| day.date == date && day.enabled)
                    .map(|day| day.bounds)
            }
            CalendarAction::Browse(_) => self
                .navigation()
                .into_iter()
                .find_map(|(rect, item)| (item == Some(action)).then_some(rect)),
            _ => None,
        }
    }
}
