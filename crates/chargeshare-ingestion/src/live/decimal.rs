use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::{NormalizedCounter, SafeCounterProblem};

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct DecimalValue {
    digits: String,
    exponent: i64,
}

impl DecimalValue {
    pub(super) fn parse(input: &str) -> Result<Self, SafeCounterProblem> {
        let invalid = SafeCounterProblem::Invalid;
        if input.starts_with('-') {
            return Err(SafeCounterProblem::Negative);
        }
        let mut parts = input.split(['e', 'E']);
        let mantissa = parts.next().ok_or(invalid)?;
        let exponent = match parts.next() {
            Some(exponent) => {
                let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
                if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid);
                }
                exponent
                    .parse::<i64>()
                    .map_err(|_| SafeCounterProblem::Overflow)?
            }
            None => 0,
        };
        if parts.next().is_some() {
            return Err(invalid);
        }
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || (mantissa.contains('.') && fraction.is_empty())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid);
        }
        let mut digits = format!("{whole}{fraction}")
            .trim_start_matches('0')
            .to_owned();
        if digits.is_empty() {
            return Ok(Self {
                digits: "0".to_owned(),
                exponent: 0,
            });
        }
        let mut exponent = exponent
            .checked_sub(fraction.len() as i64)
            .ok_or(SafeCounterProblem::Overflow)?;
        while digits.ends_with('0') {
            digits.pop();
            exponent = exponent
                .checked_add(1)
                .ok_or(SafeCounterProblem::Overflow)?;
        }
        Ok(Self { digits, exponent })
    }

    pub(super) fn is_positive(&self) -> bool {
        self.digits != "0"
    }

    pub(super) fn compare(&self, other: &Self) -> Ordering {
        match (self.is_positive(), other.is_positive()) {
            (false, false) => return Ordering::Equal,
            (false, true) => return Ordering::Less,
            (true, false) => return Ordering::Greater,
            _ => {}
        }
        let magnitude = i128::from(self.exponent) + self.digits.len() as i128;
        let other_magnitude = i128::from(other.exponent) + other.digits.len() as i128;
        magnitude.cmp(&other_magnitude).then_with(|| {
            let length = self.digits.len().max(other.digits.len());
            self.digits
                .bytes()
                .chain(std::iter::repeat(b'0'))
                .take(length)
                .cmp(
                    other
                        .digits
                        .bytes()
                        .chain(std::iter::repeat(b'0'))
                        .take(length),
                )
        })
    }

    fn micro_kwh_half_up(&self) -> Result<u64, SafeCounterProblem> {
        if !self.is_positive() {
            return Ok(0);
        }
        let places = self.digits.len() as i128 + i128::from(self.exponent) + 6;
        if places < 0 {
            return Ok(0);
        }
        if places > 20 {
            return Err(SafeCounterProblem::Overflow);
        }
        let places = places as usize;
        let mut units = 0_u64;
        for digit in self
            .digits
            .bytes()
            .chain(std::iter::repeat(b'0'))
            .take(places)
        {
            units = units
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(digit - b'0')))
                .ok_or(SafeCounterProblem::Overflow)?;
        }
        if self
            .digits
            .as_bytes()
            .get(places)
            .is_some_and(|digit| *digit >= b'5')
        {
            units = units.checked_add(1).ok_or(SafeCounterProblem::Overflow)?;
        }
        Ok(units)
    }
}

pub(super) fn normalize_counter(input: &str) -> NormalizedCounter {
    match DecimalValue::parse(input).and_then(|value| value.micro_kwh_half_up()) {
        Ok(units) => NormalizedCounter::Valid {
            kwh: format!("{}.{:06}", units / 1_000_000, units % 1_000_000),
        },
        Err(reason) => NormalizedCounter::Rejected { reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_round_once_in_decimal_including_exponents_and_representation_edges() {
        for (input, expected) in [
            ("1.2345675", "1.234568"),
            ("1.234567499999999999999999999999", "1.234567"),
            ("12345675e-7", "1.234568"),
            ("5E-7", "0.000001"),
            ("4.999999999999999999999e-7", "0.000000"),
            ("1e-100", "0.000000"),
            ("9.9999995", "10.000000"),
            ("18446744073709.5516154", "18446744073709.551615"),
        ] {
            assert!(
                normalize_counter(input)
                    == NormalizedCounter::Valid {
                        kwh: expected.to_owned()
                    }
            );
        }
        for input in [
            "", "NaN", "Infinity", "1e", "1e+-2", "1e2e3", "1.", " 1", "+1", "1_0",
        ] {
            assert!(matches!(
                normalize_counter(input),
                NormalizedCounter::Rejected { .. }
            ));
        }
        for input in [
            "-1",
            "-0",
            "1e100",
            "18446744073709.5516155",
            "18446744073710",
        ] {
            assert!(matches!(
                normalize_counter(input),
                NormalizedCounter::Rejected { .. }
            ));
        }
    }

    #[test]
    fn odometer_comparison_keeps_sub_micro_precision() {
        let parse = |text| DecimalValue::parse(text).unwrap();
        assert_eq!(parse("100.00").compare(&parse("1e2")), Ordering::Equal);
        assert_eq!(
            parse("100.00000001").compare(&parse("100")),
            Ordering::Greater
        );
        assert_eq!(parse("0").compare(&parse("1e-100")), Ordering::Less);
    }
}
