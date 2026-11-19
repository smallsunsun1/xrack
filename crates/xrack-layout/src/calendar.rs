use crate::{LayoutNode, Point, Rect, place_in_direction};
use xrack_runtime::{
    CalendarAction, CalendarHighlight, CalendarHighlightDay, CalendarState, Date, LayoutDirection,
    Node,
};

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalendarPart {
    Calendar,
    MonthGrid,
    DayOfWeekRow,
    WeekNumberColumn,
}

impl CalendarPart {
    fn cell(
        self,
        bounds: Rect,
        column: usize,
        row: usize,
        span: usize,
        direction: LayoutDirection,
    ) -> Rect {
        let (columns, rows) = match self {
            Self::Calendar => (7.0, 8.0),
            Self::MonthGrid => (7.0, 6.0),
            Self::DayOfWeekRow => (7.0, 1.0),
            Self::WeekNumberColumn => (1.0, 6.0),
        };
        let width = bounds.width / columns;
        let height = bounds.height / rows;
        let rect = Rect::new(
            bounds.x + width * column as f32,
            bounds.y + height * row as f32,
            width * span as f32,
            height,
        );
        place_in_direction(rect, bounds, direction)
    }
}

pub(crate) fn month_cell(bounds: Rect, slot: usize, direction: LayoutDirection) -> Rect {
    CalendarPart::MonthGrid.cell(bounds, slot % 7, slot / 7, 1, direction)
}

pub struct CalendarGeometry {
    bounds: Rect,
    part: CalendarPart,
    pub state: CalendarState,
    pub layout_direction: LayoutDirection,
    cells: [Option<Date>; 42],
    week_start: usize,
    today: Option<Date>,
    highlight: CalendarHighlight,
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
    pub highlight: CalendarHighlightDay,
}

#[derive(Clone, Copy)]
pub struct CalendarWeek {
    pub year: i64,
    pub number: i64,
    pub bounds: Rect,
}

impl CalendarGeometry {
    pub fn new(node: &Node, bounds: Rect) -> Self {
        Self::with_layout_direction(node, bounds, node.layout_direction().unwrap_or_default())
    }

    pub fn for_layout(node: &Node, layout: &LayoutNode) -> Self {
        Self::with_layout_direction(node, layout.bounds, layout.layout_direction)
    }

    fn with_layout_direction(node: &Node, bounds: Rect, direction: LayoutDirection) -> Self {
        let state = CalendarState::from_node(node);
        let week_start = node.int("week-start").unwrap_or(1).clamp(1, 7);
        let part = match node.kind {
            "MonthGrid" => CalendarPart::MonthGrid,
            "DayOfWeekRow" => CalendarPart::DayOfWeekRow,
            "WeekNumberColumn" => CalendarPart::WeekNumberColumn,
            _ => CalendarPart::Calendar,
        };
        let cells = if part == CalendarPart::DayOfWeekRow {
            [None; 42]
        } else {
            state.cells(week_start)
        };
        let highlight = CalendarHighlight::from_node(node, state.fixed_month);
        Self {
            bounds,
            part,
            state,
            layout_direction: direction,
            cells,
            week_start: week_start as usize - 1,
            today: node.date("today"),
            highlight,
        }
    }

    fn cell(&self, column: usize, row: usize, span: usize) -> Rect {
        self.part
            .cell(self.bounds, column, row, span, self.layout_direction)
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
                let in_month = self.state.month == date.first_of_month();
                Some(CalendarDay {
                    date,
                    slot: index,
                    bounds: if self.part == CalendarPart::MonthGrid {
                        month_cell(self.bounds, index, self.layout_direction)
                    } else {
                        self.cell(index % 7, 2 + index / 7, 1)
                    },
                    enabled: self.state.allows(date),
                    selected: self.state.selected == date && self.state.allows(date),
                    in_month,
                    highlight: self.highlight.day(date, in_month),
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
        if self.has_header() {
            let preferred = self.state.cursor();
            let mut prior = None;
            for day in self.days().filter(|day| day.in_month && day.enabled) {
                if day.date >= preferred {
                    return Some(day.date);
                }
                prior = Some(day.date);
            }
            return prior;
        }
        if let Some(date) = self
            .selection_in_view()
            .filter(|&date| self.state.allows(date))
        {
            return Some(date);
        }
        self.days()
            .find(|day| day.enabled && day.in_month)
            .or_else(|| self.days().find(|day| day.enabled))
            .map(|day| day.date)
    }

    pub(crate) fn navigation_origin(&self) -> Option<Date> {
        // A live exclusion leaves the draft in place. Start directional movement
        // there so the selectable focus fallback isn't skipped a second time.
        self.selection_in_view().or_else(|| self.cursor())
    }

    fn selection_in_view(&self) -> Option<Date> {
        let date = self.state.selected;
        let current_month_only = self.has_header() || self.state.fixed_month;
        (self.cells.contains(&Some(date))
            && (!current_month_only || date.first_of_month() == self.state.month))
            .then_some(date)
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
