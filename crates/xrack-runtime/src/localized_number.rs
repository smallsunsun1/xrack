use std::fmt;

use icu_decimal::{
    DecimalFormatter,
    input::{Decimal, SignDisplay},
    parts,
};
use icu_locale_core::Locale;
use writeable::{Part, PartsWrite, Writeable};

#[derive(Default)]
struct DecimalParts {
    text: String,
    decimal: String,
    group: String,
    sign_prefix: String,
    sign_suffix: String,
}

impl fmt::Write for DecimalParts {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.text.push_str(text);
        Ok(())
    }
}

impl PartsWrite for DecimalParts {
    type SubPartsWrite = Self;

    fn with_part(
        &mut self,
        part: Part,
        mut write: impl FnMut(&mut Self) -> fmt::Result,
    ) -> fmt::Result {
        let start = self.text.len();
        write(self)?;
        let added = &self.text[start..];
        match part {
            parts::DECIMAL => self.decimal.push_str(added),
            parts::GROUP if self.group.is_empty() => self.group.push_str(added),
            parts::MINUS_SIGN | parts::PLUS_SIGN if start == 0 => self.sign_prefix.push_str(added),
            parts::MINUS_SIGN | parts::PLUS_SIGN => self.sign_suffix.push_str(added),
            _ => {}
        }
        Ok(())
    }
}

impl DecimalParts {
    fn from_decimal(formatter: &DecimalFormatter, decimal: &Decimal) -> Self {
        let mut parts = Self::default();
        formatter
            .format(decimal)
            .write_to_parts(&mut parts)
            .expect("writing into a String");
        parts
    }

    fn sign(&self) -> (&str, &str) {
        (&self.sign_prefix, &self.sign_suffix)
    }
}

pub(crate) struct LocalizedNumber {
    formatter: DecimalFormatter,
    decimal: String,
    grouping: Option<(String, usize, usize)>,
    digits: [char; 10],
    minus: (String, String),
    plus: (String, String),
}

impl LocalizedNumber {
    pub(crate) fn new(language: &str) -> Self {
        let locale = language.parse::<Locale>().unwrap_or(Locale::UNKNOWN);
        let formatter = DecimalFormatter::try_new(locale.into(), Default::default())
            .unwrap_or_else(|_| {
                DecimalFormatter::try_new(Default::default(), Default::default())
                    .expect("ICU includes root decimal data")
            });
        // Read the digits, punctuation and grouping widths from the same public
        // ICU formatter used for output, including the locale's numbering system.
        let sample: Decimal = "1234567890.1".parse().expect("valid decimal sample");
        let mut positive: Decimal = 1.into();
        positive.apply_sign_display(SignDisplay::Always);
        let plus = DecimalParts::from_decimal(&formatter, &positive);
        let minus = DecimalParts::from_decimal(&formatter, &(-1).into());
        let sample = DecimalParts::from_decimal(&formatter, &sample);
        let (integer, _) = sample
            .text
            .split_once(&sample.decimal)
            .expect("decimal part");
        let grouping = (!sample.group.is_empty()).then(|| {
            let mut groups = integer.rsplit(&sample.group);
            let primary = groups.next().unwrap().chars().count();
            let secondary = groups.next().map_or(primary, |group| group.chars().count());
            (sample.group.clone(), primary, secondary)
        });
        let integer = if sample.group.is_empty() {
            integer.to_owned()
        } else {
            integer.replace(&sample.group, "")
        };
        let mut digits = ['0'; 10];
        for (index, digit) in integer.chars().enumerate() {
            digits[(index + 1) % 10] = digit;
        }
        let owned_sign = |parts: &DecimalParts| {
            let (prefix, suffix) = parts.sign();
            (prefix.to_owned(), suffix.to_owned())
        };
        Self {
            formatter,
            decimal: sample.decimal,
            grouping,
            digits,
            minus: owned_sign(&minus),
            plus: owned_sign(&plus),
        }
    }

    pub(crate) fn format(&self, ascii: String) -> String {
        match ascii.parse::<Decimal>() {
            Ok(decimal) => self.formatter.format_to_string(&decimal),
            Err(_) => ascii, // Non-finite float spellings have no ICU Decimal representation.
        }
    }

    pub(crate) fn parse(&self, text: &str) -> Option<f64> {
        let text = text.trim();
        let strip_sign = |(prefix, suffix): &(String, String)| {
            if prefix.is_empty() && suffix.is_empty() {
                return None;
            }
            text.strip_prefix(prefix)?.strip_suffix(suffix)
        };
        let (negative, text) = if let Some(rest) =
            strip_sign(&self.minus).or_else(|| text.strip_prefix(['-', '−']))
        {
            (true, rest)
        } else {
            (
                false,
                strip_sign(&self.plus)
                    .or_else(|| text.strip_prefix('+'))
                    .unwrap_or(text),
            )
        };
        let (integer, fraction) = text
            .split_once(&self.decimal)
            .map_or((text, None), |(integer, fraction)| {
                (integer, Some(fraction))
            });

        let mut ascii = String::with_capacity(text.len());
        if negative {
            ascii.push('-');
        }
        if let Some((separator, primary, secondary)) = &self.grouping
            && integer.contains(separator)
        {
            let groups = integer.split(separator);
            let count = groups.clone().count();
            for (index, group) in groups.enumerate() {
                let len = group.chars().count();
                let valid = if index == 0 {
                    (1..=*secondary).contains(&len)
                } else if index + 1 == count {
                    len == *primary
                } else {
                    len == *secondary
                };
                if !valid {
                    return None;
                }
                self.append_digits(group, &mut ascii)?;
            }
        } else {
            self.append_digits(integer, &mut ascii)?;
        }
        if let Some(fraction) = fraction {
            ascii.push('.');
            self.append_digits(fraction, &mut ascii)?;
        }
        ascii.parse::<f64>().ok().filter(|value| value.is_finite())
    }

    fn append_digits(&self, text: &str, output: &mut String) -> Option<()> {
        for digit in text.chars() {
            if digit.is_ascii_digit() {
                output.push(digit);
            } else {
                let index = self
                    .digits
                    .iter()
                    .position(|localized| *localized == digit)?;
                output.push(char::from(b'0' + index as u8));
            }
        }
        Some(())
    }
}
