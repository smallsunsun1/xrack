use crate::Node;
use std::fmt;

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

    fn add_months(self, months: i64) -> Option<Self> {
        if !self.valid() {
            return None;
        }
        let index = ((self.year - 1) * 12 + self.month - 1).checked_add(months)?;
        Self::new(index.div_euclid(12) + 1, index.rem_euclid(12) + 1, 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarAction {
    Select(Date),
    Browse(Date),
}

#[derive(Clone, Copy, Debug)]
pub struct CalendarState {
    pub minimum: Date,
    pub maximum: Date,
    pub selected: Date,
    /// First day of the displayed month.
    pub month: Date,
}

impl CalendarState {
    pub fn from_node(node: &Node) -> Self {
        let date = |key| node.date(key).filter(|date| date.valid());
        let minimum = date("minimum-date").unwrap_or(Date::MIN);
        let maximum = date("maximum-date").unwrap_or(Date::MAX).max(minimum);
        let selected = date("date").unwrap_or_default().clamp(minimum, maximum);
        let month = date("display-month")
            .unwrap_or(selected)
            .clamp(minimum, maximum)
            .first_of_month();
        Self {
            minimum,
            maximum,
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
        let offset = (self.month.ordinal() - (week_start.clamp(1, 7) - 1)).rem_euclid(7);
        std::array::from_fn(|index| self.month.add_days(index as i64 - offset))
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
