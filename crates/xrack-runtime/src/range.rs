use crate::Node;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialInputMode {
    #[default]
    Circular,
    Horizontal,
    Vertical,
}

/// The normalized input understood by range controls, independent of the backend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeAction {
    SetFraction(f64),
    Decrease,
    Increase,
    Minimum,
    Maximum,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderRange {
    min: f64,
    max: f64,
    value: f64,
    step: f64,
    wrap: bool,
}

impl SliderRange {
    pub fn from_node(node: &Node) -> Self {
        let min = node.number("minimum").unwrap_or(0.0);
        let max = node.number("maximum").unwrap_or(100.0).max(min);
        Self {
            min,
            max,
            value: node.number("value").unwrap_or(0.0).clamp(min, max),
            step: node.number("step").unwrap_or(1.0).max(0.0),
            wrap: node.kind == "Dial" && node.boolean("wrap", false),
        }
    }

    pub fn value(self) -> f64 {
        self.value
    }

    pub fn fraction(self) -> f64 {
        if self.max == self.min {
            return 0.0;
        }
        let span = self.max - self.min;
        if span.is_finite() {
            (self.value - self.min) / span
        } else {
            // Halve first so even a range spanning both finite extremes works.
            (self.value * 0.5 - self.min * 0.5) / (self.max * 0.5 - self.min * 0.5)
        }
    }

    pub fn apply(self, action: RangeAction) -> Option<f64> {
        let next = match action {
            RangeAction::SetFraction(fraction) if fraction.is_finite() => {
                let t = fraction.clamp(0.0, 1.0);
                self.min * (1.0 - t) + self.max * t
            }
            RangeAction::Decrease => self.value - self.step,
            RangeAction::Increase => self.value + self.step,
            RangeAction::Minimum => self.min,
            RangeAction::Maximum => self.max,
            RangeAction::SetFraction(_) | RangeAction::Release => return None,
        };
        let next = match action {
            RangeAction::Increase if self.wrap && next > self.max => self.min,
            RangeAction::Decrease if self.wrap && next < self.min => self.max,
            _ => next.clamp(self.min, self.max),
        };
        (next != self.value).then_some(next)
    }
}

pub fn progress_value(node: &Node) -> f64 {
    node.number("progress").unwrap_or(0.0).clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IndicatorState {
    Hidden,
    Determinate(f64),
    Indeterminate,
}

pub fn indicator_state(node: &Node) -> Option<IndicatorState> {
    Some(match node.kind {
        "BusyIndicator" if node.boolean("running", true) => IndicatorState::Indeterminate,
        "BusyIndicator" => IndicatorState::Hidden,
        "Spinner" | "ProgressIndicator" if node.boolean("indeterminate", false) => {
            IndicatorState::Indeterminate
        }
        "Spinner" | "ProgressIndicator" => IndicatorState::Determinate(progress_value(node)),
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct SliderInterval {
    axis: SliderRange,
    values: [f64; 2],
}

impl SliderInterval {
    pub fn from_node(node: &Node) -> Self {
        let axis = SliderRange::from_node(node);
        Self::from_values(
            node,
            node.number("lower-value").unwrap_or(axis.min),
            node.number("upper-value").unwrap_or(axis.max),
        )
    }

    pub fn from_values(node: &Node, lower: f64, upper: f64) -> Self {
        let axis = SliderRange::from_node(node);
        let upper = if upper.is_finite() {
            upper.clamp(axis.min, axis.max)
        } else {
            axis.max
        };
        let lower = if lower.is_finite() {
            lower.clamp(axis.min, upper)
        } else {
            axis.min
        };
        Self {
            axis,
            values: [lower, upper],
        }
    }

    pub fn values(self) -> [f64; 2] {
        self.values
    }

    pub fn fractions(self) -> [f64; 2] {
        self.values
            .map(|value| SliderRange { value, ..self.axis }.fraction())
    }

    pub fn apply(self, handle: usize, action: RangeAction) -> Option<[f64; 2]> {
        let value = *self.values.get(handle)?;
        let mut values = self.values;
        let next = SliderRange { value, ..self.axis }.apply(action)?;
        values[handle] = if handle == 0 {
            next.min(values[1])
        } else {
            next.max(values[0])
        };
        (values != self.values).then_some(values)
    }
}
