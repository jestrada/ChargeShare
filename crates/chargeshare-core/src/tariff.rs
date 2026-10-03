use std::fmt;

use crate::money::UsdRate;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TariffError {
    InvalidVersion,
    InvalidWindow,
    OverlappingWindows,
}

impl fmt::Display for TariffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for TariffError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateWindow {
    version: String,
    start_time: i64,
    end_time: i64,
    rate: UsdRate,
}

impl RateWindow {
    pub fn new(
        version: &str,
        start_time: i64,
        end_time: i64,
        rate: UsdRate,
    ) -> Result<Self, TariffError> {
        if version.is_empty()
            || version.len() > 64
            || !version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(TariffError::InvalidVersion);
        }
        if start_time >= end_time {
            return Err(TariffError::InvalidWindow);
        }
        Ok(Self {
            version: version.to_owned(),
            start_time,
            end_time,
            rate,
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn start_time(&self) -> i64 {
        self.start_time
    }

    pub fn end_time(&self) -> i64 {
        self.end_time
    }

    pub fn rate(&self) -> UsdRate {
        self.rate
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateSchedule {
    windows: Vec<RateWindow>,
}

impl RateSchedule {
    pub fn new(mut windows: Vec<RateWindow>) -> Result<Self, TariffError> {
        windows.sort_by_key(RateWindow::start_time);
        if windows
            .windows(2)
            .any(|pair| pair[0].end_time > pair[1].start_time)
        {
            return Err(TariffError::OverlappingWindows);
        }
        Ok(Self { windows })
    }

    pub fn windows(&self) -> &[RateWindow] {
        &self.windows
    }

    pub(crate) fn at(&self, time: i64) -> Option<&RateWindow> {
        let index = self
            .windows
            .partition_point(|window| window.start_time <= time);
        index
            .checked_sub(1)
            .and_then(|index| self.windows.get(index))
            .filter(|window| time < window.end_time)
    }
}

#[cfg(test)]
mod tests {
    use super::{RateSchedule, RateWindow, TariffError};
    use crate::money::UsdRate;

    fn window(version: &str, start: i64, end: i64) -> Result<RateWindow, TariffError> {
        RateWindow::new(version, start, end, UsdRate::parse_per_kwh("0.2").unwrap())
    }

    #[test]
    fn invalid_versions_and_nonpositive_windows_fail_safely() {
        for invalid in ["", "unsafe value", "fictional@example.invalid"] {
            assert_eq!(window(invalid, 0, 1), Err(TariffError::InvalidVersion));
        }
        assert_eq!(
            window(&"a".repeat(65), 0, 1),
            Err(TariffError::InvalidVersion)
        );
        for (start, end) in [(0, 0), (1, 0), (i64::MAX, i64::MIN)] {
            assert_eq!(
                window("fictional-v1", start, end),
                Err(TariffError::InvalidWindow)
            );
        }
    }

    #[test]
    fn windows_are_half_open_sorted_and_nonoverlapping() {
        let first = window("fictional-v1", -10, 0).unwrap();
        let second = window("fictional-v2", 0, 10).unwrap();
        let schedule = RateSchedule::new(vec![second.clone(), first.clone()]).unwrap();
        assert_eq!(schedule.windows(), &[first.clone(), second.clone()]);
        assert_eq!(schedule.at(-10), Some(&first));
        assert_eq!(schedule.at(0), Some(&second));
        assert_eq!(schedule.at(-11), None);
        assert_eq!(schedule.at(10), None);
        assert_eq!(
            RateSchedule::new(vec![first.clone(), first]),
            Err(TariffError::OverlappingWindows)
        );
        assert_eq!(
            RateSchedule::new(vec![
                window("a", 0, 10).unwrap(),
                window("b", 9, 20).unwrap()
            ]),
            Err(TariffError::OverlappingWindows)
        );
        assert_eq!(RateSchedule::new(vec![]).unwrap().at(0), None);
        assert!(RateSchedule::new(vec![window("a", i64::MIN, i64::MAX).unwrap()]).is_ok());
    }
}
