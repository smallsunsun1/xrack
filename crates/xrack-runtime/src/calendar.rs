use crate::{
    ControlAction, DialogRole, InputRoute, Node, Property, Value, VecModel,
    picker::{PickerSession, append_picker_dialog},
};
use std::{cell::RefCell, collections::BTreeMap, fmt, ops::RangeInclusive};

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

    /// Parse an ASCII ISO calendar date (YYYY-MM-DD) in the supported years.
    pub fn parse_iso(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if !matches!(bytes, [_, _, _, _, b'-', _, _, b'-', _, _]) {
            return None;
        }
        let number = |digits: &[u8]| {
            digits.iter().try_fold(0_i64, |value, digit| {
                digit
                    .is_ascii_digit()
                    .then(|| value * 10 + i64::from(digit - b'0'))
            })
        };
        Self::new(
            number(&bytes[..4])?,
            number(&bytes[5..7])?,
            number(&bytes[8..])?,
        )
    }

    /// Check the same date restrictions used by calendars and date pickers.
    pub fn is_selectable(
        self,
        minimum: Self,
        maximum: Self,
        disabled_weekdays: &VecModel<i64>,
        disabled_dates: &VecModel<Self>,
    ) -> bool {
        self.valid()
            && DateBounds::new(minimum, maximum)
                .with_disabled_weekdays(calendar_weekday_mask(disabled_weekdays))
                .with_disabled_dates(disabled_dates.snapshot())
                .allows(self)
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

    fn month_number(self) -> i64 {
        (self.year - 1) * 12 + self.month - 1
    }

    fn from_month_number(number: i64) -> Self {
        Self {
            year: number.div_euclid(12) + 1,
            month: number.rem_euclid(12) + 1,
            day: 1,
        }
    }

    fn month_span(self, to: Self) -> Option<RangeInclusive<i64>> {
        (self.valid() && to.valid() && self <= to).then(|| self.month_number()..=to.month_number())
    }

    /// Return the first day of every month intersecting this inclusive date range.
    pub fn months_through(self, to: Self) -> VecModel<Self> {
        VecModel::from(
            self.month_span(to)
                .into_iter()
                .flatten()
                .map(Self::from_month_number)
                .collect::<Vec<_>>(),
        )
    }

    /// Return the index of the date's month in this range, or -1 if it is absent.
    pub fn month_index(self, to: Self, date: Self) -> i64 {
        if date.valid()
            && let Some(span) = self.month_span(to)
        {
            let month = date.month_number();
            if span.contains(&month) {
                return month - span.start();
            }
        }
        -1
    }

    /// Return the first day of the month at the given offset.
    pub fn add_months(self, months: i64) -> Option<Self> {
        if !self.valid() {
            return None;
        }
        let index = self.month_number().checked_add(months)?;
        Self::new(index.div_euclid(12) + 1, index.rem_euclid(12) + 1, 1)
    }

    /// Shift by whole months, retaining the day or clamping it to the target month's end.
    pub fn add_months_clamped(self, months: i64) -> Option<Self> {
        let month = self.add_months(months)?;
        Some(Self {
            day: self.day.min(month.month_length()),
            ..month
        })
    }

    /// ISO weekday number, Monday (1) through Sunday (7).
    pub fn weekday(self) -> Option<i64> {
        self.valid().then(|| self.ordinal().rem_euclid(7) + 1)
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

/// Apply calendar state before emitting its change notifications.
#[doc(hidden)]
pub enum CalendarUpdate {
    SetDate(Date),
    SetMonth(Date),
    DateChanged(Date),
    MonthChanged(Date),
    Activated(Date),
}

#[doc(hidden)]
pub fn apply_calendar_action(
    node: &Node,
    action: CalendarAction,
    mut update: impl FnMut(CalendarUpdate),
) -> bool {
    try_apply_calendar_action(node, action, |change| {
        update(change);
        Ok::<_, std::convert::Infallible>(())
    })
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_apply_calendar_action<E>(
    node: &Node,
    action: CalendarAction,
    mut update: impl FnMut(CalendarUpdate) -> Result<(), E>,
) -> Result<bool, E> {
    let calendar = CalendarState::from_node(node);
    let week_start = (node.kind == "MonthGrid").then(|| node.int("week-start").unwrap_or(1));
    let Some((date, month)) = calendar.apply(action, week_start) else {
        return Ok(false);
    };
    let date = date.filter(|date| node.date("date") != Some(*date));
    let month_changed = month != calendar.month;
    if let Some(date) = date {
        update(CalendarUpdate::SetDate(date))?;
    }
    if month_changed {
        update(CalendarUpdate::SetMonth(month))?;
    }
    if let Some(date) = date {
        update(CalendarUpdate::DateChanged(date))?;
    }
    if month_changed {
        update(CalendarUpdate::MonthChanged(month))?;
    }
    if let CalendarAction::Activate(date) = action {
        update(CalendarUpdate::Activated(date))?;
        return Ok(true);
    }
    Ok(date.is_some() || month_changed)
}

#[doc(hidden)]
pub fn calendar_weekday_mask(weekdays: &VecModel<i64>) -> u8 {
    weekday_mask(weekdays.snapshot())
}

fn weekday_mask(weekdays: impl IntoIterator<Item = i64>) -> u8 {
    weekdays
        .into_iter()
        .filter(|day| (1..=7).contains(day))
        .fold(0, |mask, day| mask | (1 << (day - 1)))
}

/// Check the calendar bindings that determine how the displayed action is interpreted.
#[doc(hidden)]
pub fn try_calendar_input_is_current<E>(
    node: &Node,
    input: crate::InputRoute<'_>,
    mut read: impl FnMut(&str, Date) -> Result<Date, E>,
    week_start: impl FnOnce() -> Result<i64, E>,
    disabled_weekdays: impl FnOnce() -> Result<u8, E>,
    disabled_dates: impl FnOnce() -> Result<Vec<Date>, E>,
    fixed_month: impl FnOnce() -> Result<bool, E>,
) -> Result<bool, E> {
    let selected = node.date("date").unwrap_or_default();
    input.try_available(|| {
        let mut exclusions = DateExclusions {
            weekdays: disabled_weekdays()?,
            dates: disabled_dates()?,
        };
        exclusions.normalize();
        if exclusions != DateBounds::from_node(node).exclusions || !input.is_current() {
            return Ok(false);
        }
        if node.kind == "MonthGrid" {
            let mut read_current = |name, default| {
                let date = read(name, default)?;
                Ok(input.is_current().then_some(date))
            };
            let Some(minimum) = read_current("minimum-date", Date::MIN)? else {
                return Ok(false);
            };
            let Some(maximum) = read_current("maximum-date", Date::MAX)? else {
                return Ok(false);
            };
            let Some(date) = read_current("date", Date::default())? else {
                return Ok(false);
            };
            let Some(month) = read_current("display-month", date)? else {
                return Ok(false);
            };
            let bounds = DateBounds {
                exclusions,
                ..DateBounds::new(minimum, maximum)
            };
            let live = CalendarState::for_month_grid(date, month, bounds, fixed_month()?);
            return Ok(live == CalendarState::from_node(node)
                && week_start()? == node.int("week-start").unwrap_or(1));
        }
        for (property, default) in [
            ("minimum-date", Date::MIN),
            ("maximum-date", Date::MAX),
            ("date", Date::default()),
            ("display-month", selected),
        ] {
            if read(property, default)? != node.date(property).unwrap_or(default)
                || !input.is_current()
            {
                return Ok(false);
            }
        }
        Ok(true)
    })
}

#[doc(hidden)]
pub fn calendar_input_is_current(
    node: &Node,
    input: crate::InputRoute<'_>,
    mut read: impl FnMut(&str, Date) -> Date,
    week_start: impl FnOnce() -> i64,
    disabled_weekdays: impl FnOnce() -> u8,
    disabled_dates: impl FnOnce() -> Vec<Date>,
    fixed_month: impl FnOnce() -> bool,
) -> bool {
    try_calendar_input_is_current(
        node,
        input,
        |name, default| Ok::<_, std::convert::Infallible>(read(name, default)),
        || Ok(week_start()),
        || Ok(disabled_weekdays()),
        || Ok(disabled_dates()),
        || Ok(fixed_month()),
    )
    .unwrap_or_else(|error| match error {})
}

pub fn is_calendar_control(kind: &str) -> bool {
    matches!(kind, "Calendar" | "MonthGrid")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarState {
    pub minimum: Date,
    pub maximum: Date,
    pub selected: Date,
    /// First day of the displayed month.
    pub month: Date,
    /// A MonthGrid whose viewport belongs to a fixed month in a larger view.
    pub fixed_month: bool,
    exclusions: DateExclusions,
}

#[doc(hidden)]
#[derive(Clone)]
pub struct DateBounds {
    minimum: Date,
    maximum: Date,
    exclusions: DateExclusions,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
struct DateExclusions {
    weekdays: u8,
    dates: Vec<Date>,
}

impl DateExclusions {
    fn normalize(&mut self) {
        self.dates.retain(|date| date.valid());
        self.dates.sort_unstable();
        self.dates.dedup();
    }

    fn allows(&self, date: Date) -> bool {
        let weekday_allowed = if self.weekdays == 0 {
            date.valid()
        } else {
            date.weekday()
                .is_some_and(|day| self.weekdays & (1 << (day - 1)) == 0)
        };
        weekday_allowed && self.dates.binary_search(&date).is_err()
    }
}

impl DateBounds {
    pub fn new(minimum: Date, maximum: Date) -> Self {
        let minimum = if minimum.valid() { minimum } else { Date::MIN };
        let maximum = if maximum.valid() { maximum } else { Date::MAX };
        Self {
            minimum,
            maximum: maximum.max(minimum),
            exclusions: DateExclusions::default(),
        }
    }

    pub fn with_disabled_weekdays(mut self, weekdays: u8) -> Self {
        self.exclusions.weekdays = weekdays;
        self
    }

    pub fn with_disabled_dates(mut self, dates: Vec<Date>) -> Self {
        self.exclusions.dates = dates;
        self.exclusions.normalize();
        self
    }

    fn from_node(node: &Node) -> Self {
        Self::new(
            node.date("minimum-date").unwrap_or(Date::MIN),
            node.date("maximum-date").unwrap_or(Date::MAX),
        )
        .with_disabled_weekdays(match node.properties.get("disabled-weekdays") {
            Some(Value::IntList(weekdays)) => weekday_mask(weekdays.iter().copied()),
            _ => 0,
        })
        .with_disabled_dates(match node.properties.get("disabled-dates") {
            Some(Value::DateList(dates)) => dates.clone(),
            _ => Vec::new(),
        })
    }

    fn allows(&self, date: Date) -> bool {
        (self.minimum..=self.maximum).contains(&date) && self.exclusions.allows(date)
    }

    fn allowable_date(&self, date: Date) -> Option<Date> {
        let date = self.constrain(date);
        self.allows(date).then_some(date)
    }

    fn constrain(&self, date: Date) -> Date {
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
            Self::for_month_grid(
                date,
                month.unwrap_or(date),
                bounds,
                node.kind == "MonthGrid" && node.boolean("fixed-month", false),
            )
        } else {
            Self::with_bounds(date, month, bounds)
        }
    }

    #[doc(hidden)]
    pub fn for_month_grid(date: Date, month: Date, bounds: DateBounds, fixed_month: bool) -> Self {
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
            fixed_month,
            exclusions: bounds.exclusions,
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
            fixed_month: false,
            exclusions: bounds.exclusions,
        }
    }

    #[doc(hidden)]
    pub fn apply(
        &self,
        action: CalendarAction,
        grid_week_start: Option<i64>,
    ) -> Option<(Option<Date>, Date)> {
        match action {
            CalendarAction::Select(date) if self.allows(date) => {
                let month = if grid_week_start.is_some_and(|start| self.shows(date, start)) {
                    self.month
                } else {
                    date.first_of_month()
                };
                Some((Some(date), month))
            }
            CalendarAction::Activate(date)
                if self.allows(date)
                    && grid_week_start.is_some_and(|start| self.shows(date, start)) =>
            {
                Some((Some(date), self.month))
            }
            CalendarAction::Browse(_) if self.fixed_month => None,
            CalendarAction::Browse(month) if grid_week_start.is_some() => {
                (month.valid() && month.day == 1).then_some((None, month))
            }
            CalendarAction::Browse(month) if self.allows_month(month) => Some((None, month)),
            _ => None,
        }
    }

    pub fn allows(&self, date: Date) -> bool {
        self.selection_range().contains(&date) && self.exclusions.allows(date)
    }

    fn selection_range(&self) -> RangeInclusive<Date> {
        if self.fixed_month {
            self.minimum.max(self.month)..=self.maximum.min(self.month.last_of_month())
        } else {
            self.minimum..=self.maximum
        }
    }

    /// Find the first selectable day at or beyond the requested calendar position.
    pub fn selectable_from(&self, date: Date, forward: bool) -> Option<Date> {
        if self.exclusions.weekdays == 0x7f {
            return None;
        }
        let direction = if forward { 1 } else { -1 };
        let range = self.selection_range();
        std::iter::successors(Some(date), |day| day.add_days(direction))
            .take_while(|date| range.contains(date))
            .find(|date| self.exclusions.allows(*date))
    }

    pub fn allows_month(&self, month: Date) -> bool {
        month.valid()
            && month.day == 1
            && month <= self.maximum
            && month.last_of_month() >= self.minimum
    }

    pub fn adjacent_month(&self, months: i64) -> Option<Date> {
        self.month
            .add_months(months)
            .filter(|&date| self.allows_month(date))
    }

    pub fn cursor(&self) -> Date {
        if self.selected.first_of_month() == self.month {
            self.selected
        } else {
            self.month.max(self.minimum)
        }
    }

    /// Six complete weeks starting on Monday (1) through Sunday (7).
    pub fn cells(&self, week_start: i64) -> [Option<Date>; 42] {
        if !self.month.valid() {
            return [None; 42];
        }
        let offset = self.grid_offset(week_start);
        std::array::from_fn(|index| self.month.add_days(index as i64 - offset))
    }

    pub fn shows(&self, date: Date, week_start: i64) -> bool {
        date.valid()
            && self.month.valid()
            && (0..42)
                .contains(&(date.ordinal() - self.month.ordinal() + self.grid_offset(week_start)))
    }

    fn grid_offset(&self, week_start: i64) -> i64 {
        (self.month.ordinal() - (week_start.clamp(1, 7) - 1)).rem_euclid(7)
    }
}

/// The visual date interval can be shared without changing how a calendar selects a date.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct CalendarHighlight {
    endpoints: Option<(Date, Date)>,
    fixed_month: bool,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CalendarHighlightDay {
    pub in_range: bool,
    pub start: bool,
    pub end: bool,
}

impl CalendarHighlight {
    pub fn from_node(node: &Node, fixed_month: bool) -> Self {
        let endpoints = node
            .date("highlight-start")
            .zip(node.date("highlight-end"))
            .filter(|(start, end)| start.valid() && end.valid())
            .map(|(start, end)| (start.min(end), start.max(end)));
        Self {
            endpoints,
            fixed_month,
        }
    }

    pub fn day(self, date: Date, in_month: bool) -> CalendarHighlightDay {
        if let Some((start, end)) = self.endpoints
            && (in_month || !self.fixed_month)
            && (start..=end).contains(&date)
        {
            CalendarHighlightDay {
                in_range: true,
                start: date == start,
                end: date == end,
            }
        } else {
            CalendarHighlightDay::default()
        }
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
    pub in_range: bool,
    pub range_start: bool,
    pub range_end: bool,
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
        self.try_render(grid, |cell| {
            Ok::<_, std::convert::Infallible>(render_cell(cell))
        })
        .unwrap_or_else(|error| match error {})
    }

    pub fn try_render<E>(
        &self,
        grid: &Node,
        render_cell: impl Fn(CalendarCell) -> Result<Node, E>,
    ) -> Result<Node, E> {
        let state = CalendarState::from_node(grid);
        let start = grid.int("week-start").unwrap_or(1).clamp(1, 7);
        let today = grid.date("today");
        let dates = state.cells(start);
        let highlight = CalendarHighlight::from_node(grid, state.fixed_month);
        let mut cache = self.0.borrow_mut();
        cache.retain(|date, _| dates.contains(&Some(*date)));
        let mut cells = Node::new("CalendarCells", None);
        cells.children = dates
            .into_iter()
            .enumerate()
            .map(|(index, date)| {
                let Some(date) = date else {
                    return Ok(Node::hidden());
                };
                let Some((week_year, week)) = date.iso_week() else {
                    return Ok(Node::hidden());
                };
                let in_month = date.first_of_month() == state.month;
                let mark = highlight.day(date, in_month);
                let input = CalendarCell {
                    date,
                    index: index as i64,
                    weekday: (start - 1 + index as i64) % 7 + 1,
                    week,
                    week_year,
                    selected: date == state.selected && state.allows(date),
                    today: Some(date) == today,
                    in_month,
                    enabled: state.allows(date),
                    in_range: mark.in_range,
                    range_start: mark.start,
                    range_end: mark.end,
                };
                let cached = cache.entry(date).or_default();
                cached.input.set_if_changed(input);
                cached
                    .scene
                    .try_get(|| render_cell(cached.input.get(|| input)))
            })
            .collect::<Result<_, _>>()?;
        Ok(cells)
    }
}

/// Draft date and browsed month share one session without changing the committed date.
#[doc(hidden)]
#[derive(Default)]
pub struct DatePopupState(PickerSession<(Date, Date)>);

impl DatePopupState {
    #[doc(hidden)]
    pub fn accepts_input(&self, node: &Node, input: InputRoute<'_>) -> bool {
        self.0.accepts_input(node, input)
    }

    pub fn close(&self) {
        self.0.close();
    }

    pub fn populate(&self, dialog: &mut Node, date: impl FnOnce() -> Date) {
        self.try_populate(dialog, || Ok::<_, std::convert::Infallible>(date()))
            .unwrap_or_else(|error| match error {});
    }

    pub fn try_populate<E>(
        &self,
        dialog: &mut Node,
        date: impl FnOnce() -> Result<Date, E>,
    ) -> Result<(), E> {
        let bounds = DateBounds::from_node(dialog);
        let Some((date, month)) = self.0.try_begin(dialog, || {
            let date = bounds.constrain(date()?);
            Ok((date, date.first_of_month()))
        })?
        else {
            return Ok(());
        };
        let state = CalendarState::with_bounds(date, Some(month), bounds);
        let mut calendar = Node::new("Calendar", None);
        for key in [
            "week-start",
            "disabled-weekdays",
            "disabled-dates",
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
        append_picker_dialog(dialog, calendar, state.allows(state.selected));
        Ok(())
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
        let Some((selected, month)) = state.apply(action, None) else {
            return false;
        };
        let next = (selected.unwrap_or(state.selected), month);
        if next == (state.selected, state.month) {
            return false;
        }
        self.0.set(next);
        true
    }

    pub fn try_finish_input<E>(
        &self,
        dialog: &Node,
        role: DialogRole,
        input: InputRoute<'_>,
        bounds: impl FnOnce() -> Result<DateBounds, E>,
    ) -> Result<Option<Date>, E> {
        let draft = self
            .0
            .try_finish_input(dialog, role, input, |(date, month)| {
                Ok(bounds()?.allowable_date(date).map(|date| (date, month)))
            })?;
        Ok(draft.map(|(date, _)| date))
    }

    pub fn finish_input(
        &self,
        dialog: &Node,
        role: DialogRole,
        input: InputRoute<'_>,
        bounds: impl FnOnce() -> DateBounds,
    ) -> Option<Date> {
        self.try_finish_input(dialog, role, input, || {
            Ok::<_, std::convert::Infallible>(bounds())
        })
        .unwrap_or_else(|error| match error {})
    }

    pub fn finish(&self, bounds: DateBounds, dialog: &Node) -> Option<Date> {
        self.finish_with_role(bounds, dialog, DialogRole::Accept)
    }

    pub fn finish_with_role(
        &self,
        bounds: DateBounds,
        dialog: &Node,
        role: DialogRole,
    ) -> Option<Date> {
        if role == DialogRole::Accept && bounds.allowable_date(self.0.current(dialog)?.0).is_none()
        {
            return None;
        }
        self.0
            .finish(dialog, role)
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
    fn weekday_policy_snapshots_models_and_skips_days_within_date_bounds() {
        let date = |day| Date::new(2024, 3, day).unwrap();
        let weekdays = VecModel::from(vec![6, 7, 6, -1, 8]);
        let mut node = Node::new("Calendar", None);
        node.properties.insert("minimum-date", date(1).into());
        node.properties.insert("maximum-date", date(8).into());
        node.properties
            .insert("disabled-weekdays", weekdays.clone().into());
        let old = node.clone();
        let state = CalendarState::from_node(&old);
        assert_eq!(state.selectable_from(date(2), true), Some(date(4)));
        assert_eq!(state.selectable_from(date(3), false), Some(date(1)));
        assert_eq!(state.apply(CalendarAction::Select(date(2)), None), None);
        weekdays.push(1);
        node.properties
            .insert("disabled-weekdays", weekdays.clone().into());
        assert_eq!(
            CalendarState::from_node(&old).selectable_from(date(2), true),
            Some(date(4))
        );
        assert_eq!(
            CalendarState::from_node(&node).selectable_from(date(2), true),
            Some(date(5))
        );
        node.properties
            .insert("disabled-weekdays", Value::IntList((1_i64..=7).collect()));
        assert_eq!(
            CalendarState::from_node(&node).selectable_from(date(1), true),
            None
        );
    }

    #[test]
    fn iso_date_parsing_and_shared_constraints_cover_invalid_dates_and_live_models() {
        let leap_day = Date::new(2000, 2, 29).unwrap();
        assert_eq!(Date::parse_iso("2000-02-29"), Some(leap_day));
        assert_eq!(Date::parse_iso("0001-01-01"), Some(Date::MIN));
        assert_eq!(Date::parse_iso("9999-12-31"), Some(Date::MAX));
        for invalid in [
            "1900-02-29",
            "2024-02-30",
            "0000-01-01",
            "2024-2-29",
            "2024/02/29",
            "2024-02-29 ",
            "２０２４-０２-２９",
            "2024-02-2é",
        ] {
            assert!(Date::parse_iso(invalid).is_none(), "{invalid}");
        }
        let disabled_weekdays = VecModel::from(vec![6_i64, 7]);
        let closed = VecModel::from(vec![leap_day]);
        assert!(!leap_day.is_selectable(Date::MIN, Date::MAX, &disabled_weekdays, &closed));
        closed.clear();
        assert!(leap_day.is_selectable(Date::MIN, Date::MAX, &disabled_weekdays, &closed));
        let saturday = Date::new(2000, 3, 4).unwrap();
        assert!(!saturday.is_selectable(Date::MIN, Date::MAX, &disabled_weekdays, &closed));
        assert!(!leap_day.is_selectable(saturday, Date::MAX, &disabled_weekdays, &closed));
    }

    #[test]
    fn month_ranges_and_indexes_cover_partial_months_without_overflow() {
        let date = |year, month, day| Date::new(year, month, day).unwrap();
        let from = date(2023, 12, 31);
        let to = date(2024, 2, 29);
        assert_eq!(
            from.months_through(to).snapshot(),
            vec![date(2023, 12, 1), date(2024, 1, 1), date(2024, 2, 1)]
        );
        assert_eq!(from.month_index(to, date(2023, 12, 1)), 0);
        assert_eq!(from.month_index(to, to), 2);
        assert_eq!(from.month_index(to, date(2024, 3, 1)), -1);
        assert!(to.months_through(from).is_empty());
        assert_eq!(Date::MIN.month_index(Date::MAX, Date::MAX), 119_987);
        let invalid = Date {
            year: 1900,
            month: 2,
            day: 29,
        };
        assert!(invalid.months_through(to).is_empty());
        assert_eq!(from.month_index(to, invalid), -1);
    }

    #[test]
    fn fixed_month_keeps_adjacent_days_visible_but_unselectable() {
        let mut node = Node::new("MonthGrid", None);
        let month = Date::new(2024, 2, 1).unwrap();
        let previous = month.add_days(-1).unwrap();
        let next = month.add_months(1).unwrap();
        let last = month.last_of_month();
        node.properties.insert("display-month", month.into());
        node.properties.insert("fixed-month", true.into());
        let state = CalendarState::from_node(&node);
        assert!(state.shows(previous, 1));
        assert!(state.shows(next, 1));
        assert!(!state.allows(previous));
        assert!(!state.allows(next));
        assert_eq!(state.selectable_from(last, true), Some(last));
        assert_eq!(state.selectable_from(next, true), None);
        assert_eq!(state.apply(CalendarAction::Select(next), Some(1)), None);
        assert_eq!(state.apply(CalendarAction::Browse(next), Some(1)), None);
        assert_eq!(
            state.apply(CalendarAction::Select(last), Some(1)),
            Some((Some(last), month))
        );
    }

    #[test]
    fn highlights_normalize_reverse_ranges_and_refresh_only_changed_cells() {
        use std::cell::Cell;
        let date = |day| Date::new(2024, 2, day).unwrap();
        let mut grid = Node::new("MonthGrid", None);
        grid.properties.insert("display-month", date(1).into());
        grid.properties
            .insert("disabled-dates", Value::DateList(vec![date(28)]));
        let rerendered = Cell::new(0);
        let cache = CalendarCells::default();
        cache.render(&grid, |_| Node::new("Rectangle", None));
        grid.properties.insert("highlight-start", date(29).into());
        grid.properties.insert("highlight-end", date(27).into());
        cache.render(&grid, |cell| {
            rerendered.set(rerendered.get() + 1);
            assert!(cell.in_range);
            assert_eq!(cell.range_start, cell.date == date(27));
            assert_eq!(cell.range_end, cell.date == date(29));
            assert_eq!(cell.enabled, cell.date != date(28));
            Node::new("Rectangle", None)
        });
        assert_eq!(rerendered.get(), 3);
        let next = Date::new(2024, 3, 1).unwrap();
        grid.properties.insert("highlight-start", next.into());
        let range = CalendarHighlight::from_node(&grid, true);
        assert!(!range.day(next, false).in_range);
        assert!(range.day(next, true).end);
        grid.properties.remove("highlight-start");
        assert!(
            !CalendarHighlight::from_node(&grid, false)
                .day(date(27), true)
                .in_range
        );
    }

    #[test]
    fn date_exclusions_skip_a_closure_longer_than_one_week() {
        let date = |day| Date::new(2024, 3, day).unwrap();
        let mut node = Node::new("Calendar", None);
        let mut closures = (1..=10).rev().map(date).collect::<Vec<_>>();
        closures.extend([
            date(2),
            Date {
                year: 2024,
                month: 2,
                day: 30,
            },
        ]);
        node.properties
            .insert("disabled-dates", Value::DateList(closures));
        node.properties
            .insert("disabled-weekdays", Value::IntList(vec![1]));
        let state = CalendarState::from_node(&node);
        assert_eq!(state.selectable_from(date(1), true), Some(date(12)));
        assert!(!state.allows(date(10)));
        assert!(!state.allows(date(11)));
        assert_eq!(state.apply(CalendarAction::Select(date(10)), None), None);
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
