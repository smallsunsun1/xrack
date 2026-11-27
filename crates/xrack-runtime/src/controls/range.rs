use crate::{LayoutDirection, Node, Orientation, Property, Value};
use std::{fmt, rc::Rc};

/// Transient thumb input and the resolved layout direction for a range control.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct RangePositionHandle(Rc<RangePositionState>);

#[derive(Default)]
struct RangePositionState {
    preview: Property<Option<(usize, f64)>>,
    direction: Property<LayoutDirection>,
}

impl PartialEq for RangePositionHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for RangePositionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RangePositionHandle")
            .finish_non_exhaustive()
    }
}

impl RangePositionHandle {
    pub fn publish_preview(&self, preview: Option<(usize, f64)>) {
        self.0.preview.set_if_changed(preview);
    }

    pub fn publish_direction(&self, direction: LayoutDirection) {
        self.0.direction.set_if_changed(direction);
    }

    pub fn slider_position(&self, range: SliderRange, live: bool) -> f64 {
        if !live && let Some((_, fraction)) = self.0.preview.get(|| None) {
            return range.preview_fraction(fraction);
        }
        range.fraction()
    }

    pub fn interval_position(&self, range: SliderInterval, live: bool, handle: usize) -> f64 {
        let fractions = if !live && let Some((index, fraction)) = self.0.preview.get(|| None) {
            range.preview_fractions(index, fraction)
        } else {
            range.fractions()
        };
        fractions[handle]
    }

    pub fn visual_position(&self, position: f64, orientation: Orientation) -> f64 {
        if orientation == Orientation::Vertical
            || self.0.direction.get(LayoutDirection::default) == LayoutDirection::RightToLeft
        {
            1.0 - position
        } else {
            position
        }
    }
}

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

    /// A release observes live values after the change handler has run.
    #[doc(hidden)]
    pub fn try_deliver<T, E>(
        self,
        change: Option<T>,
        commit: impl FnOnce(T) -> Result<bool, E>,
        release: impl FnOnce() -> Result<(), E>,
    ) -> Result<bool, E> {
        match change {
            Some(value) => {
                if !commit(value)? {
                    return Ok(false);
                }
            }
            None if !self.is_release() => return Ok(false),
            None => {}
        }
        if self.is_release() {
            release()?;
        }
        Ok(true)
    }

    #[doc(hidden)]
    pub fn deliver<T>(
        self,
        change: Option<T>,
        commit: impl FnOnce(T),
        release: impl FnOnce(),
    ) -> bool {
        self.try_deliver(
            change,
            |value| {
                commit(value);
                Ok::<_, std::convert::Infallible>(true)
            },
            || {
                release();
                Ok(())
            },
        )
        .unwrap_or_else(|error| match error {})
    }
}

#[doc(hidden)]
pub const RANGE_VIEW_PROPERTIES: &[&str] = &[
    "minimum",
    "maximum",
    "step",
    "snap-mode",
    "wrap",
    "orientation",
    "input-mode",
    "start-angle",
    "end-angle",
    "live",
];

#[doc(hidden)]
pub fn range_view_properties(kind: &str) -> &'static [&'static str] {
    if kind == "ScrollBar" {
        &[
            "size",
            "step-size",
            "minimum-size",
            "interactive",
            "policy",
            "orientation",
            "snap-mode",
        ]
    } else {
        RANGE_VIEW_PROPERTIES
    }
}

/// Validate the range settings and capture live values before applying input.
#[doc(hidden)]
pub fn try_prepare_range_view<T, E>(
    node: &Node,
    action: RangeAction,
    available: impl FnOnce() -> Result<bool, E>,
    mut read: impl FnMut(&str) -> Result<Option<Value>, E>,
    values: impl FnOnce() -> Result<T, E>,
) -> Result<Option<T>, E> {
    if node.kind == "ScrollBar"
        && !match action {
            RangeAction::SetFraction(value) => value.is_finite(),
            RangeAction::Increase
            | RangeAction::Decrease
            | RangeAction::Minimum
            | RangeAction::Maximum => true,
            _ => false,
        }
    {
        return Ok(None);
    }
    let pointer = matches!(
        action,
        RangeAction::SetFraction(_) | RangeAction::CommitFraction(_) | RangeAction::SetPoint { .. }
    );
    crate::property::watch_reads(|reads| {
        if !available()? || reads.changed() {
            return Ok(None);
        }
        if action != RangeAction::Release {
            for &name in range_view_properties(node.kind) {
                let used = pointer
                    || !matches!(
                        name,
                        "orientation" | "input-mode" | "start-angle" | "end-angle" | "live"
                    );
                if used && (read(name)?.as_ref() != node.properties.get(name) || reads.changed()) {
                    return Ok(None);
                }
            }
        }
        let values = values()?;
        Ok((!reads.changed()).then_some(values))
    })
}

#[doc(hidden)]
pub fn prepare_range_view<T>(
    node: &Node,
    action: RangeAction,
    available: impl FnOnce() -> bool,
    mut read: impl FnMut(&str) -> Option<Value>,
    values: impl FnOnce() -> T,
) -> Option<T> {
    try_prepare_range_view(
        node,
        action,
        || Ok::<_, std::convert::Infallible>(available()),
        |name| Ok(read(name)),
        || Ok(values()),
    )
    .unwrap_or_else(|error| match error {})
}

impl Node {
    #[doc(hidden)]
    pub const RANGE_THUMBS: usize = 2;
    #[doc(hidden)]
    pub const RANGE_HANDLE_PARTS: [&'static str; Self::RANGE_THUMBS] =
        ["RangeLowerHandle", "RangeUpperHandle"];

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
        Self::new(
            node.number("minimum").unwrap_or(0.0),
            node.number("maximum").unwrap_or(100.0),
            node.number("value").unwrap_or(0.0),
            node.number("step").unwrap_or(1.0),
            node.kind == "Dial" && node.boolean("wrap", false),
            match node.properties.get("snap-mode") {
                Some(Value::ScrollSnapMode(mode)) => *mode,
                _ => ScrollSnapMode::None,
            },
        )
    }

    #[doc(hidden)]
    pub fn new(
        min: f64,
        max: f64,
        value: f64,
        step: f64,
        wrap: bool,
        snap: ScrollSnapMode,
    ) -> Self {
        let min = if min.is_finite() { min } else { 0.0 };
        let max = if max.is_finite() { max } else { 100.0 };
        let value = if value.is_finite() { value } else { 0.0 };
        let step = if step.is_finite() { step } else { 1.0 };
        let max = max.max(min);
        Self {
            min,
            max,
            value: value.clamp(min, max),
            step: step.max(0.0),
            wrap,
            snap,
        }
    }

    pub fn with_value(mut self, value: f64) -> Self {
        if value.is_finite() {
            self.value = value.clamp(self.min, self.max);
        }
        self
    }

    #[doc(hidden)]
    pub fn with_bounds(mut self, minimum: f64, maximum: f64) -> Self {
        self.min = minimum;
        self.max = maximum.max(minimum);
        self.value = self.value.clamp(self.min, self.max);
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

/// Numeric bounds and the clamped business value of a progress bar.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressRange {
    pub minimum: f64,
    pub maximum: f64,
    pub value: f64,
}

impl ProgressRange {
    pub fn new(minimum: f64, maximum: f64, value: f64) -> Self {
        let minimum = if minimum.is_finite() { minimum } else { 0.0 };
        let maximum = if maximum.is_finite() { maximum } else { 1.0 }.max(minimum);
        let value = if value.is_finite() { value } else { 0.0 }.clamp(minimum, maximum);
        Self {
            minimum,
            maximum,
            value,
        }
    }

    pub fn from_node(node: &Node) -> Self {
        Self::new(
            node.number("minimum").unwrap_or(0.0),
            node.number("maximum").unwrap_or(1.0),
            node.number("progress").unwrap_or(0.0),
        )
    }

    pub fn position(self) -> f64 {
        let span = self.maximum - self.minimum;
        if span == 0.0 {
            return 0.0;
        }
        if span.is_finite() {
            (self.value - self.minimum) / span
        } else {
            // Halve before subtracting when finite opposite bounds overflow.
            (self.value * 0.5 - self.minimum * 0.5) / (self.maximum * 0.5 - self.minimum * 0.5)
        }
    }
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

    #[doc(hidden)]
    pub fn position_with(position: impl FnOnce() -> f64, size: impl FnOnce() -> f64) -> f64 {
        Self::try_position_with(
            || Ok::<_, std::convert::Infallible>(position()),
            || Ok(size()),
        )
        .unwrap_or_else(|error| match error {})
    }

    /// Read the position binding before the size that constrains it.
    #[doc(hidden)]
    pub fn try_position_with<E>(
        position: impl FnOnce() -> Result<f64, E>,
        size: impl FnOnce() -> Result<f64, E>,
    ) -> Result<f64, E> {
        let position = position()?;
        let size = size()?;
        Ok(Self::new(position, size, 0.0).position)
    }

    pub fn maximum(self) -> f64 {
        1.0 - self.size
    }

    #[doc(hidden)]
    pub fn with_position(self, position: f64) -> Self {
        Self::new(position, self.size, self.step)
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
        "ProgressIndicator" => {
            IndicatorState::Determinate(ProgressRange::from_node(node).position())
        }
        "Spinner" => IndicatorState::Determinate(progress_value(node)),
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
        Self::from_range(SliderRange::from_node(node), lower, upper)
    }

    #[doc(hidden)]
    pub fn from_range(axis: SliderRange, lower: f64, upper: f64) -> Self {
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

    #[doc(hidden)]
    pub fn changed_values(
        self,
        handle: usize,
        action: RangeAction,
        raw: [f64; 2],
    ) -> Option<[Option<f64>; 2]> {
        let next = self.apply(handle, action)?;
        Some(std::array::from_fn(|index| {
            (next[index] != raw[index]).then_some(next[index])
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_range_stays_finite_for_extreme_and_collapsed_bounds() {
        assert_eq!(ProgressRange::new(-f64::MAX, f64::MAX, 0.0).position(), 0.5);
        assert_eq!(ProgressRange::new(100.0, 90.0, 120.0).position(), 0.0);
        assert_eq!(ProgressRange::new(100.0, 900.0, f64::NAN).value, 100.0);
        assert_eq!(ProgressRange::new(100.0, 900.0, 1100.0).value, 900.0);
    }

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
