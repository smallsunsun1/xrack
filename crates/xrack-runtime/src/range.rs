use crate::{Node, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialInputMode {
    #[default]
    Circular,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollSnapMode {
    #[default]
    None,
    Always,
    OnRelease,
}

/// The normalized input understood by range controls, independent of the backend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeAction {
    SetFraction(f64),
    CommitFraction(f64),
    SetValue(f64),
    SetPoint { x: f64, y: f64 },
    Decrease,
    Increase,
    Step(i64),
    Minimum,
    Maximum,
    Release,
}

impl RangeAction {
    pub fn is_release(self) -> bool {
        matches!(self, Self::Release | Self::CommitFraction(_))
    }
}

impl Node {
    #[doc(hidden)]
    pub const RANGE_THUMBS: usize = 2;

    #[doc(hidden)]
    pub fn range_target(&self, path: &[usize]) -> Option<&Self> {
        let target = self.interactive_at_path(path)?;
        match target.kind {
            "Slider" | "Dial" | "ScrollBar" | "ColorPlane" => Some(target),
            "RangeThumb" => {
                let (&index, parent) = path.split_last()?;
                self.at_path(parent)
                    .filter(|node| node.kind == "RangeSlider" && index < Self::RANGE_THUMBS)
            }
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn add_range_thumbs(&mut self) {
        self.children
            .extend([Self::new("RangeThumb", None), Self::new("RangeThumb", None)]);
    }
}

/// Step by a wheel batch without iterating once per detent.
pub(crate) fn step_float(
    value: f64,
    limits: (f64, f64),
    step: f64,
    steps: i64,
    wrap: bool,
    normalize: impl Fn(f64) -> f64,
) -> f64 {
    let (min, max) = limits;
    if step == 0.0 || steps == 0 || min == max {
        return value;
    }
    let forward = steps > 0;
    let amount = steps.unsigned_abs() as f64;
    let signed_step = if forward { step } else { -step };
    let advance = |from, count| normalize(signed_step.mul_add(count, from));
    let limit = if forward { max } else { min };
    let fits = |candidate| {
        if forward {
            candidate <= limit
        } else {
            candidate >= limit
        }
    };
    let next = advance(value, amount);
    if !wrap || fits(next) {
        return next.clamp(min, max);
    }
    let steps_in_range = |from: f64| {
        let distance = limit - from;
        let estimate = if distance.is_finite() {
            distance / signed_step
        } else {
            (limit * 0.5 - from * 0.5) / (signed_step * 0.5)
        }
        .floor()
        .min(amount);
        if estimate < amount && fits(advance(from, estimate + 1.0)) {
            estimate + 1.0
        } else if estimate > 0.0 && !fits(advance(from, estimate)) {
            estimate - 1.0
        } else {
            estimate
        }
    };
    let start = if forward { min } else { max };
    let remaining = (amount - steps_in_range(value) - 1.0).max(0.0);
    let cycle = steps_in_range(start) + 1.0;
    advance(start, remaining % cycle).clamp(min, max)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderRange {
    min: f64,
    max: f64,
    value: f64,
    step: f64,
    wrap: bool,
    snap: ScrollSnapMode,
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
            snap: match node.properties.get("snap-mode") {
                Some(Value::ScrollSnapMode(mode)) => *mode,
                _ => ScrollSnapMode::None,
            },
        }
    }

    pub fn with_value(mut self, value: f64) -> Self {
        if value.is_finite() {
            self.value = value.clamp(self.min, self.max);
        }
        self
    }

    pub fn snap_mode(self) -> ScrollSnapMode {
        self.snap
    }

    pub fn preview_fraction(self, fraction: f64) -> f64 {
        self.with_value(
            self.apply(RangeAction::SetFraction(fraction))
                .unwrap_or(self.value),
        )
        .fraction()
    }

    fn snap_value(self, value: f64) -> f64 {
        let value = value.clamp(self.min, self.max);
        if self.step == 0.0 || value == self.min || value == self.max {
            return value;
        }
        let steps = ((value - self.min) / self.step).round();
        if !steps.is_finite() {
            return value;
        }
        let snapped = steps.mul_add(self.step, self.min).clamp(self.min, self.max);
        if self.max - value < (snapped - value).abs() {
            self.max
        } else {
            snapped
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
            RangeAction::SetFraction(fraction) | RangeAction::CommitFraction(fraction)
                if fraction.is_finite() =>
            {
                let t = fraction.clamp(0.0, 1.0);
                let value = self.min * (1.0 - t) + self.max * t;
                if self.snap == ScrollSnapMode::Always
                    || self.snap == ScrollSnapMode::OnRelease && action.is_release()
                {
                    self.snap_value(value)
                } else {
                    value
                }
            }
            RangeAction::SetValue(value) if value.is_finite() => {
                if self.snap == ScrollSnapMode::None {
                    value
                } else {
                    self.snap_value(value)
                }
            }
            RangeAction::Decrease => self.value - self.step,
            RangeAction::Increase => self.value + self.step,
            RangeAction::Step(steps) => step_float(
                self.value,
                (self.min, self.max),
                self.step,
                steps,
                self.wrap,
                |value| value,
            ),
            RangeAction::Minimum => self.min,
            RangeAction::Maximum => self.max,
            _ => return None,
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
pub struct ScrollBarRange {
    pub position: f64,
    pub size: f64,
    pub step: f64,
}

impl ScrollBarRange {
    pub const DEFAULT_SIZE: f64 = 0.0;
    pub const DEFAULT_STEP: f64 = 0.1;

    pub fn from_node(node: &Node) -> Self {
        Self::new(
            node.number("position").unwrap_or_default(),
            node.number("size").unwrap_or(Self::DEFAULT_SIZE),
            node.number("step-size").unwrap_or(Self::DEFAULT_STEP),
        )
    }

    pub fn new(position: f64, size: f64, step: f64) -> Self {
        let unit = |value: f64, default| {
            if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                default
            }
        };
        let size = unit(size, Self::DEFAULT_SIZE);
        Self {
            position: unit(position, 0.0).min(1.0 - size),
            size,
            step: unit(step, Self::DEFAULT_STEP),
        }
    }

    pub fn maximum(self) -> f64 {
        1.0 - self.size
    }

    pub fn apply(self, action: RangeAction) -> Option<f64> {
        let next = match action {
            RangeAction::SetFraction(fraction) if fraction.is_finite() => fraction * self.maximum(),
            RangeAction::Decrease => self.position - self.step,
            RangeAction::Increase => self.position + self.step,
            RangeAction::Minimum => 0.0,
            RangeAction::Maximum => self.maximum(),
            _ => return None,
        }
        .clamp(0.0, self.maximum());
        (next != self.position).then_some(next)
    }
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

    pub fn nearest_handle(self, fraction: f64) -> usize {
        let [lower, upper] = self.fractions();
        usize::from(if lower == upper {
            fraction >= lower
        } else {
            (fraction - upper).abs() < (fraction - lower).abs()
        })
    }

    pub fn preview_fractions(self, handle: usize, fraction: f64) -> [f64; 2] {
        let values = self
            .apply(handle, RangeAction::SetFraction(fraction))
            .unwrap_or(self.values);
        Self { values, ..self }.fractions()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batched_steps_keep_remaining_detents_after_wrap() {
        let mut dial = Node::new("Dial", None);
        dial.properties.insert("minimum", (-2.0_f64).into());
        dial.properties.insert("maximum", 3.0_f64.into());
        dial.properties.insert("value", 2.0_f64.into());
        dial.properties.insert("wrap", true.into());
        let range = SliderRange::from_node(&dial);
        assert_eq!(range.apply(RangeAction::Step(3)), Some(-1.0));
        assert_eq!(range.apply(RangeAction::Step(-6)), None);
        assert_eq!(
            step_float(
                -f64::MAX,
                (-f64::MAX, f64::MAX),
                f64::MAX,
                10,
                true,
                |value| value
            ),
            0.0,
        );
    }

    #[test]
    fn slider_snap_uses_the_minimum_and_keeps_a_partial_final_step_reachable() {
        let mut node = Node::new("Slider", None);
        node.properties.insert("minimum", 3.0_f64.into());
        node.properties.insert("maximum", 17.0_f64.into());
        node.properties.insert("step", 5.0_f64.into());
        node.properties
            .insert("snap-mode", ScrollSnapMode::OnRelease.into());
        let range = SliderRange::from_node(&node);
        assert_eq!(range.apply(RangeAction::SetFraction(0.5)), Some(10.0));
        assert_eq!(range.apply(RangeAction::CommitFraction(0.5)), Some(8.0));
        assert_eq!(range.apply(RangeAction::CommitFraction(0.96)), Some(17.0));
        assert_eq!(range.apply(RangeAction::SetValue(15.0)), Some(13.0));
        assert_eq!(range.apply(RangeAction::SetValue(f64::INFINITY)), None);
    }
}
