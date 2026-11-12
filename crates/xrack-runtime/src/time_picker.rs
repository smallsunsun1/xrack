use crate::{
    ControlAction, Node, Tumbler, TumblerItems, VecModel,
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
    pub fn new(hour: i64, minute: i64, second: i64) -> Option<Self> {
        let value = Self {
            hour,
            minute,
            second,
        };
        (value == value.normalized()).then_some(value)
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
#[derive(Clone, Copy)]
pub struct TimePickerOptions {
    use_24_hours: bool,
    show_seconds: bool,
}

impl TimePickerOptions {
    pub fn new(use_24_hours: bool, show_seconds: bool) -> Self {
        Self {
            use_24_hours,
            show_seconds,
        }
    }

    pub fn from_node(node: &Node) -> Self {
        Self::new(
            node.boolean("use-24-hour-format", true),
            node.boolean("show-seconds", false),
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
}

#[doc(hidden)]
#[derive(Default)]
pub struct TimePopupState(PickerSession<Time>);

impl TimePopupState {
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
        TimePickerOptions::from_node(dialog).populate(&mut picker, draft);
        for wheel in &mut picker.children {
            self.0.stamp(wheel);
        }
        append_picker_dialog(dialog, picker);
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

    pub fn finish(&self, dialog: &Node) -> Option<Time> {
        self.0.finish(dialog)
    }
}
