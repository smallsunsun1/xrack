use crate::{
    ControlAction, DialogRole, InputRoute, Node, Tumbler, TumblerItems, VecModel,
    picker::{PickerSession, append_picker_dialog},
};
use std::fmt;

/// A time of day with second precision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time {
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
}

impl Time {
    pub const MIN: Self = Self {
        hour: 0,
        minute: 0,
        second: 0,
    };
    pub const MAX: Self = Self {
        hour: 23,
        minute: 59,
        second: 59,
    };

    pub fn new(hour: i64, minute: i64, second: i64) -> Option<Self> {
        let value = Self {
            hour,
            minute,
            second,
        };
        (value == value.normalized()).then_some(value)
    }

    /// Parse an ASCII time of day, with optional seconds.
    pub fn parse_iso(text: &str) -> Option<Self> {
        let (hour, minute, second) = match text.as_bytes() {
            [h1, h2, b':', m1, m2] => ((*h1, *h2), (*m1, *m2), (b'0', b'0')),
            [h1, h2, b':', m1, m2, b':', s1, s2] => ((*h1, *h2), (*m1, *m2), (*s1, *s2)),
            _ => return None,
        };
        let pair = |(left, right): (u8, u8)| {
            (left.is_ascii_digit() && right.is_ascii_digit())
                .then(|| i64::from(left - b'0') * 10 + i64::from(right - b'0'))
        };
        Self::new(pair(hour)?, pair(minute)?, pair(second)?)
    }

    pub fn format_iso(self, show_seconds: bool) -> String {
        let time = self.normalized();
        if show_seconds {
            time.to_string()
        } else {
            format!("{:02}:{:02}", time.hour, time.minute)
        }
    }

    pub fn in_range(self, minimum: Self, maximum: Self) -> bool {
        self == self.normalized() && (minimum.normalized()..=maximum.normalized()).contains(&self)
    }

    pub fn normalized(self) -> Self {
        Self {
            hour: self.hour.clamp(0, 23),
            minute: self.minute.clamp(0, 59),
            second: self.second.clamp(0, 59),
        }
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

thread_local! {
    static HOURS_24: TumblerItems = numbers(0..24);
    static HOURS_12: TumblerItems = numbers(std::iter::once(12).chain(1..12));
    static MINUTES_SECONDS: TumblerItems = numbers(0..60);
    static PERIODS: TumblerItems = TumblerItems::new(VecModel::from(vec!["AM".into(), "PM".into()]));
}

fn numbers(values: impl Iterator<Item = i64>) -> TumblerItems {
    TumblerItems::new(VecModel::from(
        values
            .map(|value| format!("{value:02}"))
            .collect::<Vec<_>>(),
    ))
}

#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TimePickerOptions {
    use_24_hours: bool,
    show_seconds: bool,
    minimum: Time,
    maximum: Time,
}

impl TimePickerOptions {
    pub fn new(use_24_hours: bool, show_seconds: bool) -> Self {
        Self {
            use_24_hours,
            show_seconds,
            minimum: Time::MIN,
            maximum: Time::MAX,
        }
    }

    pub fn with_bounds(mut self, minimum: Time, maximum: Time) -> Self {
        self.minimum = minimum.normalized();
        self.maximum = maximum.normalized();
        self
    }

    fn allows(self, time: Time) -> bool {
        time.in_range(self.minimum, self.maximum)
    }

    pub fn from_node(node: &Node) -> Self {
        Self::new(
            node.boolean("use-24-hour-format", true),
            node.boolean("show-seconds", false),
        )
        .with_bounds(
            node.time("minimum-time").unwrap_or(Time::MIN),
            node.time("maximum-time").unwrap_or(Time::MAX),
        )
    }

    fn wheel(self, time: Time, slot: usize) -> Option<(&'static str, TumblerItems, i64)> {
        Some(match slot {
            0 if self.use_24_hours => ("Hour", HOURS_24.with(Clone::clone), time.hour),
            0 => ("Hour", HOURS_12.with(Clone::clone), time.hour % 12),
            1 => ("Minute", MINUTES_SECONDS.with(Clone::clone), time.minute),
            2 if self.show_seconds => ("Second", MINUTES_SECONDS.with(Clone::clone), time.second),
            3 if !self.use_24_hours => ("AM/PM", PERIODS.with(Clone::clone), time.hour / 12),
            _ => return None,
        })
    }

    pub fn populate(self, picker: &mut Node, time: Time) {
        let time = time.normalized();
        for slot in 0..4 {
            let wheel = if let Some((label, model, index)) = self.wheel(time, slot) {
                let mut wheel = Node::new("Tumbler", None);
                wheel.tumbler_items = Some(model);
                wheel
                    .properties
                    .insert("accessible-name", label.to_owned().into());
                wheel.properties.insert("current-index", index.into());
                wheel
            } else {
                Node::hidden()
            };
            picker.children.push(wheel);
        }
    }

    pub fn select(
        self,
        time: Time,
        slot: usize,
        node: &Node,
        action: ControlAction,
    ) -> Option<Time> {
        let ControlAction::PickTumbler { index, row } = action else {
            return None;
        };
        let time = time.normalized();
        let (_, model, current) = self.wheel(time, slot)?;
        let index_value = i64::try_from(index).ok()?;
        if node.kind != "Tumbler"
            || node.tumbler_items.as_ref() != Some(&model)
            || node.int("current-index") != Some(current)
            || Tumbler::from_node(node).row_key(index) != Some(row)
            || index_value == current
        {
            return None;
        }
        Some(match slot {
            0 => Time {
                hour: index_value
                    + if self.use_24_hours {
                        0
                    } else {
                        time.hour / 12 * 12
                    },
                ..time
            },
            1 => Time {
                minute: index_value,
                ..time
            },
            2 => Time {
                second: index_value,
                ..time
            },
            3 => Time {
                hour: time.hour % 12 + index_value * 12,
                ..time
            },
            _ => unreachable!(),
        })
    }

    /// Resolve a wheel selection before either backend writes the time.
    pub fn try_select_input<E>(
        node: &Node,
        slot: usize,
        action: ControlAction,
        input: InputRoute<'_>,
        options: impl FnOnce() -> Result<Self, E>,
        time: impl FnOnce() -> Result<Option<Time>, E>,
    ) -> Result<Option<Time>, E> {
        let ControlAction::PickTumbler { index, row } = action else {
            return Ok(None);
        };
        if node.kind != "Tumbler"
            || Tumbler::from_node(node).row_key(index) != Some(row)
            || !input.is_current()
        {
            return Ok(None);
        }
        crate::property::watch_reads(|reads| {
            let options = options()?;
            if reads.changed() || !input.is_current() {
                return Ok(None);
            }
            let Some(time) = time()? else {
                return Ok(None);
            };
            Ok(options
                .select(time, slot, node, action)
                .filter(|_| !reads.changed() && input.is_current()))
        })
    }

    pub fn select_input(
        node: &Node,
        slot: usize,
        action: ControlAction,
        input: InputRoute<'_>,
        options: impl FnOnce() -> Self,
        time: impl FnOnce() -> Time,
    ) -> Option<Time> {
        Self::try_select_input(
            node,
            slot,
            action,
            input,
            || Ok::<_, std::convert::Infallible>(options()),
            || Ok(Some(time())),
        )
        .unwrap_or_else(|error| match error {})
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct TimePopupState(PickerSession<Time>);

impl TimePopupState {
    #[doc(hidden)]
    pub fn accepts_input(&self, node: &Node, input: InputRoute<'_>) -> bool {
        self.0.accepts_input(node, input)
    }

    pub fn close(&self) {
        self.0.close();
    }

    pub fn populate(&self, dialog: &mut Node, time: impl FnOnce() -> Time) {
        self.try_populate(dialog, || Ok::<_, std::convert::Infallible>(time()))
            .unwrap_or_else(|error| match error {});
    }

    pub fn try_populate<E>(
        &self,
        dialog: &mut Node,
        time: impl FnOnce() -> Result<Time, E>,
    ) -> Result<(), E> {
        let Some(draft) = self.0.try_begin(dialog, || time().map(Time::normalized))? else {
            return Ok(());
        };
        let mut picker = Node::new("TimePicker", None);
        let options = TimePickerOptions::from_node(dialog);
        options.populate(&mut picker, draft);
        for wheel in &mut picker.children {
            self.0.stamp(wheel);
        }
        append_picker_dialog(dialog, picker, options.allows(draft));
        Ok(())
    }

    pub fn edit(
        &self,
        options: TimePickerOptions,
        slot: usize,
        node: &Node,
        action: ControlAction,
    ) -> bool {
        let Some(next) = self
            .0
            .current(node)
            .and_then(|time| options.select(time, slot, node, action))
        else {
            return false;
        };
        self.0.set(next);
        true
    }

    pub fn try_edit_input<E>(
        &self,
        node: &Node,
        slot: usize,
        action: ControlAction,
        input: InputRoute<'_>,
        options: impl FnOnce() -> Result<TimePickerOptions, E>,
    ) -> Result<bool, E> {
        if !self.accepts_input(node, input) {
            return Ok(false);
        }
        let Some(next) =
            TimePickerOptions::try_select_input(node, slot, action, input, options, || {
                Ok(self.0.current(node))
            })?
        else {
            return Ok(false);
        };
        self.0.set(next);
        Ok(true)
    }

    pub fn edit_input(
        &self,
        node: &Node,
        slot: usize,
        action: ControlAction,
        input: InputRoute<'_>,
        options: impl FnOnce() -> TimePickerOptions,
    ) -> bool {
        self.try_edit_input(node, slot, action, input, || {
            Ok::<_, std::convert::Infallible>(options())
        })
        .unwrap_or_else(|error| match error {})
    }

    /// Confirm only the draft displayed with the current options.
    pub fn try_finish_input<E>(
        &self,
        dialog: &Node,
        role: DialogRole,
        input: InputRoute<'_>,
        options: impl FnOnce() -> Result<TimePickerOptions, E>,
    ) -> Result<Option<Time>, E> {
        self.0.try_finish_input(dialog, role, input, |draft| {
            let options = options()?;
            Ok(
                (options == TimePickerOptions::from_node(dialog) && options.allows(draft))
                    .then_some(draft),
            )
        })
    }

    pub fn finish_input(
        &self,
        dialog: &Node,
        role: DialogRole,
        input: InputRoute<'_>,
        options: impl FnOnce() -> TimePickerOptions,
    ) -> Option<Time> {
        self.try_finish_input(dialog, role, input, || {
            Ok::<_, std::convert::Infallible>(options())
        })
        .unwrap_or_else(|error| match error {})
    }

    pub fn finish(&self, dialog: &Node) -> Option<Time> {
        self.finish_with_role(dialog, DialogRole::Accept)
    }

    pub fn finish_with_role(&self, dialog: &Node, role: DialogRole) -> Option<Time> {
        if role == DialogRole::Accept
            && !TimePickerOptions::from_node(dialog).allows(self.0.current(dialog)?)
        {
            return None;
        }
        self.0.finish(dialog, role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_strict_iso_times() {
        assert_eq!(Time::parse_iso("00:00"), Some(Time::MIN));
        assert_eq!(Time::parse_iso("23:59:59"), Some(Time::MAX));
        assert_eq!(Time::parse_iso("13:07"), Time::new(13, 7, 0));
        let time = Time::new(1, 2, 3).unwrap();
        assert_eq!(time.format_iso(false), "01:02");
        assert_eq!(time.format_iso(true), "01:02:03");
        for invalid in [
            "24:00",
            "12:60",
            "23:59:60",
            "1:23",
            "01:2",
            "01:23:",
            "01:23Z",
            " 01:23",
            "１２:３４",
            "01:2é",
        ] {
            assert!(Time::parse_iso(invalid).is_none(), "{invalid}");
        }
        assert!(time.in_range(Time::MIN, time));
        assert!(!time.in_range(Time::MAX, Time::MIN));
    }

    #[test]
    fn time_popup_keeps_an_out_of_range_draft_and_disables_confirmation() {
        let draft = Time::new(15, 30, 12).unwrap();
        let earlier = Time::new(15, 0, 0).unwrap();
        let state = TimePopupState::default();
        let dialog = |maximum: Time| {
            let mut node = Node::new("TimePickerPopup", None);
            node.properties.insert("open", true.into());
            node.properties.insert("maximum-time", maximum.into());
            node
        };
        let mut disabled = dialog(earlier);
        state.populate(&mut disabled, || draft);
        assert!(!disabled.children[1].children[1].boolean("enabled", true));
        assert_eq!(state.finish(&disabled), None);
        let mut enabled = dialog(draft);
        state.populate(&mut enabled, || Time::MIN);
        assert!(enabled.children[1].children[1].boolean("enabled", true));
        assert_eq!(state.finish(&enabled), Some(draft));
    }
}
