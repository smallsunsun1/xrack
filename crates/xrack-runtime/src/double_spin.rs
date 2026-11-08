use crate::{Node, SpinAction, spin_editable};

/// A finite value carried by a decimal editor action.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinDecimal(f64);

impl Eq for SpinDecimal {}

impl SpinDecimal {
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    pub fn parse(text: &str, separator: char) -> Option<Self> {
        if !spin_text_is_valid(text, Some(separator)) {
            return None;
        }
        let normalized;
        let text = if separator == '.' {
            text
        } else {
            normalized = text.replace(separator, ".");
            &normalized
        };
        Self::new(text.parse().ok()?)
    }
}

pub fn spin_text_is_valid(text: &str, separator: Option<char>) -> bool {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    let ascii_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if let Some((whole, fraction)) = separator.and_then(|separator| digits.split_once(separator)) {
        ascii_digits(whole) && ascii_digits(fraction)
    } else {
        ascii_digits(digits)
    }
}

pub fn decimal_separator(node: &Node) -> char {
    if node.text("decimal-separator") == Some(",") {
        ','
    } else {
        '.'
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DecimalSpinRange {
    minimum: f64,
    maximum: f64,
    original: Option<SpinDecimal>,
    value: f64,
    step: f64,
    decimals: usize,
    separator: char,
    wrap: bool,
}

fn rounded(value: f64, decimals: usize) -> f64 {
    if !value.is_finite() {
        return value;
    }
    // Use the same decimal rounding for display, steps and commits.
    // The fixed-format form also handles finite values beyond integer scales.
    let rounded = format!("{value:.decimals$}").parse().unwrap_or(value);
    if rounded == 0.0 { 0.0 } else { rounded }
}

impl DecimalSpinRange {
    pub fn from_node(node: &Node) -> Self {
        let decimals = node.int("decimals").unwrap_or(2).clamp(0, 15) as usize;
        let round = |value| rounded(value, decimals);
        let minimum = round(node.number("minimum").unwrap_or(0.0));
        let maximum = round(node.number("maximum").unwrap_or(100.0)).max(minimum);
        let original = node.number("value").and_then(SpinDecimal::new);
        let value = original
            .map_or(minimum, |value| round(value.0))
            .clamp(minimum, maximum);
        let step = node.number("step-size").unwrap_or(1.0).max(0.0);
        let step = if step == 0.0 {
            0.0
        } else {
            round(step).max(10.0_f64.powi(-(decimals as i32)))
        };
        let separator = decimal_separator(node);
        Self {
            minimum,
            maximum,
            original,
            value,
            step,
            decimals,
            separator,
            wrap: node.boolean("wrap", false),
        }
    }

    pub fn value(self) -> f64 {
        self.value
    }

    pub fn limits(self) -> (f64, f64) {
        (self.minimum, self.maximum)
    }

    pub fn step(self) -> f64 {
        self.step
    }

    pub fn display(self) -> String {
        let text = format!("{:.*}", self.decimals, self.value);
        if self.separator == '.' {
            text
        } else {
            text.replace('.', ",")
        }
    }

    pub fn apply(self, action: SpinAction) -> Option<f64> {
        let next = match action {
            SpinAction::Increase => rounded(self.value + self.step, self.decimals),
            SpinAction::Decrease => rounded(self.value - self.step, self.decimals),
            SpinAction::Minimum => self.minimum,
            SpinAction::Maximum => self.maximum,
            SpinAction::CommitDecimal(value) => rounded(value.0, self.decimals),
            SpinAction::CommitDecimalIfCurrent { original, value } if original == self.original => {
                rounded(value.0, self.decimals)
            }
            _ => return None,
        };
        let next = match action {
            SpinAction::Increase if self.wrap && next > self.maximum => self.minimum,
            SpinAction::Decrease if self.wrap && next < self.minimum => self.maximum,
            _ => next.clamp(self.minimum, self.maximum),
        };
        (next != self.original.map_or(self.value, |value| value.0)).then_some(next)
    }
}

pub fn decimal_spin_value_after(node: &Node, action: SpinAction) -> Option<f64> {
    if node.kind != "DoubleSpinBox"
        || node.boolean("read-only", false)
        || matches!(
            action,
            SpinAction::CommitDecimal(_) | SpinAction::CommitDecimalIfCurrent { .. }
        ) && !spin_editable(node)
    {
        return None;
    }
    DecimalSpinRange::from_node(node).apply(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_steps_compare_rounded_values_before_wrapping() {
        let mut node = Node::new("DoubleSpinBox", None);
        node.properties.insert("minimum", (-0.4_f64).into());
        node.properties.insert("maximum", 0.3_f64.into());
        node.properties.insert("step-size", 0.1_f64.into());
        node.properties.insert("wrap", true.into());
        node.properties.insert("value", 0.2_f64.into());
        assert_eq!(
            decimal_spin_value_after(&node, SpinAction::Increase),
            Some(0.3)
        );
        node.properties.insert("value", 0.3_f64.into());
        assert_eq!(
            decimal_spin_value_after(&node, SpinAction::Increase),
            Some(-0.4)
        );
        node.properties.insert("value", (-0.4_f64).into());
        assert_eq!(
            decimal_spin_value_after(&node, SpinAction::Decrease),
            Some(0.3)
        );

        node.properties.insert("minimum", (-f64::MAX).into());
        node.properties.insert("maximum", f64::MAX.into());
        node.properties.insert("value", f64::MAX.into());
        node.properties.insert("step-size", f64::MAX.into());
        assert_eq!(
            decimal_spin_value_after(&node, SpinAction::Increase),
            Some(-f64::MAX)
        );
    }

    #[test]
    fn decimal_drafts_allow_partial_input_but_only_finite_values_can_commit() {
        assert!(spin_text_is_valid("-,", Some(',')));
        assert!(SpinDecimal::parse("-,", ',').is_none());
        assert!(SpinDecimal::parse("1.2", ',').is_none());
        assert!(SpinDecimal::parse("NaN", '.').is_none());
        assert!(SpinDecimal::parse(&"9".repeat(400), '.').is_none());
        assert!(SpinDecimal::new(f64::INFINITY).is_none());
        assert_eq!(SpinDecimal::parse("-1,2", ','), SpinDecimal::new(-1.2));
    }
}
