use chargeshare_core::{
    ChargeType, ChargerClassification, ConnectionId, CounterReading, Energy, Event, EventKind,
    ExactUsd, Ledger, OwnerId, PricingHold, RateSchedule, Session, SessionId, SessionPricing,
    UsdRate, VehicleId,
};
use serde_json::{Value, json};

use crate::rates::{Date, SAMPLE_PUBLIC_TARIFF, period_label, schedule_for_days, version_label};

type DemoResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const DEMO_NOW: i64 = 13 * 24 + 14;
const MONTH_START: i64 = 24;
const WEEK_START: i64 = 7 * 24;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Scenario {
    Complete,
    Missing,
    Winter,
}

impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Missing => "missing",
            Self::Winter => "winter",
        }
    }

    fn month(self) -> u8 {
        if self == Self::Winter { 10 } else { 9 }
    }
}

fn month_name(month: u8) -> &'static str {
    match month {
        9 => "September",
        10 => "October",
        _ => unreachable!("fixed sample months"),
    }
}

fn sample_date_label(month: u8, tick: i64) -> String {
    format!("{} {}", &month_name(month)[..3], tick.div_euclid(24))
}

fn sample_clock_label(month: u8, tick: i64) -> String {
    format!(
        "{}, 2026 · {}",
        sample_date_label(month, tick),
        clock_label(tick)
    )
}

fn period_labels(month: u8, now: i64, week_start: i64, month_start: i64) -> Value {
    json!({
        "week": format!("{} to {}", sample_date_label(month, week_start), now.div_euclid(24)),
        "month": format!("{} to {}", sample_date_label(month, month_start), now.div_euclid(24)),
        "as_of": sample_clock_label(month, now),
        "name": month_name(month),
    })
}

fn quantity(energy: Energy) -> String {
    energy
        .to_string()
        .trim_end_matches(" kWh")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn dollars(cost: ExactUsd) -> String {
    let cents = cost.round_half_up_to_cents().as_cents();
    format!("${}.{:02}", cents / 100, cents % 100)
}

fn rate_label(rate: UsdRate) -> String {
    let decimal = rate.to_string();
    let units: u64 = decimal
        .trim_end_matches(" USD/kWh")
        .replace('.', "")
        .parse()
        .expect("exact rate representation");
    let rounded = units / 1_000_000 + u64::from(units % 1_000_000 >= 500_000);
    format!("${}.{:04}", rounded / 10_000, rounded % 10_000)
}

fn clock_label(tick: i64) -> String {
    match tick.rem_euclid(24) {
        0 => "midnight".to_owned(),
        12 => "noon".to_owned(),
        hour if hour < 12 => format!("{hour} am"),
        hour => format!("{} pm", hour - 12),
    }
}

fn session_clock_label(tick: i64) -> String {
    let hour = tick.rem_euclid(24);
    let period = if hour < 12 { "am" } else { "pm" };
    format!("{}:00{period}", (hour + 11) % 12 + 1)
}

fn sample_rates(month: u8) -> DemoResult<RateSchedule> {
    let days = if month == 9 { 30 } else { 31 };
    schedule_for_days(
        SAMPLE_PUBLIC_TARIFF,
        (1..=days).map(|day| {
            (
                Date {
                    year: 2026,
                    month,
                    day,
                },
                i64::from(day) * 24,
            )
        }),
    )
}

fn rate_outlook(rates: &RateSchedule, now: i64) -> Value {
    let current = rates
        .windows()
        .iter()
        .find(|window| window.start_time() <= now && now < window.end_time());
    let next = current.and_then(|window| {
        rates
            .windows()
            .iter()
            .find(|next| next.start_time() == window.end_time())
    });
    json!({
        "label": current.and_then(|window| version_label(window.version())).unwrap_or("Rate coverage unavailable"),
        "current": current.map(|window| json!({ "price": rate_label(window.rate()), "until": clock_label(window.end_time()) })),
        "next": next.map(|window| json!({ "price": rate_label(window.rate()), "starts": clock_label(window.start_time()), "ends": clock_label(window.end_time()) })),
    })
}

fn session_periods(priced: &SessionPricing, rates: &RateSchedule) -> Vec<Value> {
    match &priced.quote {
        Ok(quote) => quote.lines.iter().map(|line| json!({
            "label": period_label(line.rate_window.version(), line.rate_window.start_time().rem_euclid(24)).unwrap_or("Unknown period"),
            "energy": quantity(line.energy),
        })).collect(),
        Err(_) => {
            let mut labels = Vec::new();
            for window in rates.windows().iter().filter(|window| {
                priced.session.start_time.is_some_and(|start| start < window.end_time())
                    && priced.session.end_time.is_some_and(|end| window.start_time() < end)
            }) {
                if let Some(label) = period_label(window.version(), window.start_time().rem_euclid(24))
                    && !labels.contains(&label) { labels.push(label); }
            }
            labels.into_iter().map(|label| json!({ "label": label, "energy": null })).collect()
        }
    }
}

fn within_period(session: &Session, start: i64, end: i64) -> bool {
    session
        .start_time
        .is_some_and(|time| start <= time && time < end)
        && session.end_time.is_some_and(|time| time <= end)
}

#[derive(Default)]
struct PeriodTotals {
    energy: Energy,
    cost: ExactUsd,
    held: usize,
}

impl PeriodTotals {
    fn from_sessions(sessions: &[SessionPricing], start: i64, end: i64) -> DemoResult<Self> {
        let mut totals = Self::default();
        for priced in sessions
            .iter()
            .filter(|priced| within_period(&priced.session, start, end))
        {
            match &priced.quote {
                Ok(quote) => {
                    totals.energy = totals
                        .energy
                        .checked_add(priced.session.observed_ac)
                        .map_err(|_| "sample energy overflow")?;
                    totals.cost = totals.cost.checked_add(quote.exact_cost)?;
                }
                Err(_) => totals.held += 1,
            }
        }
        Ok(totals)
    }

    fn add(&mut self, other: &Self) -> DemoResult<()> {
        self.energy = self
            .energy
            .checked_add(other.energy)
            .map_err(|_| "sample energy overflow")?;
        self.cost = self.cost.checked_add(other.cost)?;
        self.held += other.held;
        Ok(())
    }

    fn to_json(&self) -> Value {
        json!({ "priced_energy": quantity(self.energy), "priced_subtotal": dollars(self.cost), "held": self.held })
    }
}

pub(crate) fn snapshot(scenario: Scenario) -> DemoResult<Value> {
    snapshot_with_rates(scenario, &sample_rates(scenario.month())?)
}

fn snapshot_with_rates(scenario: Scenario, rates: &RateSchedule) -> DemoResult<Value> {
    let month_number = scenario.month();
    let missing = scenario == Scenario::Missing;
    let mut ledger = Ledger::default();
    for (id, owner, first, last, earlier) in [
        ("vehicle-a", "owner-a", "18", "24", "20"),
        ("vehicle-b", "owner-b", "12", "16", "8"),
    ] {
        let vehicle = VehicleId::new(id)?;
        ledger.register(vehicle.clone(), OwnerId::new(owner)?)?;
        for (connection, day, offset, readings) in [
            ("session-01", 9, 0, vec![(6, "0"), (8, first)]),
            (
                "session-02",
                11,
                10,
                if missing && id == "vehicle-b" {
                    vec![(12, "0"), (16, "10")]
                } else {
                    vec![(12, "0"), (15, "4"), (16, "10")]
                },
            ),
            ("session-03", 13, 20, vec![(1, "0"), (3, last)]),
            ("earlier-month", 3, 30, vec![(1, "0"), (3, earlier)]),
            ("prior-month", 0, 40, vec![(1, "0"), (3, "30")]),
        ] {
            let connection = ConnectionId::new(connection)?;
            for (index, (time, counter)) in readings.iter().enumerate() {
                ledger.ingest(Event {
                    vehicle: Some(vehicle.clone()),
                    connection: connection.clone(),
                    position: offset + index as u64,
                    time: day * 24 + *time,
                    kind: if index == 0 {
                        EventKind::Start
                    } else if index + 1 == readings.len() {
                        EventKind::End
                    } else {
                        EventKind::Sample
                    },
                    charge_type: ChargeType::Ac,
                    counter: Some(CounterReading::parse_kwh(counter)),
                })?;
            }
            ledger.classify(
                &vehicle,
                &SessionId {
                    vehicle: vehicle.clone(),
                    connection,
                },
                ChargerClassification::SharedCharger,
            )?;
        }
    }
    let mut vehicles = Vec::new();
    let mut total_week = PeriodTotals::default();
    let mut total_month = PeriodTotals::default();
    for (id, name, model, battery, state) in [
        (
            "vehicle-a",
            "Joseph",
            "Model Y Quicksilver",
            68,
            "Unplugged",
        ),
        ("vehicle-b", "Evan", "Model Y Black", 42, "Charging"),
    ] {
        let summary = ledger.pricing(&VehicleId::new(id)?, rates)?;
        let week = PeriodTotals::from_sessions(&summary.sessions, WEEK_START, DEMO_NOW)?;
        let month = PeriodTotals::from_sessions(&summary.sessions, MONTH_START, DEMO_NOW)?;
        total_week.add(&week)?;
        total_month.add(&month)?;
        let sessions: Vec<Value> = summary.sessions.iter().filter(|priced| within_period(&priced.session, MONTH_START, DEMO_NOW)).map(|priced| {
            let (cost, reason, lines) = match &priced.quote {
                Ok(quote) => (Some(dollars(quote.exact_cost)), None, quote.lines.iter().map(|line| json!({
                    "energy": quantity(line.energy), "rate": format!("{} / kWh", rate_label(line.rate_window.rate())),
                    "cost": dollars(line.exact_cost), "version": line.rate_window.version(),
                })).collect::<Vec<_>>()),
                Err(reason) => (None, Some(match reason {
                    PricingHold::UnresolvedRateBoundary { .. } => "A reading is missing at the rate change. This session is excluded from both totals.",
                    PricingHold::MissingRate { .. } => "A rate is missing for this session.",
                    PricingHold::NonIncreasingTime { .. } => "The readings do not establish when this energy was used.",
                    PricingHold::IneligibleSession => "This session needs its charging evidence reviewed.",
                }), vec![]),
            };
            let start = priced.session.start_time.expect("bounded sample session start");
            let end = priced.session.end_time.expect("bounded sample session end");
            json!({
                "id": format!("{}-{}", id, priced.session.id.connection.as_str()),
                "date": sample_date_label(month_number, start), "date_order": start,
                "time": format!("{}hr ({} - {})", end - start, session_clock_label(start), session_clock_label(end)),
                "vehicle": name, "energy": quantity(priced.session.observed_ac),
                "cost": cost, "reason": reason, "lines": lines, "periods": session_periods(priced, rates),
            })
        }).collect();
        vehicles.push(json!({
            "id": id, "name": name, "model": model, "battery": battery, "state": state,
            "week": week.to_json(), "month": month.to_json(), "sessions": sessions,
            "last_updated": sample_clock_label(month_number, if state == "Charging" { DEMO_NOW } else { DEMO_NOW - 1 }),
            "added_kwh": if state == "Charging" { Some("6") } else { None },
        }));
    }
    Ok(json!({
        "scenario": scenario.name(), "vehicles": vehicles,
        "reimbursement": vehicles[0]["month"],
        "totals": { "week": total_week.to_json(), "month": total_month.to_json() },
        "periods": period_labels(month_number, DEMO_NOW, WEEK_START, MONTH_START),
        "rates": rate_outlook(rates, DEMO_NOW),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winter_preview_uses_its_own_version_and_labels() {
        let data = snapshot(Scenario::Winter).unwrap();
        assert_eq!(data["reimbursement"]["priced_subtotal"], "$21.02");
        assert_eq!(data["totals"]["month"]["priced_subtotal"], "$34.82");
        assert_eq!(data["rates"]["label"], "Winter estimate, unverified");
        assert_eq!(data["rates"]["current"]["price"], "$0.2779");
        assert_eq!(data["rates"]["next"]["price"], "$0.4465");
        assert_eq!(data["periods"]["as_of"], "Oct 13, 2026 · 2 pm");
        for vehicle in data["vehicles"].as_array().unwrap() {
            for session in vehicle["sessions"].as_array().unwrap() {
                for line in session["lines"].as_array().unwrap() {
                    assert_eq!(line["version"], "ev2a-winter-2026-est");
                }
            }
        }
    }

    #[test]
    fn missing_coverage_retains_sessions_and_holds_reimbursement() {
        let data =
            snapshot_with_rates(Scenario::Complete, &RateSchedule::new(vec![]).unwrap()).unwrap();
        assert_eq!(data["reimbursement"]["held"], 4);
        assert_eq!(data["reimbursement"]["priced_energy"], "0");
        assert_eq!(data["totals"]["month"]["held"], 8);
        assert!(data["rates"]["current"].is_null());
        assert!(data["rates"]["next"].is_null());
        for session in data["vehicles"][0]["sessions"].as_array().unwrap() {
            assert!(session["cost"].is_null());
            assert_eq!(session["reason"], "A rate is missing for this session.");
        }
    }

    #[test]
    fn tou_splits_use_measured_lines_and_do_not_invent_missing_allocations() {
        for scenario in [Scenario::Complete, Scenario::Missing] {
            let data = snapshot(scenario).unwrap();
            let session = data["vehicles"][1]["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|session| session["id"] == "vehicle-b-session-02")
                .unwrap();
            assert_eq!(session["periods"][0]["label"], "Off-peak");
            assert_eq!(session["periods"][1]["label"], "Partial-peak");
            if scenario == Scenario::Complete {
                assert_eq!(session["periods"][0]["energy"], "4");
                assert_eq!(session["periods"][1]["energy"], "6");
            } else {
                assert!(session["periods"][0]["energy"].is_null());
                assert!(session["periods"][1]["energy"].is_null());
            }
            assert_eq!(data["reimbursement"]["priced_subtotal"], "$21.18");
        }
    }

    #[test]
    fn labels_follow_fixture_clock_boundaries_and_preserve_rate_precision() {
        let labels = period_labels(9, 15 * 24 + 17, 14 * 24, 2 * 24);
        assert_eq!(labels["week"], "Sep 14 to 15");
        assert_eq!(labels["month"], "Sep 2 to 15");
        assert_eq!(labels["as_of"], "Sep 15, 2026 · 5 pm");
        assert_eq!(
            rate_label(UsdRate::parse_per_kwh("0.2773931020").unwrap()),
            "$0.2774"
        );
        assert_eq!(
            rate_label(UsdRate::parse_per_kwh("1.9999999999").unwrap()),
            "$2.0000"
        );
        assert_eq!(
            rate_label(UsdRate::parse_per_kwh("0.0000499999").unwrap()),
            "$0.0000"
        );
        assert_eq!(
            rate_label(UsdRate::parse_per_kwh("0.00005").unwrap()),
            "$0.0001"
        );
    }

    #[test]
    fn week_and_month_use_core_quotes_and_exclude_prior_month() {
        let data = snapshot(Scenario::Complete).unwrap();
        assert_eq!(data["totals"]["week"]["priced_energy"], "90");
        assert_eq!(data["totals"]["week"]["priced_subtotal"], "$27.38");
        assert_eq!(data["totals"]["month"]["priced_energy"], "118");
        assert_eq!(data["totals"]["month"]["priced_subtotal"], "$35.14");
        assert_eq!(data["vehicles"][0]["week"]["priced_subtotal"], "$15.63");
        assert_eq!(data["vehicles"][0]["month"]["priced_subtotal"], "$21.18");
        assert_eq!(data["vehicles"][1]["month"]["priced_subtotal"], "$13.97");
        for vehicle in data["vehicles"].as_array().unwrap() {
            assert_eq!(vehicle["sessions"].as_array().unwrap().len(), 4);
            assert!(!vehicle["sessions"].to_string().contains("prior-month"));
        }
    }

    #[test]
    fn missing_reading_is_excluded_from_both_periods_and_replay_restores_totals() {
        let original = snapshot(Scenario::Complete).unwrap();
        let missing = snapshot(Scenario::Missing).unwrap();
        assert_eq!(missing["totals"]["week"]["priced_energy"], "80");
        assert_eq!(missing["totals"]["week"]["priced_subtotal"], "$23.40");
        assert_eq!(missing["totals"]["month"]["priced_energy"], "108");
        assert_eq!(missing["totals"]["month"]["priced_subtotal"], "$31.16");
        assert_eq!(missing["totals"]["week"]["held"], 1);
        assert_eq!(missing["totals"]["month"]["held"], 1);
        assert_eq!(missing["vehicles"][1]["month"]["priced_subtotal"], "$9.99");
        let held = missing["vehicles"][1]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|session| session["id"] == "vehicle-b-session-02")
            .unwrap();
        assert!(held["cost"].is_null());
        assert_eq!(held["energy"], "10");
        assert_eq!(snapshot(Scenario::Complete).unwrap(), original);
    }

    #[test]
    fn sample_rate_lookup_switches_at_boundary_and_handles_midnight() {
        let rates = sample_rates(9).unwrap();
        let before = rate_outlook(&rates, DEMO_NOW);
        assert_eq!(before["current"]["price"], "$0.2774");
        assert_eq!(before["current"]["until"], "3 pm");
        assert_eq!(before["next"]["price"], "$0.4784");
        assert_eq!(before["next"]["ends"], "4 pm");
        let at_boundary = rate_outlook(&rates, DEMO_NOW + 1);
        assert_eq!(at_boundary["current"]["price"], "$0.4784");
        assert_eq!(at_boundary["next"]["price"], "$0.5866");
        assert_eq!(rate_outlook(&rates, 14 * 24)["current"]["price"], "$0.2774");
        assert!(rate_outlook(&rates, -1)["current"].is_null());
        assert!(rate_outlook(&rates, 30 * 24 + 23)["next"].is_null());
        assert_eq!(
            PeriodTotals::from_sessions(&[], MONTH_START, DEMO_NOW)
                .unwrap()
                .to_json()["priced_subtotal"],
            "$0.00"
        );
    }
}
