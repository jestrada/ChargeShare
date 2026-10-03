use std::fmt;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Energy(u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CounterProblem {
    Invalid,
    Negative,
    UnsupportedPrecision,
    Overflow,
}

impl Energy {
    pub const ZERO: Self = Self(0);

    pub fn parse_kwh(input: &str) -> Result<Self, CounterProblem> {
        if input.starts_with('-') {
            return Err(CounterProblem::Negative);
        }
        let (whole, fraction) = match input.split_once('.') {
            Some((whole, fraction)) => (whole, Some(fraction)),
            None => (input, None),
        };
        if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
            return Err(CounterProblem::Invalid);
        }
        let fraction = fraction.unwrap_or("");
        if (input.contains('.') && fraction.is_empty())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(CounterProblem::Invalid);
        }
        if fraction.len() > 6 {
            return Err(CounterProblem::UnsupportedPrecision);
        }
        let fraction_digits = fraction.len() as u32;
        let whole: u64 = whole.parse().map_err(|_| CounterProblem::Overflow)?;
        let fraction: u64 = if fraction.is_empty() {
            0
        } else {
            fraction.parse().map_err(|_| CounterProblem::Overflow)?
        };
        whole
            .checked_mul(1_000_000)
            .and_then(|v| v.checked_add(fraction * 10_u64.pow(6 - fraction_digits)))
            .map(Self)
            .ok_or(CounterProblem::Overflow)
    }

    pub(crate) fn checked_delta_from(self, previous: Self) -> Option<Self> {
        self.0.checked_sub(previous.0).map(Self)
    }

    pub(crate) fn micro_kwh(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, CounterProblem> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(CounterProblem::Overflow)
    }
}

impl fmt::Display for Energy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:06} kWh", self.0 / 1_000_000, self.0 % 1_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::Energy;

    #[test]
    fn checked_delta_preserves_exact_nonnegative_energy_at_representation_limits() {
        let maximum = Energy::parse_kwh("18446744073709.551615").unwrap();
        let one_unit = Energy::parse_kwh("0.000001").unwrap();
        let below_maximum = Energy::parse_kwh("18446744073709.551614").unwrap();

        assert_eq!(maximum.checked_delta_from(Energy::ZERO), Some(maximum));
        assert_eq!(maximum.checked_delta_from(maximum), Some(Energy::ZERO));
        assert_eq!(maximum.checked_delta_from(below_maximum), Some(one_unit));
    }

    #[test]
    fn checked_delta_rejects_counter_rollback_without_wrapping() {
        let one_unit = Energy::parse_kwh("0.000001").unwrap();
        let maximum = Energy::parse_kwh("18446744073709.551615").unwrap();

        assert_eq!(Energy::ZERO.checked_delta_from(one_unit), None);
        assert_eq!(one_unit.checked_delta_from(maximum), None);
    }
}
