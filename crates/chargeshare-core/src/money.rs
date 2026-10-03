use std::fmt;

use crate::energy::Energy;

const RATE_SCALE: u64 = 10_000_000_000;
const COST_SCALE: u128 = 10_000_000_000_000_000;
const UNITS_PER_CENT: u128 = COST_SCALE / 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoneyError {
    InvalidRate,
    UnsupportedRatePrecision,
    RateOverflow,
    CostOverflow,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MoneyError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UsdRate(u64);

impl UsdRate {
    pub fn parse_per_kwh(input: &str) -> Result<Self, MoneyError> {
        let (whole, fraction) = match input.split_once('.') {
            Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
            Some(_) => return Err(MoneyError::InvalidRate),
            None => (input, ""),
        };
        if whole.is_empty()
            || !whole.bytes().all(|digit| digit.is_ascii_digit())
            || !fraction.bytes().all(|digit| digit.is_ascii_digit())
        {
            return Err(MoneyError::InvalidRate);
        }
        if fraction.len() > 10 {
            return Err(MoneyError::UnsupportedRatePrecision);
        }
        let whole: u64 = whole.parse().map_err(|_| MoneyError::RateOverflow)?;
        let fractional_units = if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<u64>()
                .map_err(|_| MoneyError::RateOverflow)?
                * 10_u64.pow(10 - fraction.len() as u32)
        };
        whole
            .checked_mul(RATE_SCALE)
            .and_then(|units| units.checked_add(fractional_units))
            .map(Self)
            .ok_or(MoneyError::RateOverflow)
    }

    pub fn cost_for(self, energy: Energy) -> ExactUsd {
        ExactUsd(u128::from(self.0) * u128::from(energy.micro_kwh()))
    }
}

impl fmt::Display for UsdRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{:010} USD/kWh",
            self.0 / RATE_SCALE,
            self.0 % RATE_SCALE
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct ExactUsd(u128);

impl ExactUsd {
    pub const ZERO: Self = Self(0);

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(MoneyError::CostOverflow)
    }

    pub fn round_half_up_to_cents(self) -> UsdCents {
        let cents = self.0 / UNITS_PER_CENT;
        let round_up = self.0 % UNITS_PER_CENT >= UNITS_PER_CENT / 2;
        UsdCents(cents + u128::from(round_up))
    }
}

impl fmt::Display for ExactUsd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:016} USD", self.0 / COST_SCALE, self.0 % COST_SCALE)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct UsdCents(u128);

impl UsdCents {
    pub fn as_cents(self) -> u128 {
        self.0
    }
}

impl fmt::Display for UsdCents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02} USD", self.0 / 100, self.0 % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExactUsd, MoneyError, UsdRate};
    use crate::energy::Energy;

    #[test]
    fn decimal_rates_retain_ten_places_and_reject_unsafe_input_without_echoing() {
        assert_eq!(
            UsdRate::parse_per_kwh("000.1234567890")
                .unwrap()
                .to_string(),
            "0.1234567890 USD/kWh"
        );
        for invalid in [
            "", "-1", "-0", "+1", " 1", "1 ", "NaN", "inf", "1e2", ".1", "1.", "1.2.3", "１",
        ] {
            assert_eq!(
                UsdRate::parse_per_kwh(invalid),
                Err(MoneyError::InvalidRate)
            );
        }
        assert_eq!(
            UsdRate::parse_per_kwh("0.12345678901"),
            Err(MoneyError::UnsupportedRatePrecision)
        );
        for overflow in [
            "1844674407.3709551616",
            "1844674408",
            "18446744073709551616",
        ] {
            assert_eq!(
                UsdRate::parse_per_kwh(overflow),
                Err(MoneyError::RateOverflow)
            );
        }
        assert_eq!(MoneyError::InvalidRate.to_string(), "InvalidRate");
    }

    #[test]
    fn multiplication_keeps_the_smallest_unit_and_full_representation_range() {
        let smallest = UsdRate::parse_per_kwh("0.0000000001")
            .unwrap()
            .cost_for(Energy::parse_kwh("0.000001").unwrap());
        assert_eq!(smallest.to_string(), "0.0000000000000001 USD");
        let maximum = UsdRate::parse_per_kwh("1844674407.3709551615")
            .unwrap()
            .cost_for(Energy::parse_kwh("18446744073709.551615").unwrap());
        assert_eq!(maximum.0, u128::from(u64::MAX) * u128::from(u64::MAX));
        assert_eq!(maximum.checked_add(maximum), Err(MoneyError::CostOverflow));
        assert_eq!(maximum.checked_add(ExactUsd::ZERO), Ok(maximum));
    }

    #[test]
    fn half_up_rounding_uses_exact_cents_and_does_not_overflow() {
        let kwh = Energy::parse_kwh("1").unwrap();
        for (rate, cents) in [
            ("0", 0),
            ("0.0049999999", 0),
            ("0.005", 1),
            ("0.0149999999", 1),
            ("0.015", 2),
            ("0.995", 100),
        ] {
            assert_eq!(
                UsdRate::parse_per_kwh(rate)
                    .unwrap()
                    .cost_for(kwh)
                    .round_half_up_to_cents()
                    .as_cents(),
                cents
            );
        }
        assert_eq!(
            ExactUsd(u128::MAX).round_half_up_to_cents().as_cents(),
            3_402_823_669_209_384_634_633_746
        );
    }
}
