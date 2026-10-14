use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, NaiveDate, TimeZone};
use chrono_tz::America::Los_Angeles;

use crate::energy::Energy;
use crate::money::{ExactUsd, MoneyError};
use crate::session::{ChargerClassification, QualityFlag};
use crate::time_of_use::{TimeOfUsePricing, TimeOfUseSession};

pub const CHARGING_PERIODS: [&str; 3] = ["off_peak", "part_peak", "peak"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonthlyHistoryError {
    Calendar,
    EnergyOverflow,
    Money(MoneyError),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChargingTotal {
    pub energy: Energy,
    pub exact_cost: ExactUsd,
}

impl ChargingTotal {
    fn add(&mut self, other: Self) -> Result<(), MonthlyHistoryError> {
        self.energy = self
            .energy
            .checked_add(other.energy)
            .map_err(|_| MonthlyHistoryError::EnergyOverflow)?;
        self.exact_cost = self
            .exact_cost
            .checked_add(other.exact_cost)
            .map_err(MonthlyHistoryError::Money)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChargingEvidence {
    pub quoted_sessions: usize,
    pub held_sessions: usize,
    pub other_sessions: usize,
    pub partial_sessions: usize,
    pub complete_zero_sessions: usize,
    pub quality_flags: BTreeSet<QualityFlag>,
}

impl ChargingEvidence {
    fn include(&mut self, session: &TimeOfUseSession) {
        if session.quote.is_ok() {
            self.quoted_sessions += 1;
            self.partial_sessions += usize::from(!session.session.quality_flags.is_empty());
            self.complete_zero_sessions += usize::from(
                session.session.observed_ac == Energy::ZERO
                    && session.session.quality_flags.is_empty(),
            );
        } else if session.session.classification == ChargerClassification::OtherCharger {
            self.other_sessions += 1;
        } else {
            self.held_sessions += 1;
        }
        self.quality_flags.extend(&session.session.quality_flags);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChargingDay {
    pub local_date: NaiveDate,
    pub future: bool,
    pub periods: [ChargingTotal; 3],
    pub total: ChargingTotal,
    pub evidence: ChargingEvidence,
}

impl ChargingDay {
    fn new(local_date: NaiveDate, today: NaiveDate) -> Self {
        Self {
            local_date,
            future: local_date > today,
            periods: [ChargingTotal::default(); 3],
            total: ChargingTotal::default(),
            evidence: ChargingEvidence::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChargingMonth {
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
    pub days: Vec<ChargingDay>,
    pub periods: [ChargingTotal; 3],
    pub total: ChargingTotal,
    pub evidence: ChargingEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonthlyHistory {
    pub today: NaiveDate,
    pub months: Vec<ChargingMonth>,
    pub evidence: ChargingEvidence,
    pub undated_sessions: usize,
}

fn local_date(seconds: i64) -> Result<NaiveDate, MonthlyHistoryError> {
    Los_Angeles
        .timestamp_opt(seconds, 0)
        .single()
        .map(|time| time.date_naive())
        .ok_or(MonthlyHistoryError::Calendar)
}

fn session_dates(session: &TimeOfUseSession) -> Result<BTreeSet<NaiveDate>, MonthlyHistoryError> {
    if let Ok(quote) = &session.quote
        && !quote.slices.is_empty()
    {
        return quote
            .slices
            .iter()
            .map(|slice| slice_date(&slice.local_date))
            .collect();
    }
    let evidence = &session.session;
    let counter_first = evidence
        .observed_window
        .as_ref()
        .and_then(|window| window.first_time);
    let counter_last = evidence
        .observed_window
        .as_ref()
        .and_then(|window| window.last_time);
    let (first, last) = if session.quote.is_ok() {
        (
            counter_first.or(evidence.start_time),
            counter_last.or(evidence.end_time),
        )
    } else {
        (
            evidence.start_time.or(counter_first),
            evidence.end_time.or(counter_last),
        )
    };
    let Some(first) = first.or(last) else {
        return Ok(BTreeSet::new());
    };
    let last = last.unwrap_or(first);
    let mut date = local_date(first.min(last))?;
    let last_date = local_date(if last > first { last - 1 } else { first })?;
    let mut dates = BTreeSet::new();
    while date <= last_date {
        dates.insert(date);
        date = date.succ_opt().ok_or(MonthlyHistoryError::Calendar)?;
    }
    Ok(dates)
}

fn slice_date(date: &str) -> Result<NaiveDate, MonthlyHistoryError> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| MonthlyHistoryError::Calendar)
}

impl TimeOfUsePricing {
    pub fn monthly_history(&self, now: i64) -> Result<MonthlyHistory, MonthlyHistoryError> {
        let today = local_date(now)?;
        let mut days = BTreeMap::<NaiveDate, ChargingDay>::new();
        let mut month_evidence = BTreeMap::<NaiveDate, ChargingEvidence>::new();
        month_evidence.insert(today.with_day(1).unwrap(), ChargingEvidence::default());
        let mut evidence = ChargingEvidence::default();
        let mut undated_sessions = 0;
        for session in &self.sessions {
            evidence.include(session);
            let dates = session_dates(session)?;
            undated_sessions += usize::from(dates.is_empty());
            let mut session_months = BTreeSet::new();
            for date in dates {
                days.entry(date)
                    .or_insert_with(|| ChargingDay::new(date, today))
                    .evidence
                    .include(session);
                session_months.insert(date.with_day(1).unwrap());
            }
            for month in session_months {
                month_evidence.entry(month).or_default().include(session);
            }
            if let Ok(quote) = &session.quote {
                for slice in &quote.slices {
                    let date = slice_date(&slice.local_date)?;
                    let period = CHARGING_PERIODS
                        .iter()
                        .position(|period| *period == slice.period)
                        .ok_or(MonthlyHistoryError::Calendar)?;
                    let total = ChargingTotal {
                        energy: slice.energy,
                        exact_cost: slice.exact_cost,
                    };
                    let day = days.get_mut(&date).ok_or(MonthlyHistoryError::Calendar)?;
                    day.periods[period].add(total)?;
                    day.total.add(total)?;
                }
            }
        }
        let mut months = Vec::new();
        for (first_date, evidence) in month_evidence.into_iter().rev() {
            let next = first_date
                .checked_add_months(chrono::Months::new(1))
                .ok_or(MonthlyHistoryError::Calendar)?;
            let last_date = next.pred_opt().ok_or(MonthlyHistoryError::Calendar)?;
            let mut month = ChargingMonth {
                first_date,
                last_date,
                days: Vec::new(),
                periods: [ChargingTotal::default(); 3],
                total: ChargingTotal::default(),
                evidence,
            };
            let mut date = first_date;
            while date < next {
                let day = days
                    .remove(&date)
                    .unwrap_or_else(|| ChargingDay::new(date, today));
                for (total, period) in month.periods.iter_mut().zip(day.periods) {
                    total.add(period)?;
                }
                month.total.add(day.total)?;
                month.days.push(day);
                date = date.succ_opt().ok_or(MonthlyHistoryError::Calendar)?;
            }
            months.push(month);
        }
        let current_month = today.with_day(1).unwrap();
        months.sort_by_key(|month| {
            (
                month.first_date != current_month,
                std::cmp::Reverse(month.first_date),
            )
        });
        Ok(MonthlyHistory {
            today,
            months,
            evidence,
            undated_sessions,
        })
    }
}
