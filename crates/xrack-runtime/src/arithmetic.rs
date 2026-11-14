//! Numeric operations for XR expressions and bindings.

/// Compare stored floats without losing the sign of zero or a NaN payload.
#[doc(hidden)]
pub fn same_float_bits(left: &f64, right: &f64) -> bool {
    left.to_bits() == right.to_bits()
}

pub fn try_int_divide(left: i64, right: i64) -> Result<i64, &'static str> {
    left.checked_div(right).ok_or("invalid integer division")
}

pub fn try_int_remainder(left: i64, right: i64) -> Result<i64, &'static str> {
    left.checked_rem(right).ok_or("invalid integer remainder")
}

#[track_caller]
pub fn int_divide(left: i64, right: i64) -> i64 {
    match try_int_divide(left, right) {
        Ok(value) => value,
        Err(message) => panic!("{message}"),
    }
}

#[track_caller]
pub fn int_remainder(left: i64, right: i64) -> i64 {
    match try_int_remainder(left, right) {
        Ok(value) => value,
        Err(message) => panic!("{message}"),
    }
}

/// Round to an integer, resolving ties away from zero and saturating at the integer bounds.
#[doc(hidden)]
pub fn round_to_int(value: f64) -> i64 {
    value.round() as i64
}

#[doc(hidden)]
pub fn floor_to_int(value: f64) -> i64 {
    value.floor() as i64
}

#[doc(hidden)]
pub fn ceil_to_int(value: f64) -> i64 {
    value.ceil() as i64
}

#[doc(hidden)]
pub fn clamp_int(value: i64, first: i64, second: i64) -> i64 {
    value.clamp(first.min(second), first.max(second))
}

/// Preserve the negative sign when choosing the minimum of two zeros.
#[doc(hidden)]
pub fn min_float(left: f64, right: f64) -> f64 {
    if left == 0.0 && right == 0.0 {
        f64::from_bits(left.to_bits() | right.to_bits())
    } else {
        left.min(right)
    }
}

/// Preserve the positive sign when choosing the maximum of two zeros.
#[doc(hidden)]
pub fn max_float(left: f64, right: f64) -> f64 {
    if left == 0.0 && right == 0.0 {
        f64::from_bits(left.to_bits() & right.to_bits())
    } else {
        left.max(right)
    }
}

#[doc(hidden)]
pub fn clamp_float(value: f64, first: f64, second: f64) -> f64 {
    let min = min_float(first, second);
    let max = max_float(first, second);
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

#[doc(hidden)]
pub fn format_fixed_int(value: i64, precision: usize) -> String {
    if precision == 0 {
        value.to_string()
    } else {
        format!("{value}.{:0>precision$}", "")
    }
}

#[doc(hidden)]
pub fn format_fixed_float(value: f64, precision: usize) -> String {
    format!("{value:.precision$}")
}

#[doc(hidden)]
pub fn character_count(text: &str) -> i64 {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).count() as i64
}

#[doc(hidden)]
pub fn modulo_int(value: i64, divisor: i64) -> i64 {
    value.checked_rem_euclid(divisor).unwrap_or(0)
}

#[doc(hidden)]
pub fn number_sign(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value.signum() }
}

#[doc(hidden)]
pub fn parse_int(text: &str) -> Option<i64> {
    text.trim().parse().ok()
}

#[doc(hidden)]
pub fn parse_float(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}
