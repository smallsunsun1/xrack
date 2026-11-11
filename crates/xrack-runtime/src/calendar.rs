use crate::{
    ControlAction, Node, Property,
    picker::{PickerSession, append_picker_dialog},
};
use std::{cell::RefCell, collections::BTreeMap, fmt};

/// A Gregorian civil date. Validation is independent of time zones and clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl Default for Date {
    fn default() -> Self {
        Self {
            year: 1970,
            month: 1,
            day: 1,
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Date {
    pub const MIN: Self = Self {
        year: 1,
        month: 1,
        day: 1,
    };
    pub const MAX: Self = Self {
        year: 9999,
        month: 12,
        day: 31,
    };

    pub fn new(year: i64, month: i64, day: i64) -> Option<Self> {
        let date = Self { year, month, day };
        date.valid().then_some(date)
    }

    pub fn valid(self) -> bool {
        (1..=9999).contains(&self.year) && (1..=self.month_length()).contains(&self.day)
    }

    pub fn first_of_month(self) -> Self {
        Self { day: 1, ..self }
    }

    pub fn last_of_month(self) -> Self {
        Self {
            day: self.month_length(),
            ..self
        }
    }

    fn month_length(self) -> i64 {
        match self.month {
            4 | 6 | 9 | 11 => 30,
            2 => {
                28 + i64::from(self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0))
            }
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        }
    }

    fn days_before_year(year: i64) -> i64 {
        let prior = year - 1;
        prior * 365 + prior / 4 - prior / 100 + prior / 400
    }

    fn ordinal(self) -> i64 {
        Self::days_before_year(self.year) + self.day - 1
            + (1..self.month)
                .map(|month| Self { month, ..self }.month_length())
                .sum::<i64>()
    }

    pub fn add_days(self, days: i64) -> Option<Self> {
        if !self.valid() {
            return None;
        }
        let ordinal = self.ordinal().checked_add(days)?;
        if !(0..Self::days_before_year(10_000)).contains(&ordinal) {
            return None;
        }
        let (mut year, mut end) = (1, 10_000);
        while end - year > 1 {
            let middle = (year + end) / 2;
            if Self::days_before_year(middle) <= ordinal {
                year = middle;
            } else {
                end = middle;
            }
        }
        let mut day = ordinal - Self::days_before_year(year) + 1;
        for month in 1..=12 {
            let date = Self { year, month, day };
            let count = date.month_length();
            if day <= count {
                return Some(date);
            }
            day -= count;
        }
        None
    }

    /// Return the first day of the month at the given offset.
    pub fn add_months(self, months: i64) -> Option<Self> {
        if !self.valid() {
            return None;
        }
        let index = ((self.year - 1) * 12 + self.month - 1).checked_add(months)?;
        Self::new(index.div_euclid(12) + 1, index.rem_euclid(12) + 1, 1)
    }

    /// ISO 8601 week-based year and week number.
    pub fn iso_week(self) -> Option<(i64, i64)> {
        if !self.valid() {
            return None;
        }
        let ordinal = self.ordinal();
        let thursday = ordinal + 3 - ordinal.rem_euclid(7);
        let year = if thursday < Self::days_before_year(self.year) {
            self.year - 1
        } else if thursday >= Self::days_before_year(self.year + 1) {
            self.year + 1
        } else {
            self.year
        };
        Some((year, (thursday - Self::days_before_year(year)) / 7 + 1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarAction {
    Select(Date),
    Browse(Date),
    Activate(Date),
}

pub fn is_calendar_control(kind: &str) -> bool {
    matches!(kind, "Calendar" | "MonthGrid")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarState {
    pub minimum: Date,
    pub maximum: Date,
    pub selected: Date,
    /// First day of the displayed month.
    pub month: Date,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct DateBounds {
    minimum: Date,
    maximum: Date,
}

impl DateBounds {
    pub fn new(minimum: Date, maximum: Date) -> Self {
        let minimum = if minimum.valid() { minimum } else { Date::MIN };
        let maximum = if maximum.valid() { maximum } else { Date::MAX };
        Self {
            minimum,
            maximum: maximum.max(minimum),
        }
    }

    fn from_node(node: &Node) -> Self {
        Self::new(
            node.date("minimum-date").unwrap_or(Date::MIN),
            node.date("maximum-date").unwrap_or(Date::MAX),
        )
    }

    fn constrain(self, date: Date) -> Date {
        let date = if date.valid() { date } else { Date::default() };
        date.clamp(self.minimum, self.maximum)
    }
}

impl CalendarState {
    pub fn from_node(node: &Node) -> Self {
        let date = node.date("date").unwrap_or_default();
        let month = node.date("display-month");
        let bounds = DateBounds::from_node(node);
        if matches!(node.kind, "MonthGrid" | "WeekNumberColumn") {
            Self::for_month_grid(date, month.unwrap_or(date), bounds)
        } else {
            Self::with_bounds(date, month, bounds)
        }
    }

    #[doc(hidden)]
    pub fn for_month_grid(date: Date, month: Date, bounds: DateBounds) -> Self {
        let month = if month.valid() {
            month
        } else {
            Date::default()
        };
        Self {
            minimum: bounds.minimum,
            maximum: bounds.maximum,
            selected: bounds.constrain(date),
            month: month.first_of_month(),
        }
    }

    fn with_bounds(date: Date, month: Option<Date>, bounds: DateBounds) -> Self {
        let selected = bounds.constrain(date);
        let month = bounds
            .constrain(month.filter(|date| date.valid()).unwrap_or(selected))
            .first_of_month();
        Self {
            minimum: bounds.minimum,
            maximum: bounds.maximum,
            selected,
            month,
        }
    }

    pub fn allows(self, date: Date) -> bool {
        date.valid() && (self.minimum..=self.maximum).contains(&date)
    }

    pub fn allows_month(self, month: Date) -> bool {
        month.valid()
            && month.day == 1
            && month <= self.maximum
            && month.last_of_month() >= self.minimum
    }

    pub fn adjacent_month(self, months: i64) -> Option<Date> {
        self.month
            .add_months(months)
            .filter(|&date| self.allows_month(date))
    }

    pub fn cursor(self) -> Date {
        if self.selected.first_of_month() == self.month {
            self.selected
        } else {
            self.month.max(self.minimum)
        }
    }

    /// Six complete weeks starting on Monday (1) through Sunday (7).
    pub fn cells(self, week_start: i64) -> [Option<Date>; 42] {
        if !self.month.valid() {
            return [None; 42];
        }
        let offset = self.grid_offset(week_start);
        std::array::from_fn(|index| self.month.add_days(index as i64 - offset))
    }

    pub fn shows(self, date: Date, week_start: i64) -> bool {
        date.valid()
            && self.month.valid()
            && (0..42)
                .contains(&(date.ordinal() - self.month.ordinal() + self.grid_offset(week_start)))
    }

    fn grid_offset(self, week_start: i64) -> i64 {
        (self.month.ordinal() - (week_start.clamp(1, 7) - 1)).rem_euclid(7)
    }
}

/// Inputs to a visual template for one date in a MonthGrid.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarCell {
    pub date: Date,
    pub index: i64,
    pub weekday: i64,
    pub week: i64,
    pub week_year: i64,
    pub selected: bool,
    pub today: bool,
    pub in_month: bool,
    pub enabled: bool,
}

#[derive(Default)]
struct CachedCalendarCell {
    input: Property<CalendarCell>,
    scene: Property<Node>,
}

/// Cache the visual bindings of each displayed date independently.
#[doc(hidden)]
#[derive(Default)]
pub struct CalendarCells(RefCell<BTreeMap<Date, CachedCalendarCell>>);

impl CalendarCells {
    pub fn render(&self, grid: &Node, render_cell: impl Fn(CalendarCell) -> Node) -> Node {
        let state = CalendarState::from_node(grid);
        let start = grid.int("week-start").unwrap_or(1).clamp(1, 7);
        let today = grid.date("today");
        let dates = state.cells(start);
        let mut cache = self.0.borrow_mut();
        cache.retain(|date, _| dates.contains(&Some(*date)));
        let mut cells = Node::new("CalendarCells", None);
        cells.children = dates
            .into_iter()
            .enumerate()
            .map(|(index, date)| {
                let Some(date) = date else {
                    return Node::hidden();
                };
                let (week_year, week) =
                    date.iso_week().expect("calendar cells contain valid dates");
                let input = CalendarCell {
                    date,
                    index: index as i64,
                    weekday: (start - 1 + index as i64) % 7 + 1,
                    week,
                    week_year,
                    selected: date == state.selected,
                    today: Some(date) == today,
                    in_month: date.first_of_month() == state.month,
                    enabled: state.allows(date),
                };
                let cached = cache.entry(date).or_default();
                cached.input.set_if_changed(input);
                cached.scene.get(|| render_cell(cached.input.get(|| input)))
            })
            .collect();
        cells
    }
}

/// Draft date and browsed month share one session without changing the committed date.
#[doc(hidden)]
#[derive(Default)]
pub struct DatePopupState(PickerSession<(Date, Date)>);

impl DatePopupState {
    pub fn close(&self) {
        self.0.close();
    }

    pub fn populate(&self, dialog: &mut Node, date: impl FnOnce() -> Date) {
        let bounds = DateBounds::from_node(dialog);
        let Some((date, month)) = self.0.begin(dialog, || {
            let date = bounds.constrain(date());
            (date, date.first_of_month())
        }) else {
            return;
        };
        let state = CalendarState::with_bounds(date, Some(month), bounds);
        let mut calendar = Node::new("Calendar", None);
        for key in [
            "week-start",
            "weekday-labels",
            "font",
            "font-size",
            "font-family",
            "font-weight",
            "font-italic",
        ] {
            if let Some(value) = dialog.properties.get(key) {
                calendar.properties.insert(key, value.clone());
            }
        }
        calendar.properties.insert("date", state.selected.into());
        calendar
            .properties
            .insert("display-month", state.month.into());
        calendar
            .properties
            .insert("minimum-date", state.minimum.into());
        calendar
            .properties
            .insert("maximum-date", state.maximum.into());
        self.0.stamp(&mut calendar);
        append_picker_dialog(dialog, calendar);
    }

    pub fn edit(&self, bounds: DateBounds, node: &Node, action: ControlAction) -> bool {
        let (Some((date, month)), ControlAction::Calendar(action)) = (self.0.current(node), action)
        else {
            return false;
        };
        let state = CalendarState::with_bounds(date, Some(month), bounds);
        if node.kind != "Calendar" || CalendarState::from_node(node) != state {
            return false;
        }
        let next = match action {
            CalendarAction::Select(date) if state.allows(date) => (date, date.first_of_month()),
            CalendarAction::Browse(month) if state.allows_month(month) => (state.selected, month),
            _ => return false,
        };
        if next == (state.selected, state.month) {
            return false;
        }
        self.0.set(next);
        true
    }

    pub fn finish(&self, bounds: DateBounds, dialog: &Node) -> Option<Date> {
        self.0
            .finish(dialog)
            .map(|(date, _)| bounds.constrain(date))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gregorian_arithmetic_handles_leap_centuries_and_calendar_limits() {
        let date = |year, month, day| Date::new(year, month, day).unwrap();
        assert_eq!(date(1900, 2, 28).add_days(1), Some(date(1900, 3, 1)));
        assert_eq!(date(2000, 2, 28).add_days(1), Some(date(2000, 2, 29)));
        assert_eq!(date(2025, 1, 1).add_days(-1), Some(date(2024, 12, 31)));
        assert_eq!(
            date(1970, 1, 1).add_days(20_000).unwrap().add_days(-20_000),
            Some(Date::default())
        );
        assert_eq!(Date::MIN.add_days(-1), None);
        assert_eq!(Date::MAX.add_days(1), None);
        assert_eq!(Date::MIN.add_days(i64::MAX), None);
    }

    #[test]
    fn calendar_template_only_renders_cells_whose_inputs_changed() {
        use std::cell::Cell;
        let date = |day| Date::new(2026, 11, day).unwrap();
        let evaluations = Cell::new(0);
        let note = Property::default();
        let cells = CalendarCells::default();
        let mut grid = Node::new("MonthGrid", None);
        grid.properties.insert("display-month", date(1).into());
        grid.properties.insert("date", date(11).into());
        let render = |grid: &Node| {
            cells.render(grid, |day| {
                evaluations.set(evaluations.get() + 1);
                let mut cell = Node::new("Rectangle", None);
                if day.date == date(12) {
                    cell.properties
                        .insert("accessible-description", note.get(String::new).into());
                }
                cell
            })
        };
        assert_eq!(render(&grid).children.len(), 42);
        assert_eq!(evaluations.get(), 42);
        grid.properties.insert("date", date(12).into());
        render(&grid);
        assert_eq!(evaluations.get(), 44);
        note.set_if_changed("Review".to_owned());
        render(&grid);
        assert_eq!(evaluations.get(), 45);
    }

    #[test]
    fn iso_weeks_use_the_thursday_at_new_year() {
        let week = |year, month, day| Date::new(year, month, day).unwrap().iso_week().unwrap();
        assert_eq!(week(2018, 12, 31), (2019, 1));
        assert_eq!(week(2021, 1, 1), (2020, 53));
        assert_eq!(week(2021, 1, 3), (2020, 53));
        assert_eq!(week(2021, 1, 4), (2021, 1));
        assert_eq!(Date::MIN.iso_week(), Some((1, 1)));
        assert_eq!(Date::MAX.iso_week(), Some((9999, 52)));
    }

    #[test]
    fn date_bounds_disable_cells_and_week_start_changes_the_first_column() {
        let mut node = Node::new("Calendar", None);
        let date = |month, day| Date::new(2024, month, day).unwrap();
        node.properties.insert("date", date(2, 29).into());
        node.properties.insert("minimum-date", date(2, 20).into());
        node.properties.insert("maximum-date", date(3, 5).into());
        let calendar = CalendarState::from_node(&node);
        assert_eq!(calendar.cells(1)[0], Some(date(1, 29)));
        assert_eq!(calendar.cells(7)[0], Some(date(1, 28)));
        assert!(calendar.cells(1).contains(&Some(date(2, 29))));
        assert!(!calendar.allows(date(2, 19)));
        assert!(calendar.allows(date(3, 5)));
        assert_eq!(calendar.adjacent_month(-1), None);
        assert_eq!(calendar.adjacent_month(1), Some(date(3, 1)));
    }
}
