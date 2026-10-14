use std::collections::BTreeSet;

use chrono::{Datelike, LocalResult, NaiveDate, NaiveTime, Offset, TimeZone, Timelike, Weekday};
use chrono_tz::America::Los_Angeles;

use crate::energy::Energy;
use crate::money::{ExactUsd, UsdCents, UsdRate};
use crate::pricing::{PricingError, PricingHold};
use crate::session::{ReconstructedSession, Session};
use crate::tariff::TariffError;

pub const TIMEZONE_RULES_VERSION: &str = chrono_tz::IANA_TZDB_VERSION;
pub const TOU_CALCULATION_VERSION: &str = "tou-linear-micro-kwh-v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Season {
    Summer,
    Winter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DayType {
    Weekday,
    Weekend,
    Holiday,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatePeriod {
    pub season: Season,
    pub period: String,
    pub start_second: u32,
    pub end_second: u32,
    pub rate: UsdRate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateModelDefinition {
    pub id: String,
    pub plan: String,
    pub source_label: String,
    pub source_url: String,
    pub effective_from: String,
    pub effective_until: Option<String>,
    pub timezone: String,
    pub timezone_rules_version: String,
    pub season_rules_version: String,
    pub day_type_rules_version: String,
    pub holidays: Vec<String>,
    pub periods: Vec<RatePeriod>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateModelVersion {
    definition: RateModelDefinition,
    from: NaiveDate,
    until: Option<NaiveDate>,
    holidays: BTreeSet<NaiveDate>,
}

impl RateModelVersion {
    pub fn new(mut definition: RateModelDefinition) -> Result<Self, TariffError> {
        for alias in [&definition.id, &definition.plan] {
            if crate::identity::OwnerId::new(alias).is_err() {
                return Err(TariffError::InvalidModel);
            }
        }
        if definition.timezone != "America/Los_Angeles"
            || definition.timezone_rules_version != TIMEZONE_RULES_VERSION
            || definition.season_rules_version != "june-september-v1"
            || definition.day_type_rules_version != "all-days-v1"
        {
            return Err(TariffError::IncompatibleRules);
        }
        if definition.source_label.is_empty()
            || definition.source_label.len() > 160
            || !definition
                .source_label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b" -_()./&".contains(&byte))
            || definition.source_url
                != "https://www.pge.com/tariffs/assets/pdf/tariffbook/ELEC_SCHEDS_EV2%20%28Sch%29.pdf"
            || definition.periods.len() > 48
            || definition.holidays.len() > 512
        {
            return Err(TariffError::InvalidModel);
        }
        let from = local_date(&definition.effective_from)?;
        let until = definition
            .effective_until
            .as_deref()
            .map(local_date)
            .transpose()?;
        if until.is_some_and(|until| until <= from) {
            return Err(TariffError::InvalidWindow);
        }
        definition.holidays.sort();
        let holidays: BTreeSet<_> = definition
            .holidays
            .iter()
            .map(|date| local_date(date))
            .collect::<Result<_, _>>()?;
        if holidays.len() != definition.holidays.len() {
            return Err(TariffError::InvalidModel);
        }
        definition
            .periods
            .sort_by_key(|period| (period.season, period.start_second));
        for season in [Season::Summer, Season::Winter] {
            let mut previous_end = 0;
            for period in definition
                .periods
                .iter()
                .filter(|period| period.season == season)
            {
                if period.start_second >= period.end_second
                    || period.end_second > 86_400
                    || crate::identity::OwnerId::new(&period.period).is_err()
                {
                    return Err(TariffError::InvalidModel);
                }
                if period.start_second < previous_end {
                    return Err(TariffError::OverlappingWindows);
                }
                previous_end = period.end_second;
            }
        }
        Ok(Self {
            definition,
            from,
            until,
            holidays,
        })
    }

    pub fn definition(&self) -> &RateModelDefinition {
        &self.definition
    }

    fn covers(&self, date: NaiveDate) -> bool {
        self.from <= date && self.until.is_none_or(|until| date < until)
    }
}

fn local_date(input: &str) -> Result<NaiveDate, TariffError> {
    let date =
        NaiveDate::parse_from_str(input, "%Y-%m-%d").map_err(|_| TariffError::InvalidModel)?;
    if date.format("%Y-%m-%d").to_string() != input {
        return Err(TariffError::InvalidModel);
    }
    Ok(date)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateModelHistory {
    versions: Vec<RateModelVersion>,
}

impl RateModelHistory {
    pub fn new(mut versions: Vec<RateModelVersion>) -> Result<Self, TariffError> {
        versions.sort_by_key(|version| version.from);
        let ids: BTreeSet<_> = versions
            .iter()
            .map(|version| &version.definition.id)
            .collect();
        if versions.len() > 64 || ids.len() != versions.len() {
            return Err(TariffError::InvalidModel);
        }
        if versions
            .windows(2)
            .any(|pair| pair[0].until.is_none_or(|until| until > pair[1].from))
        {
            return Err(TariffError::OverlappingWindows);
        }
        Ok(Self { versions })
    }

    pub fn versions(&self) -> &[RateModelVersion] {
        &self.versions
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RatedSlice {
    pub start_time: i64,
    pub end_time: i64,
    pub local_date: String,
    pub local_start: String,
    pub local_end: String,
    pub utc_offset_seconds: i32,
    pub season: Season,
    pub day_type: DayType,
    pub period: String,
    pub model_version: String,
    pub energy: Energy,
    pub rate: UsdRate,
    pub exact_cost: ExactUsd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeOfUseQuote {
    pub slices: Vec<RatedSlice>,
    pub exact_cost: ExactUsd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeOfUseSession {
    pub session: Session,
    pub quote: Result<TimeOfUseQuote, PricingHold>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeOfUsePricing {
    pub sessions: Vec<TimeOfUseSession>,
    pub priced_energy: Energy,
    pub exact_subtotal: ExactUsd,
    pub rounded_subtotal: UsdCents,
}

fn offset_at(time: i64) -> Option<i32> {
    Some(
        Los_Angeles
            .timestamp_opt(time, 0)
            .single()?
            .offset()
            .fix()
            .local_minus_utc(),
    )
}

fn day_boundaries(date: NaiveDate, periods: &[RatePeriod]) -> Option<BTreeSet<i64>> {
    let next = date.succ_opt()?;
    let start = Los_Angeles
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .single()?
        .timestamp();
    let end = Los_Angeles
        .from_local_datetime(&next.and_hms_opt(0, 0, 0)?)
        .single()?
        .timestamp();
    let mut boundaries = BTreeSet::from([start, end]);
    let season = if (6..=9).contains(&date.month()) {
        Season::Summer
    } else {
        Season::Winter
    };
    for second in periods
        .iter()
        .filter(|period| period.season == season)
        .flat_map(|period| [period.start_second, period.end_second])
        .filter(|second| *second < 86_400)
    {
        let local = date.and_time(NaiveTime::from_num_seconds_from_midnight_opt(second, 0)?);
        match Los_Angeles.from_local_datetime(&local) {
            LocalResult::Single(time) => {
                boundaries.insert(time.timestamp());
            }
            LocalResult::Ambiguous(first, second) => {
                boundaries.insert(first.timestamp());
                boundaries.insert(second.timestamp());
            }
            LocalResult::None => {}
        }
    }
    let original_offset = offset_at(start)?;
    if original_offset != offset_at(end - 1)? {
        let mut low = start;
        let mut high = end - 1;
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            if offset_at(middle)? == original_offset {
                low = middle;
            } else {
                high = middle;
            }
        }
        boundaries.insert(high);
    }
    Some(boundaries)
}

fn rate_segment(
    start: i64,
    end: i64,
    energy: Energy,
    history: &RateModelHistory,
) -> Result<Vec<RatedSlice>, PricingHold> {
    let unresolved = PricingHold::UnresolvedRateBoundary {
        start_time: start,
        end_time: end,
    };
    if start >= end {
        return Err(PricingHold::NonIncreasingTime {
            start_time: start,
            end_time: end,
        });
    }
    if energy == Energy::ZERO {
        return Ok(Vec::new());
    }
    let mut cursor = start;
    let mut slices = Vec::new();
    while cursor < end {
        let local = Los_Angeles
            .timestamp_opt(cursor, 0)
            .single()
            .ok_or(unresolved)?;
        let date = local.date_naive();
        let version = history
            .versions
            .iter()
            .find(|version| version.covers(date))
            .ok_or(PricingHold::MissingRate {
                start_time: cursor,
                end_time: end,
            })?;
        let season = if (6..=9).contains(&date.month()) {
            Season::Summer
        } else {
            Season::Winter
        };
        let second = local.time().num_seconds_from_midnight();
        let period = version
            .definition
            .periods
            .iter()
            .find(|period| {
                period.season == season
                    && period.start_second <= second
                    && second < period.end_second
            })
            .ok_or(PricingHold::MissingRate {
                start_time: cursor,
                end_time: end,
            })?;
        let boundaries = day_boundaries(date, &version.definition.periods).ok_or(unresolved)?;
        let next = boundaries
            .range((
                std::ops::Bound::Excluded(cursor),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .copied()
            .ok_or(unresolved)?
            .min(end);
        let day_type = if version.holidays.contains(&date) {
            DayType::Holiday
        } else if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
            DayType::Weekend
        } else {
            DayType::Weekday
        };
        slices.push(RatedSlice {
            start_time: cursor,
            end_time: next,
            local_date: date.to_string(),
            local_start: local.to_rfc3339(),
            local_end: Los_Angeles
                .timestamp_opt(next, 0)
                .single()
                .ok_or(unresolved)?
                .to_rfc3339(),
            utc_offset_seconds: local.offset().fix().local_minus_utc(),
            season,
            day_type,
            period: period.period.clone(),
            model_version: version.definition.id.clone(),
            energy: Energy::ZERO,
            rate: period.rate,
            exact_cost: ExactUsd::ZERO,
        });
        cursor = next;
    }
    allocate_energy(&mut slices, energy, start, end);
    Ok(slices)
}

fn allocate_energy(slices: &mut [RatedSlice], energy: Energy, start: i64, end: i64) {
    let duration = (i128::from(end) - i128::from(start)) as u128;
    let mut assigned = 0_u64;
    let mut remainders = Vec::new();
    for (index, slice) in slices.iter_mut().enumerate() {
        let seconds = (i128::from(slice.end_time) - i128::from(slice.start_time)) as u128;
        let numerator = u128::from(energy.micro_kwh()) * seconds;
        let units = (numerator / duration) as u64;
        assigned += units;
        slice.energy = Energy::from_micro_kwh(units);
        remainders.push((index, numerator % duration));
    }
    remainders.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    for (index, _) in remainders
        .into_iter()
        .take((energy.micro_kwh() - assigned) as usize)
    {
        slices[index].energy = Energy::from_micro_kwh(slices[index].energy.micro_kwh() + 1);
    }
    for slice in slices {
        slice.exact_cost = slice.rate.cost_for(slice.energy);
    }
}

fn quote_session(
    reconstructed: &ReconstructedSession,
    history: &RateModelHistory,
) -> Result<Result<TimeOfUseQuote, PricingHold>, PricingError> {
    if !reconstructed.session.is_eligible() {
        return Ok(Err(PricingHold::IneligibleSession));
    }
    let mut quote = TimeOfUseQuote {
        slices: Vec::new(),
        exact_cost: ExactUsd::ZERO,
    };
    for segment in &reconstructed.segments {
        let slices = match rate_segment(
            segment.start_time,
            segment.end_time,
            segment.energy,
            history,
        ) {
            Ok(slices) => slices,
            Err(hold) => return Ok(Err(hold)),
        };
        for slice in slices {
            quote.exact_cost = quote
                .exact_cost
                .checked_add(slice.exact_cost)
                .map_err(PricingError::Money)?;
            quote.slices.push(slice);
        }
    }
    Ok(Ok(quote))
}

pub(crate) fn summarize_time_of_use(
    reconstructed: Vec<ReconstructedSession>,
    history: &RateModelHistory,
) -> Result<TimeOfUsePricing, PricingError> {
    let mut summary = TimeOfUsePricing {
        sessions: Vec::new(),
        priced_energy: Energy::ZERO,
        exact_subtotal: ExactUsd::ZERO,
        rounded_subtotal: UsdCents::default(),
    };
    for reconstructed in reconstructed {
        let quote = quote_session(&reconstructed, history)?;
        if let Ok(quote) = &quote {
            summary.exact_subtotal = summary
                .exact_subtotal
                .checked_add(quote.exact_cost)
                .map_err(PricingError::Money)?;
            summary.priced_energy = summary
                .priced_energy
                .checked_add(reconstructed.session.observed_ac)
                .map_err(|_| PricingError::EnergyOverflow)?;
        }
        summary.sessions.push(TimeOfUseSession {
            session: reconstructed.session,
            quote,
        });
    }
    summary.rounded_subtotal = summary.exact_subtotal.round_half_up_to_cents();
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ChargeType, ChargerGeofence, ConnectionId, CounterReading, Event, EventKind, Ledger,
        LocationPoint, LocationSample, MoneyError, OwnerId, VehicleId,
    };

    fn timestamp(value: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp()
    }

    fn model() -> RateModelDefinition {
        RateModelDefinition {
            id: "pge-ev2-a-v1".to_owned(),
            plan: "PGE-EV2-A".to_owned(),
            source_label: "Public synthetic PG&E bundled EV2-A sample Advice 7846-E".to_owned(),
            source_url:
                "https://www.pge.com/tariffs/assets/pdf/tariffbook/ELEC_SCHEDS_EV2%20%28Sch%29.pdf"
                    .to_owned(),
            effective_from: "2026-03-01".to_owned(),
            effective_until: None,
            timezone: "America/Los_Angeles".to_owned(),
            timezone_rules_version: TIMEZONE_RULES_VERSION.to_owned(),
            season_rules_version: "june-september-v1".to_owned(),
            day_type_rules_version: "all-days-v1".to_owned(),
            holidays: vec!["2026-09-07".to_owned()],
            periods: [
                (Season::Summer, "0.53809", "0.42760"),
                (Season::Winter, "0.41099", "0.39428"),
            ]
            .into_iter()
            .flat_map(|(season, peak, part)| {
                [
                    ("off_peak", 0, 54000, "0.22558"),
                    ("part_peak", 54000, 57600, part),
                    ("peak", 57600, 75600, peak),
                    ("part_peak", 75600, 86400, part),
                ]
                .map(|(period, start_second, end_second, rate)| RatePeriod {
                    season,
                    period: period.to_owned(),
                    start_second,
                    end_second,
                    rate: UsdRate::parse_per_kwh(rate).unwrap(),
                })
            })
            .collect(),
        }
    }

    fn history() -> RateModelHistory {
        RateModelHistory::new(vec![RateModelVersion::new(model()).unwrap()]).unwrap()
    }

    fn slices(start: &str, end: &str, kwh: &str) -> Vec<RatedSlice> {
        rate_segment(
            timestamp(start),
            timestamp(end),
            Energy::parse_kwh(kwh).unwrap(),
            &history(),
        )
        .unwrap()
    }

    fn ledger(intervals: &[(&str, &str, &str)]) -> (Ledger, VehicleId) {
        let mut ledger = Ledger::default();
        let vehicle = VehicleId::new("vehicle-a").unwrap();
        let owner = OwnerId::new("owner-a").unwrap();
        ledger.register(vehicle.clone(), owner.clone()).unwrap();
        let point = LocationPoint::new(0.0, 0.0).unwrap();
        ledger.configure_charger(owner, ChargerGeofence::new(point, 100).unwrap());
        for (index, (start, end, kwh)) in intervals.iter().enumerate() {
            for (step, time, kind, counter) in [
                (0, *start, EventKind::Start, "0"),
                (1, *end, EventKind::End, *kwh),
            ] {
                ledger
                    .ingest(Event {
                        vehicle: Some(vehicle.clone()),
                        connection: ConnectionId::new(&format!("connection-{index}")).unwrap(),
                        position: (index * 2 + step) as u64,
                        time: timestamp(time),
                        kind,
                        charge_type: ChargeType::Ac,
                        counter: Some(CounterReading::parse_kwh(counter)),
                        location: LocationSample::Valid(point),
                    })
                    .unwrap();
            }
        }
        (ledger, vehicle)
    }

    #[test]
    fn live_partial_edges_quote_actual_counter_times_without_extending_energy_to_state_boundaries()
    {
        let (mut ledger, vehicle) = ledger(&[]);
        ledger.use_observed_counter_window(&vehicle).unwrap();
        for (index, (time, kind, counter, charge_type)) in [
            (
                "2026-09-01T12:00:00Z",
                EventKind::Start,
                None,
                ChargeType::Unknown,
            ),
            (
                "2026-09-01T12:05:00Z",
                EventKind::Sample,
                Some("10"),
                ChargeType::Unknown,
            ),
            (
                "2026-09-01T12:10:00Z",
                EventKind::Sample,
                Some("10.5"),
                ChargeType::Ac,
            ),
            ("2026-09-01T12:15:00Z", EventKind::End, None, ChargeType::Ac),
        ]
        .into_iter()
        .enumerate()
        {
            ledger
                .ingest(Event {
                    vehicle: Some(vehicle.clone()),
                    connection: ConnectionId::new("connection-a").unwrap(),
                    position: index as u64,
                    time: timestamp(time),
                    kind,
                    charge_type,
                    counter: counter.map(CounterReading::parse_kwh),
                    location: LocationSample::InsideGeofence,
                })
                .unwrap();
        }
        let pricing = ledger.time_of_use_pricing(&vehicle, &history()).unwrap();
        let session = &pricing.sessions[0].session;
        assert!(session.is_eligible());
        assert_eq!(session.observed_window.as_ref().unwrap().usable_readings, 2);
        assert_eq!(
            session.observed_window.as_ref().unwrap().first_time,
            Some(timestamp("2026-09-01T12:05:00Z"))
        );
        let quote = pricing.sessions[0].quote.as_ref().unwrap();
        assert_eq!(
            quote.slices[0].start_time,
            timestamp("2026-09-01T12:05:00Z")
        );
        assert_eq!(quote.slices[0].end_time, timestamp("2026-09-01T12:10:00Z"));
        assert_eq!(quote.exact_cost.to_string(), "0.1127900000000000 USD");
        assert_eq!(pricing.rounded_subtotal.to_string(), "0.11 USD");
    }

    #[test]
    fn ev2_a_golden_sessions_sum_exact_costs_and_round_the_scoped_subtotal_once() {
        let (ledger, vehicle) = ledger(&[
            ("2026-09-01T12:00:00Z", "2026-09-01T12:10:00Z", "0.5"),
            ("2026-09-01T12:00:00Z", "2026-09-01T12:10:00Z", "0.25"),
        ]);
        let pricing = ledger.time_of_use_pricing(&vehicle, &history()).unwrap();
        assert_eq!(
            pricing.sessions[0]
                .quote
                .as_ref()
                .unwrap()
                .exact_cost
                .to_string(),
            "0.1127900000000000 USD"
        );
        assert_eq!(
            pricing.sessions[1]
                .quote
                .as_ref()
                .unwrap()
                .exact_cost
                .to_string(),
            "0.0563950000000000 USD"
        );
        assert_eq!(pricing.exact_subtotal.to_string(), "0.1691850000000000 USD");
        assert_eq!(pricing.rounded_subtotal.as_cents(), 17);
        assert_eq!(pricing.priced_energy, Energy::parse_kwh("0.75").unwrap());
    }

    #[test]
    fn interval_crossing_1500_and_1600_uses_all_three_named_periods() {
        let lines = slices("2026-09-01T21:30:00Z", "2026-09-01T23:30:00Z", "2");
        assert_eq!(
            lines
                .iter()
                .map(|line| line.period.as_str())
                .collect::<Vec<_>>(),
            ["off_peak", "part_peak", "peak"]
        );
        assert_eq!(
            lines
                .iter()
                .map(|line| line.energy.to_string())
                .collect::<Vec<_>>(),
            ["0.500000 kWh", "1.000000 kWh", "0.500000 kWh"]
        );
        let total = lines
            .iter()
            .try_fold(ExactUsd::ZERO, |sum, line| sum.checked_add(line.exact_cost))
            .unwrap();
        assert_eq!(total.to_string(), "0.8094350000000000 USD");
    }

    #[test]
    fn half_open_period_edges_and_evening_part_peak_are_exact() {
        for (start, end, period) in [
            ("2026-09-01T21:59:59Z", "2026-09-01T22:00:00Z", "off_peak"),
            ("2026-09-01T22:00:00Z", "2026-09-01T23:00:00Z", "part_peak"),
            ("2026-09-01T23:00:00Z", "2026-09-02T04:00:00Z", "peak"),
            ("2026-09-02T04:00:00Z", "2026-09-02T07:00:00Z", "part_peak"),
        ] {
            assert_eq!(
                slices(start, end, "1")
                    .iter()
                    .map(|line| line.period.as_str())
                    .collect::<Vec<_>>(),
                [period]
            );
        }
    }

    #[test]
    fn largest_fraction_allocation_conserves_micro_kwh_and_breaks_ties_earlier() {
        let tied = slices("2026-09-01T21:59:59Z", "2026-09-01T22:00:01Z", "0.000001");
        assert_eq!(tied[0].energy, Energy::parse_kwh("0.000001").unwrap());
        assert_eq!(tied[1].energy, Energy::ZERO);
        let uneven = slices("2026-09-01T21:59:59Z", "2026-09-01T22:00:02Z", "0.000002");
        assert_eq!(
            uneven
                .iter()
                .map(|line| line.energy.micro_kwh())
                .collect::<Vec<_>>(),
            [1, 1]
        );
        let later_fraction = slices("2026-09-01T21:59:59Z", "2026-09-01T22:00:02Z", "0.000001");
        assert_eq!(
            later_fraction
                .iter()
                .map(|line| line.energy.micro_kwh())
                .collect::<Vec<_>>(),
            [0, 1]
        );
        for units in 1..100 {
            let lines = rate_segment(
                timestamp("2026-09-01T21:30:00Z"),
                timestamp("2026-09-02T08:00:00Z"),
                Energy::from_micro_kwh(units),
                &history(),
            )
            .unwrap();
            assert_eq!(
                lines
                    .iter()
                    .map(|line| line.energy.micro_kwh())
                    .sum::<u64>(),
                units
            );
        }
    }

    #[test]
    fn spring_gap_and_fall_fold_split_at_the_real_utc_transition() {
        for (start, end, transition, offsets) in [
            (
                "2026-03-08T09:30:00Z",
                "2026-03-08T10:30:00Z",
                "2026-03-08T10:00:00Z",
                [-28800, -25200],
            ),
            (
                "2026-11-01T08:30:00Z",
                "2026-11-01T09:30:00Z",
                "2026-11-01T09:00:00Z",
                [-25200, -28800],
            ),
        ] {
            let lines = slices(start, end, "1");
            assert_eq!(lines.len(), 2);
            assert_eq!(lines[0].end_time, timestamp(transition));
            assert_eq!(
                lines
                    .iter()
                    .map(|line| line.utc_offset_seconds)
                    .collect::<Vec<_>>(),
                offsets
            );
            assert_eq!(
                lines
                    .iter()
                    .map(|line| line.energy.micro_kwh())
                    .collect::<Vec<_>>(),
                [500000, 500000]
            );
        }
    }

    #[test]
    fn midnight_and_summer_boundaries_resolve_seasons_from_local_dates() {
        for (start, end, seasons) in [
            (
                "2026-06-01T06:30:00Z",
                "2026-06-01T07:30:00Z",
                [Season::Winter, Season::Summer],
            ),
            (
                "2026-10-01T06:30:00Z",
                "2026-10-01T07:30:00Z",
                [Season::Summer, Season::Winter],
            ),
        ] {
            let lines = slices(start, end, "1");
            assert_eq!(
                lines.iter().map(|line| line.season).collect::<Vec<_>>(),
                seasons
            );
            assert_ne!(lines[0].local_date, lines[1].local_date);
        }
    }

    #[test]
    fn weekday_weekend_and_configured_holiday_have_the_same_period_rates() {
        for (day, expected) in [
            ("2026-09-01", DayType::Weekday),
            ("2026-09-05", DayType::Weekend),
            ("2026-09-07", DayType::Holiday),
        ] {
            let lines = slices(
                &format!("{day}T23:00:00Z"),
                &format!("{day}T23:01:00Z"),
                "1",
            );
            assert_eq!(lines[0].day_type, expected);
            assert_eq!(lines[0].rate, UsdRate::parse_per_kwh("0.53809").unwrap());
        }
    }

    #[test]
    fn effective_date_split_uses_both_versions_and_a_gap_holds_the_whole_session() {
        let mut first = model();
        first.effective_until = Some("2026-09-02".to_owned());
        let mut second = model();
        second.id = "pge-ev2-a-v2".to_owned();
        second.effective_from = "2026-09-02".to_owned();
        for period in &mut second.periods {
            period.rate = UsdRate::parse_per_kwh("1").unwrap();
        }
        let history = RateModelHistory::new(vec![
            RateModelVersion::new(first.clone()).unwrap(),
            RateModelVersion::new(second.clone()).unwrap(),
        ])
        .unwrap();
        let (ledger, vehicle) = ledger(&[("2026-09-02T06:30:00Z", "2026-09-02T07:30:00Z", "1")]);
        let pricing = ledger.time_of_use_pricing(&vehicle, &history).unwrap();
        let quote = pricing.sessions[0].quote.as_ref().unwrap();
        assert_eq!(
            quote
                .slices
                .iter()
                .map(|line| line.model_version.as_str())
                .collect::<Vec<_>>(),
            ["pge-ev2-a-v1", "pge-ev2-a-v2"]
        );
        assert_eq!(quote.exact_cost.to_string(), "0.7138000000000000 USD");
        second.effective_from = "2026-09-03".to_owned();
        let gapped = RateModelHistory::new(vec![
            RateModelVersion::new(first).unwrap(),
            RateModelVersion::new(second).unwrap(),
        ])
        .unwrap();
        let held = ledger.time_of_use_pricing(&vehicle, &gapped).unwrap();
        assert!(matches!(
            held.sessions[0].quote,
            Err(PricingHold::MissingRate { .. })
        ));
        assert_eq!(held.priced_energy, Energy::ZERO);
        assert_eq!(
            ledger.sessions(&vehicle).unwrap(),
            pricing
                .sessions
                .iter()
                .map(|session| session.session.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_missing_period_and_non_increasing_time_never_return_partial_quotes() {
        let mut missing = model();
        missing.periods.retain(|period| period.period != "peak");
        let missing = RateModelHistory::new(vec![RateModelVersion::new(missing).unwrap()]).unwrap();
        let (ledger, vehicle) = ledger(&[("2026-09-01T21:30:00Z", "2026-09-01T23:30:00Z", "1")]);
        let held = ledger.time_of_use_pricing(&vehicle, &missing).unwrap();
        assert!(matches!(
            held.sessions[0].quote,
            Err(PricingHold::MissingRate { .. })
        ));
        assert_eq!(held.exact_subtotal, ExactUsd::ZERO);
        assert!(matches!(
            rate_segment(1, 1, Energy::ZERO, &history()),
            Err(PricingHold::NonIncreasingTime { .. })
        ));
        assert!(matches!(
            rate_segment(2, 1, Energy::from_micro_kwh(1), &history()),
            Err(PricingHold::NonIncreasingTime { .. })
        ));
        assert!(matches!(
            rate_segment(
                i64::MAX - 1,
                i64::MAX,
                Energy::from_micro_kwh(1),
                &history()
            ),
            Err(PricingHold::UnresolvedRateBoundary { .. })
        ));
    }

    #[test]
    fn complete_zero_quotes_without_a_rate_and_subcent_sessions_round_only_once() {
        let (ledger, vehicle) = ledger(&[("2026-09-01T12:00:00Z", "2026-09-01T12:01:00Z", "0")]);
        let zero = ledger
            .time_of_use_pricing(&vehicle, &RateModelHistory::new(vec![]).unwrap())
            .unwrap();
        assert_eq!(
            zero.sessions[0].quote.as_ref().unwrap().exact_cost,
            ExactUsd::ZERO
        );
        let mut tiny = model();
        for period in &mut tiny.periods {
            period.rate = UsdRate::parse_per_kwh("0.004").unwrap();
        }
        let rates = RateModelHistory::new(vec![RateModelVersion::new(tiny).unwrap()]).unwrap();
        let (ledger, vehicle) = self::ledger(&[
            ("2026-09-01T12:00:00Z", "2026-09-01T12:01:00Z", "1"),
            ("2026-09-01T12:00:00Z", "2026-09-01T12:01:00Z", "1"),
        ]);
        let pricing = ledger.time_of_use_pricing(&vehicle, &rates).unwrap();
        assert!(pricing.sessions.iter().all(|session| {
            session
                .quote
                .as_ref()
                .unwrap()
                .exact_cost
                .round_half_up_to_cents()
                .as_cents()
                == 0
        }));
        assert_eq!(pricing.rounded_subtotal.as_cents(), 1);
    }

    #[test]
    fn checked_cost_overflow_is_a_typed_error_without_a_partial_summary() {
        let mut maximum = model();
        for period in &mut maximum.periods {
            period.rate = UsdRate::parse_per_kwh("1844674407.3709551615").unwrap();
        }
        let rates = RateModelHistory::new(vec![RateModelVersion::new(maximum).unwrap()]).unwrap();
        let (ledger, vehicle) = ledger(&[
            (
                "2026-09-01T12:00:00Z",
                "2026-09-01T12:01:00Z",
                "18446744073709.551615",
            ),
            (
                "2026-09-01T12:02:00Z",
                "2026-09-01T12:03:00Z",
                "18446744073709.551615",
            ),
        ]);
        assert_eq!(
            ledger.time_of_use_pricing(&vehicle, &rates),
            Err(PricingError::Money(MoneyError::CostOverflow))
        );
    }

    #[test]
    fn invalid_calendar_rules_overlapping_periods_and_effective_dates_are_refused() {
        let mut invalid = model();
        invalid.timezone_rules_version = "unknown".to_owned();
        assert_eq!(
            RateModelVersion::new(invalid),
            Err(TariffError::IncompatibleRules)
        );
        let mut invalid = model();
        invalid.periods[0].end_second = 55000;
        assert_eq!(
            RateModelVersion::new(invalid),
            Err(TariffError::OverlappingWindows)
        );
        let mut invalid = model();
        invalid.effective_until = Some(invalid.effective_from.clone());
        assert_eq!(
            RateModelVersion::new(invalid),
            Err(TariffError::InvalidWindow)
        );
        let first = RateModelVersion::new(model()).unwrap();
        let mut second = model();
        second.id = "v2".to_owned();
        assert_eq!(
            RateModelHistory::new(vec![first, RateModelVersion::new(second).unwrap()]),
            Err(TariffError::OverlappingWindows)
        );
    }
    #[test]
    fn long_counter_gaps_are_allocated_without_inventing_a_maximum_gap_hold() {
        let lines = slices("2026-09-01T12:00:00Z", "2026-09-06T12:00:00Z", "0.000007");
        assert!(lines.len() > 20);
        assert_eq!(
            lines
                .iter()
                .map(|line| line.energy.micro_kwh())
                .sum::<u64>(),
            7
        );
        assert_eq!(
            lines.first().unwrap().start_time,
            timestamp("2026-09-01T12:00:00Z")
        );
        assert_eq!(
            lines.last().unwrap().end_time,
            timestamp("2026-09-06T12:00:00Z")
        );
    }

    #[test]
    fn monthly_history_conserves_slices_across_midnight_dst_and_rate_versions() {
        let mut first = model();
        first.effective_from = "2024-01-01".to_owned();
        first.effective_until = Some("2026-10-01".to_owned());
        let mut second = model();
        second.id = "synthetic-v2".to_owned();
        second.effective_from = "2026-10-01".to_owned();
        for period in &mut second.periods {
            period.rate = UsdRate::parse_per_kwh("0.004").unwrap();
        }
        let rates = RateModelHistory::new(vec![
            RateModelVersion::new(first).unwrap(),
            RateModelVersion::new(second).unwrap(),
        ])
        .unwrap();
        let (ledger, vehicle) = ledger(&[
            ("2024-02-29T20:00:00Z", "2024-02-29T21:00:00Z", "0"),
            ("2026-03-08T09:30:00Z", "2026-03-08T10:30:00Z", "1"),
            ("2026-09-01T21:30:00Z", "2026-09-01T23:30:00Z", "2"),
            ("2026-10-01T06:30:00Z", "2026-10-01T07:30:00Z", "1"),
            ("2026-11-01T08:30:00Z", "2026-11-01T09:30:00Z", "1"),
            ("2026-11-01T10:30:00Z", "2026-11-01T11:30:00Z", "1"),
        ]);
        let pricing = ledger.time_of_use_pricing(&vehicle, &rates).unwrap();
        let history = pricing
            .monthly_history(timestamp("2026-12-02T00:00:00Z"))
            .unwrap();
        assert_eq!(
            history
                .months
                .iter()
                .map(|month| month.first_date.to_string())
                .collect::<Vec<_>>(),
            [
                "2026-12-01",
                "2026-11-01",
                "2026-10-01",
                "2026-09-01",
                "2026-03-01",
                "2024-02-01"
            ]
        );
        assert_eq!(history.months[0].days.len(), 31);
        assert_eq!(history.months[1].days.len(), 30);
        assert_eq!(history.months[5].days.len(), 29);
        assert!(!history.months[0].days[0].future);
        assert!(history.months[0].days[1].future);
        assert_eq!(history.months[0].evidence.quoted_sessions, 0);
        assert_eq!(
            history.months[1].days[0].total.energy,
            Energy::parse_kwh("2").unwrap()
        );
        assert_eq!(
            history.months[1].days[0]
                .total
                .exact_cost
                .round_half_up_to_cents()
                .as_cents(),
            1
        );
        assert_eq!(history.months[1].evidence.quoted_sessions, 2);
        assert_eq!(
            history.months[3].days[0]
                .periods
                .map(|total| total.energy.to_string()),
            ["0.500000 kWh", "1.000000 kWh", "0.500000 kWh"]
        );
        assert_eq!(history.months[3].evidence.quoted_sessions, 2);
        assert_eq!(
            history.months[5].days[28].evidence.complete_zero_sessions,
            1
        );
        let mut total = crate::ChargingTotal::default();
        for month in &history.months {
            let day_energy = month
                .days
                .iter()
                .try_fold(Energy::ZERO, |sum, day| sum.checked_add(day.total.energy))
                .unwrap();
            assert_eq!(month.total.energy, day_energy);
            let day_cost = month
                .days
                .iter()
                .try_fold(ExactUsd::ZERO, |sum, day| {
                    sum.checked_add(day.total.exact_cost)
                })
                .unwrap();
            assert_eq!(month.total.exact_cost, day_cost);
            for day in &month.days {
                assert_eq!(
                    day.total.energy,
                    day.periods
                        .iter()
                        .try_fold(Energy::ZERO, |sum, total| sum.checked_add(total.energy))
                        .unwrap()
                );
                assert_eq!(
                    day.total.exact_cost,
                    day.periods
                        .iter()
                        .try_fold(ExactUsd::ZERO, |sum, total| sum
                            .checked_add(total.exact_cost))
                        .unwrap()
                );
            }
            let period_energy = month
                .periods
                .iter()
                .try_fold(Energy::ZERO, |sum, period| sum.checked_add(period.energy))
                .unwrap();
            assert_eq!(month.total.energy, period_energy);
            let period_cost = month
                .periods
                .iter()
                .try_fold(ExactUsd::ZERO, |sum, period| {
                    sum.checked_add(period.exact_cost)
                })
                .unwrap();
            assert_eq!(month.total.exact_cost, period_cost);
            total.energy = total.energy.checked_add(month.total.energy).unwrap();
            total.exact_cost = total
                .exact_cost
                .checked_add(month.total.exact_cost)
                .unwrap();
        }
        assert_eq!(total.energy, pricing.priced_energy);
        assert_eq!(total.exact_cost, pricing.exact_subtotal);
        assert_eq!(
            history,
            pricing
                .monthly_history(timestamp("2026-12-02T00:00:00Z"))
                .unwrap()
        );
    }

    #[test]
    fn monthly_history_keeps_holds_other_chargers_zero_and_undated_evidence_separate() {
        let (ledger, vehicle) = ledger(&[("2026-09-01T12:00:00Z", "2026-09-01T12:10:00Z", "1")]);
        let mut pricing = ledger
            .time_of_use_pricing(&vehicle, &RateModelHistory::new(vec![]).unwrap())
            .unwrap();
        let mut other = pricing.sessions[0].clone();
        other.session.classification = crate::ChargerClassification::OtherCharger;
        other.quote = Err(PricingHold::IneligibleSession);
        pricing.sessions.push(other);
        let mut undated = pricing.sessions[0].clone();
        undated.session.start_time = None;
        undated.session.end_time = None;
        pricing.sessions.push(undated);
        let history = pricing
            .monthly_history(timestamp("2026-09-02T12:00:00Z"))
            .unwrap();
        assert_eq!(history.evidence.held_sessions, 2);
        assert_eq!(history.evidence.other_sessions, 1);
        assert_eq!(history.undated_sessions, 1);
        assert_eq!(history.months[0].days[0].evidence.held_sessions, 1);
        assert_eq!(history.months[0].days[0].evidence.other_sessions, 1);
        assert!(
            history.months[0]
                .days
                .iter()
                .all(|day| day.total.energy == Energy::ZERO
                    && day.total.exact_cost == ExactUsd::ZERO
                    && day.evidence.quoted_sessions == 0)
        );
        assert_eq!(
            pricing.monthly_history(i64::MAX),
            Err(crate::MonthlyHistoryError::Calendar)
        );
    }

    #[test]
    fn monthly_history_flags_only_the_quoted_partial_counter_days() {
        let (mut ledger, vehicle) = ledger(&[]);
        ledger.use_observed_counter_window(&vehicle).unwrap();
        for (index, (time, kind, counter)) in [
            ("2026-09-01T06:00:00Z", EventKind::Start, None),
            ("2026-09-01T12:00:00Z", EventKind::Sample, Some("1")),
            ("2026-09-01T13:00:00Z", EventKind::Sample, Some("2")),
            ("2026-09-02T08:00:00Z", EventKind::End, None),
        ]
        .into_iter()
        .enumerate()
        {
            ledger
                .ingest(Event {
                    vehicle: Some(vehicle.clone()),
                    connection: ConnectionId::new("partial-window").unwrap(),
                    position: index as u64,
                    time: timestamp(time),
                    kind,
                    charge_type: ChargeType::Ac,
                    counter: counter.map(CounterReading::parse_kwh),
                    location: LocationSample::InsideGeofence,
                })
                .unwrap();
        }
        let pricing = ledger.time_of_use_pricing(&vehicle, &history()).unwrap();
        let monthly = pricing
            .monthly_history(timestamp("2026-09-03T12:00:00Z"))
            .unwrap();
        assert_eq!(monthly.months.len(), 1);
        assert_eq!(monthly.evidence.partial_sessions, 1);
        assert_eq!(monthly.months[0].days[0].evidence.partial_sessions, 1);
        assert_eq!(monthly.months[0].days[1].evidence.partial_sessions, 0);
        assert_eq!(
            monthly.months[0].total.energy,
            Energy::parse_kwh("1").unwrap()
        );
    }

    #[test]
    fn monthly_calendar_rollover_and_half_open_holds_preserve_evidence_without_extra_days() {
        let (ledger, vehicle) = ledger(&[("2026-10-01T06:30:00Z", "2026-10-01T07:00:00Z", "1")]);
        let pricing = ledger
            .time_of_use_pricing(&vehicle, &RateModelHistory::new(vec![]).unwrap())
            .unwrap();
        let before = pricing
            .monthly_history(timestamp("2026-10-01T06:59:59Z"))
            .unwrap();
        let after = pricing
            .monthly_history(timestamp("2026-10-01T07:00:00Z"))
            .unwrap();
        assert_eq!(before.months.len(), 1);
        assert_eq!(before.today.to_string(), "2026-09-30");
        assert_eq!(after.months.len(), 2);
        assert_eq!(after.today.to_string(), "2026-10-01");
        assert_eq!(after.months[0].evidence.held_sessions, 0);
        assert_eq!(after.months[1], before.months[0]);
        assert_eq!(after.months[1].days[29].evidence.held_sessions, 1);
        let future = pricing
            .monthly_history(timestamp("2026-08-01T12:00:00Z"))
            .unwrap();
        assert_eq!(future.months[0].first_date.to_string(), "2026-08-01");
        assert_eq!(future.months[1].first_date.to_string(), "2026-09-01");
        assert!(future.months[1].days.iter().all(|day| day.future));
    }

    #[test]
    fn monthly_totals_refuse_energy_and_cost_overflow_without_partial_history() {
        let (ledger, vehicle) = ledger(&[("2026-09-01T12:00:00Z", "2026-09-01T12:10:00Z", "1")]);
        let mut pricing = ledger.time_of_use_pricing(&vehicle, &history()).unwrap();
        let maximum_energy = Energy::parse_kwh("18446744073709.551615").unwrap();
        pricing.sessions[0].quote.as_mut().unwrap().slices[0].energy = maximum_energy;
        pricing.sessions.push(pricing.sessions[0].clone());
        assert_eq!(
            pricing.monthly_history(timestamp("2026-09-02T12:00:00Z")),
            Err(crate::MonthlyHistoryError::EnergyOverflow)
        );
        let maximum_cost = UsdRate::parse_per_kwh("1844674407.3709551615")
            .unwrap()
            .cost_for(maximum_energy);
        for session in &mut pricing.sessions {
            let slice = &mut session.quote.as_mut().unwrap().slices[0];
            slice.energy = Energy::parse_kwh("1").unwrap();
            slice.exact_cost = maximum_cost;
        }
        assert_eq!(
            pricing.monthly_history(timestamp("2026-09-02T12:00:00Z")),
            Err(crate::MonthlyHistoryError::Money(MoneyError::CostOverflow))
        );
    }
}
