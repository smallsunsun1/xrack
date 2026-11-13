//! Checked integer operations for XR expressions.

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
