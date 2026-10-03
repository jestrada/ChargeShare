use chargeshare_core::{
    ChargeType, ChargerClassification, ConnectionId, CounterReading, Energy, Event, EventKind,
    ExactUsd, Ledger, OwnerId, PricingHold, RateSchedule, RateWindow, Session, SessionId,
    SessionPricing, UsdRate, VehicleId,
};
use serde_json::{Value, json};

type DemoResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const DEMO_NOW: i64 = 13 * 24 + 14;
const MONTH_START: i64 = 24;
const WEEK_START: i64 = 7 * 24;

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
    dollars(rate.cost_for(Energy::parse_kwh("1").expect("valid sample unit")))
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

fn sample_rates() -> DemoResult<RateSchedule> {
    let low = UsdRate::parse_per_kwh("0.20")?;
    let high = UsdRate::parse_per_kwh("0.40")?;
    let mut windows = Vec::new();
    for day in 0..=14 {
        windows.push(RateWindow::new(
            "fictional-low-v1",
            day * 24,
            day * 24 + 15,
            low,
        )?);
        windows.push(RateWindow::new(
            "fictional-high-v1",
            day * 24 + 15,
            (day + 1) * 24,
            high,
        )?);
    }
    Ok(RateSchedule::new(windows)?)
}

fn rate_outlook(rates: &RateSchedule, now: i64) -> DemoResult<Value> {
    let index = rates
        .windows()
        .iter()
        .position(|window| window.start_time() <= now && now < window.end_time())
        .ok_or("sample rate unavailable")?;
    let current = &rates.windows()[index];
    let next = rates
        .windows()
        .get(index + 1)
        .ok_or("next sample rate unavailable")?;
    Ok(json!({
        "current": { "price": rate_label(current.rate()), "until": clock_label(current.end_time()) },
        "next": { "price": rate_label(next.rate()), "starts": clock_label(next.start_time()), "ends": clock_label(next.end_time()) },
    }))
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

pub(crate) fn snapshot(missing: bool) -> DemoResult<Value> {
    let rates = sample_rates()?;
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
        let summary = ledger.pricing(&VehicleId::new(id)?, &rates)?;
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
                "date": format!("Sep {}", start / 24), "date_order": start,
                "time": format!("{}hr ({} - {})", end - start, session_clock_label(start), session_clock_label(end)),
                "vehicle": name, "energy": quantity(priced.session.observed_ac),
                "cost": cost, "reason": reason, "lines": lines,
            })
        }).collect();
        vehicles.push(json!({
            "id": id, "name": name, "model": model, "battery": battery, "state": state,
            "week": week.to_json(), "month": month.to_json(), "sessions": sessions,
        }));
    }
    Ok(json!({
        "scenario": if missing { "missing" } else { "complete" }, "vehicles": vehicles,
        "totals": { "week": total_week.to_json(), "month": total_month.to_json() },
        "periods": { "week": "Sep 7 to 13", "month": "Sep 1 to 13", "as_of": "Sep 13, 2026 · 2 pm" },
        "rates": rate_outlook(&rates, DEMO_NOW)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::{DEMO_NOW, MONTH_START, PeriodTotals, rate_outlook, sample_rates, snapshot};

    #[test]
    fn week_and_month_use_core_quotes_and_exclude_prior_month() {
        let data = snapshot(false).unwrap();
        assert_eq!(data["totals"]["week"]["priced_energy"], "90");
        assert_eq!(data["totals"]["week"]["priced_subtotal"], "$20.40");
        assert_eq!(data["totals"]["month"]["priced_energy"], "118");
        assert_eq!(data["totals"]["month"]["priced_subtotal"], "$26.00");
        assert_eq!(data["vehicles"][0]["week"]["priced_subtotal"], "$11.60");
        assert_eq!(data["vehicles"][0]["month"]["priced_subtotal"], "$15.60");
        assert_eq!(data["vehicles"][1]["month"]["priced_subtotal"], "$10.40");
        for vehicle in data["vehicles"].as_array().unwrap() {
            assert_eq!(vehicle["sessions"].as_array().unwrap().len(), 4);
            assert!(!vehicle["sessions"].to_string().contains("prior-month"));
        }
    }

    #[test]
    fn missing_reading_is_excluded_from_both_periods_and_replay_restores_totals() {
        let original = snapshot(false).unwrap();
        let missing = snapshot(true).unwrap();
        assert_eq!(missing["totals"]["week"]["priced_energy"], "80");
        assert_eq!(missing["totals"]["week"]["priced_subtotal"], "$17.20");
        assert_eq!(missing["totals"]["month"]["priced_energy"], "108");
        assert_eq!(missing["totals"]["month"]["priced_subtotal"], "$22.80");
        assert_eq!(missing["totals"]["week"]["held"], 1);
        assert_eq!(missing["totals"]["month"]["held"], 1);
        assert_eq!(missing["vehicles"][1]["month"]["priced_subtotal"], "$7.20");
        let held = missing["vehicles"][1]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|session| session["id"] == "vehicle-b-session-02")
            .unwrap();
        assert!(held["cost"].is_null());
        assert_eq!(held["energy"], "10");
        assert_eq!(snapshot(false).unwrap(), original);
    }

    #[test]
    fn sample_rate_lookup_switches_at_boundary_and_handles_midnight() {
        let rates = sample_rates().unwrap();
        let before = rate_outlook(&rates, DEMO_NOW).unwrap();
        assert_eq!(before["current"]["price"], "$0.20");
        assert_eq!(before["current"]["until"], "3 pm");
        assert_eq!(before["next"]["price"], "$0.40");
        assert_eq!(before["next"]["ends"], "midnight");
        let at_boundary = rate_outlook(&rates, DEMO_NOW + 1).unwrap();
        assert_eq!(at_boundary["current"]["price"], "$0.40");
        assert_eq!(at_boundary["next"]["price"], "$0.20");
        assert_eq!(
            rate_outlook(&rates, 14 * 24).unwrap()["current"]["price"],
            "$0.20"
        );
        assert!(rate_outlook(&rates, -1).is_err());
        assert_eq!(
            PeriodTotals::from_sessions(&[], MONTH_START, DEMO_NOW)
                .unwrap()
                .to_json()["priced_subtotal"],
            "$0.00"
        );
    }
}
