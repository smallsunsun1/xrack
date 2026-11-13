use crate::{CheckState, VecModel};
use std::borrow::Cow;

pub(crate) const MAX_CLIPBOARD_CELLS: usize = 1_000_000;
pub(crate) const MAX_CLIPBOARD_BYTES: usize = 8 * 1024 * 1024;

/// A pending paste into the model version observed before the clipboard read.
#[doc(hidden)]
pub type TablePaste = Box<dyn FnOnce(&str) -> bool>;

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct TablePasteInput<'a> {
    pub stamp: (u64, u64),
    pub row: usize,
    pub columns: &'a [usize],
    pub text: &'a str,
}

fn field(input: &str) -> Option<(Cow<'_, str>, &str)> {
    let Some(mut quoted) = input.strip_prefix('"') else {
        let end = input.find(['\t', '\r', '\n']).unwrap_or(input.len());
        return Some((Cow::Borrowed(&input[..end]), &input[end..]));
    };
    let mut value = String::new();
    loop {
        let (part, rest) = quoted.split_once('"')?;
        if let Some(escaped) = rest.strip_prefix('"') {
            value.push_str(part);
            value.push('"');
            quoted = escaped;
        } else if value.is_empty() {
            return Some((Cow::Borrowed(part), rest));
        } else {
            value.push_str(part);
            return Some((Cow::Owned(value), rest));
        }
    }
}

fn parse(mut input: &str) -> Option<(usize, Vec<Cow<'_, str>>)> {
    if input.is_empty() || input.len() > MAX_CLIPBOARD_BYTES {
        return None;
    }
    let mut width = 0;
    let mut count = 0;
    let mut cells = Vec::new();
    loop {
        if cells.len() == MAX_CLIPBOARD_CELLS {
            return None;
        }
        let (value, rest) = field(input)?;
        cells.push(value);
        count += 1;
        if let Some(next) = rest.strip_prefix('\t') {
            input = next;
            continue;
        }
        if width == 0 {
            width = count;
        } else if width != count {
            return None;
        }
        if rest.is_empty() {
            break;
        }
        input = rest
            .strip_prefix("\r\n")
            .or_else(|| rest.strip_prefix(['\n', '\r']))?;
        if input.is_empty() {
            break;
        }
        count = 0;
    }
    Some((width, cells))
}

/// Validate and stage all rows before changing the model. Readers retain row identities.
#[doc(hidden)]
pub fn paste_table_text<T: Clone + PartialEq>(
    model: &VecModel<T>,
    stamp: (u64, u64),
    start: usize,
    columns: &[usize],
    text: &str,
    mut assign: impl FnMut(usize, usize, &T, &mut T, &str) -> Option<()>,
) -> Option<bool> {
    let input = TablePasteInput {
        stamp,
        row: start,
        columns,
        text,
    };
    let staged = try_stage_table_text(
        model,
        input,
        Some,
        || Ok(true),
        |row, column, before, after, text, _| {
            Ok::<_, std::convert::Infallible>(assign(row, column, before, after, text))
        },
        |_, _, _| {},
    )
    .unwrap_or_else(|error| match error {})?;
    Some(model.replace_rows_if_current(stamp, staged))
}

pub(crate) fn try_stage_table_text<T: Clone + PartialEq, E>(
    model: &VecModel<T>,
    input: TablePasteInput<'_>,
    source_index: impl Fn(usize) -> Option<usize>,
    mut still_current: impl FnMut() -> Result<bool, E>,
    mut assign: impl FnMut(
        usize,
        usize,
        &T,
        &mut T,
        &str,
        &mut dyn FnMut() -> Result<bool, E>,
    ) -> Result<Option<()>, E>,
    mut capture: impl FnMut(usize, &T, &T),
) -> Result<Option<Vec<(usize, T)>>, E> {
    crate::property::watch_reads(|reads| {
        let mut current_target = || {
            if reads.changed() || model.edit_stamp() != input.stamp {
                return Ok(false);
            }
            Ok(still_current()? && !reads.changed() && model.edit_stamp() == input.stamp)
        };
        if !current_target()? {
            return Ok(None);
        }
        let Some((width, cells)) = parse(input.text) else {
            return Ok(None);
        };
        let Some(columns) = input.columns.get(..width) else {
            return Ok(None);
        };
        let height = cells.len() / width;
        if input
            .row
            .checked_add(height)
            .is_none_or(|end| end > model.len())
        {
            return Ok(None);
        }
        let mut staged = Vec::with_capacity(height);
        for (offset, row) in cells.chunks_exact(width).enumerate() {
            let Some(index) = source_index(input.row + offset) else {
                return Ok(None);
            };
            let Some(current) = model.project_row_untracked(index, T::clone) else {
                return Ok(None);
            };
            let mut next = current.clone();
            for (&column, value) in columns.iter().zip(row) {
                if assign(
                    index,
                    column,
                    &current,
                    &mut next,
                    value,
                    &mut current_target,
                )?
                .is_none()
                    || !current_target()?
                {
                    return Ok(None);
                }
            }
            capture(index, &current, &next);
            if !current_target()? {
                return Ok(None);
            }
            staged.push((index, next));
        }
        Ok(Some(staged))
    })
}

/// Validate parsed numeric text before reading live bounds, stopping if the
/// paste target changes during either read.
#[doc(hidden)]
pub fn try_table_paste_number<T: PartialOrd, E>(
    parsed: Option<T>,
    mut current: impl FnMut() -> Result<bool, E>,
    minimum: impl FnOnce() -> Result<T, E>,
    maximum: impl FnOnce() -> Result<T, E>,
    upper_bound: impl FnOnce(T, T) -> T,
) -> Result<Option<T>, E> {
    let Some(value) = parsed else { return Ok(None) };
    if !current()? {
        return Ok(None);
    }
    let minimum = minimum()?;
    if !current()? {
        return Ok(None);
    }
    let maximum = maximum()?;
    if !current()? {
        return Ok(None);
    }
    let above_minimum = minimum <= value;
    Ok((above_minimum && value <= upper_bound(maximum, minimum)).then_some(value))
}

#[doc(hidden)]
pub fn table_paste_number<T: PartialOrd>(
    parsed: Option<T>,
    mut current: impl FnMut() -> bool,
    minimum: impl FnOnce() -> T,
    maximum: impl FnOnce() -> T,
    upper_bound: impl FnOnce(T, T) -> T,
) -> Option<T> {
    try_table_paste_number(
        parsed,
        || Ok::<_, std::convert::Infallible>(current()),
        || Ok(minimum()),
        || Ok(maximum()),
        upper_bound,
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn table_paste_bool(input: &str) -> Option<bool> {
    let input = input.trim();
    if input == "1" || input.eq_ignore_ascii_case("true") {
        Some(true)
    } else if input == "0" || input.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

#[doc(hidden)]
pub fn table_paste_check_state(input: &str, tristate: bool) -> Option<CheckState> {
    let input = input.trim();
    if input.eq_ignore_ascii_case("checked") {
        Some(CheckState::Checked)
    } else if input.eq_ignore_ascii_case("unchecked") {
        Some(CheckState::Unchecked)
    } else if tristate && input.eq_ignore_ascii_case("partiallychecked") {
        Some(CheckState::PartiallyChecked)
    } else {
        table_paste_bool(input).map(CheckState::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rectangular_tsv_and_quoted_cells() {
        let (width, cells) = parse("α\t\"a\t\"\"b\"\"\r\nc\"\r\nβ\t\r\n").unwrap();
        assert_eq!(width, 2);
        assert_eq!(cells, ["α", "a\t\"b\"\r\nc", "β", ""]);
        assert_eq!(parse("\"\"").unwrap().1, [""]);
        assert_eq!(parse("first\n\"\"").unwrap().1, ["first", ""]);
        for input in ["", "a\tb\nc", "\"unterminated", "\"a\"suffix"] {
            assert!(parse(input).is_none(), "accepted {input:?}");
        }
    }

    #[test]
    fn batch_paste_invalidates_only_changed_rows_and_preserves_structure() {
        use crate::Property;
        use std::cell::Cell;

        let model = VecModel::from(vec![1, 2, 3]);
        let untouched = Property::default();
        let changed = Property::default();
        let stable_reads = Cell::new(0);
        let stable = || {
            untouched.get(|| {
                stable_reads.set(stable_reads.get() + 1);
                model.row(0).unwrap()
            })
        };
        let edited = || changed.get(|| model.row(1).unwrap());
        let structure = model.structure_stamp();
        let stamp = model.edit_stamp();
        assert_eq!((stable(), edited()), (1, 2));
        assert_eq!(
            paste_table_text(&model, stamp, 0, &[0], "1\n20", |_, _, _, next, text| {
                *next = text.parse().ok()?;
                Some(())
            }),
            Some(true),
        );
        assert_eq!(model.structure_stamp(), structure);
        assert_ne!(model.edit_stamp(), stamp);
        assert_eq!((stable(), edited()), (1, 20));
        assert_eq!(stable_reads.get(), 1);
    }
}
