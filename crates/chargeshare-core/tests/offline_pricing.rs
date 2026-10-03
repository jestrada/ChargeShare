use chargeshare_core::*;

fn vehicle(alias: &str) -> VehicleId {
    VehicleId::new(alias).unwrap()
}

fn energy(value: &str) -> Energy {
    Energy::parse_kwh(value).unwrap()
}

fn configured() -> Ledger {
    let mut ledger = Ledger::default();
    for (car, owner) in [("vehicle-a", "owner-a"), ("vehicle-b", "owner-b")] {
        ledger
            .register(vehicle(car), OwnerId::new(owner).unwrap())
            .unwrap();
    }
    ledger
}

fn event(
    car: &str,
    connection: &str,
    position: u64,
    time: i64,
    kind: EventKind,
    counter: Option<&str>,
) -> Event {
    Event {
        vehicle: Some(vehicle(car)),
        connection: ConnectionId::new(connection).unwrap(),
        position,
        time,
        kind,
        charge_type: ChargeType::Ac,
        counter: counter.map(CounterReading::parse_kwh),
    }
}

fn complete(car: &str, connection: &str, position: u64, kwh: &str) -> Vec<Event> {
    vec![
        event(car, connection, position, 10, EventKind::Start, Some("0")),
        event(car, connection, position + 1, 20, EventKind::End, Some(kwh)),
    ]
}

fn ingest(ledger: &mut Ledger, events: impl IntoIterator<Item = Event>) {
    for event in events {
        ledger.ingest(event).unwrap();
    }
}

fn classify(
    ledger: &mut Ledger,
    car: &str,
    connection: &str,
    classification: ChargerClassification,
) {
    ledger
        .classify(
            &vehicle(car),
            &SessionId {
                vehicle: vehicle(car),
                connection: ConnectionId::new(connection).unwrap(),
            },
            classification,
        )
        .unwrap();
}

fn confirm(ledger: &mut Ledger, car: &str, connection: &str) {
    classify(
        ledger,
        car,
        connection,
        ChargerClassification::SharedCharger,
    );
}

fn window(version: &str, start: i64, end: i64, rate: &str) -> RateWindow {
    RateWindow::new(version, start, end, UsdRate::parse_per_kwh(rate).unwrap()).unwrap()
}

fn flat(rate: &str) -> RateSchedule {
    RateSchedule::new(vec![window("fictional-flat-v1", 0, 30, rate)]).unwrap()
}

fn split() -> RateSchedule {
    RateSchedule::new(vec![
        window("fictional-low-v1", 0, 15, "0.20"),
        window("fictional-high-v1", 15, 30, "0.40"),
    ])
    .unwrap()
}

#[test]
fn flat_rate_prices_only_the_confirmed_vehicle_and_preserves_scope() {
    let mut ledger = configured();
    let a = complete("vehicle-a", "connection-1", 1, "10");
    let b = complete("vehicle-b", "connection-1", 1, "4");
    ingest(
        &mut ledger,
        [a[0].clone(), b[0].clone(), a[1].clone(), b[1].clone()],
    );
    confirm(&mut ledger, "vehicle-a", "connection-1");
    let a = ledger.pricing(&vehicle("vehicle-a"), &flat("0.2")).unwrap();
    assert_eq!(a.rounded_priced_subtotal.as_cents(), 200);
    assert_eq!(
        a.exact_priced_subtotal.to_string(),
        "2.0000000000000000 USD"
    );
    assert_eq!(a.priced_energy, energy("10"));
    assert_eq!(a.owner.as_str(), "owner-a");
    assert_eq!(a.evidence_label, EVIDENCE_LABEL);
    assert!(!a.has_unpriced_sessions());
    let b = ledger.pricing(&vehicle("vehicle-b"), &flat("0.2")).unwrap();
    assert_eq!(b.priced_energy, Energy::ZERO);
    assert_eq!(b.owner.as_str(), "owner-b");
    assert!(b.has_unpriced_sessions());
    assert_eq!(b.sessions[0].quote, Err(PricingHold::IneligibleSession));
    assert!(
        b.sessions[0]
            .session
            .exclusion_reasons
            .contains(&ExclusionReason::UnconfirmedCharger)
    );
    confirm(&mut ledger, "vehicle-b", "connection-1");
    assert_eq!(
        ledger
            .pricing(&vehicle("vehicle-b"), &flat("0.2"))
            .unwrap()
            .rounded_priced_subtotal
            .as_cents(),
        80
    );
    assert_eq!(
        ledger.pricing(&vehicle("vehicle-a"), &flat("0.2")).unwrap(),
        a
    );
}

#[test]
fn boundary_readings_preserve_energy_and_explain_each_rate_and_version() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event(
                "vehicle-a",
                "connection-1",
                1,
                10,
                EventKind::Start,
                Some("100"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                2,
                15,
                EventKind::Sample,
                Some("104"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                3,
                20,
                EventKind::End,
                Some("110"),
            ),
        ],
    );
    confirm(&mut ledger, "vehicle-a", "connection-1");
    let summary = ledger.pricing(&vehicle("vehicle-a"), &split()).unwrap();
    assert_eq!(summary.priced_energy, energy("10"));
    assert_eq!(summary.rounded_priced_subtotal.to_string(), "3.20 USD");
    let quote = summary.sessions[0].quote.as_ref().unwrap();
    assert_eq!(quote.lines.len(), 2);
    assert_eq!(quote.lines[0].energy, energy("4"));
    assert_eq!(quote.lines[1].energy, energy("6"));
    assert_eq!(quote.lines[0].rate_window.version(), "fictional-low-v1");
    assert_eq!(quote.lines[1].rate_window.version(), "fictional-high-v1");
    assert_eq!(quote.lines[0].start_time, 10);
    assert_eq!(quote.lines[0].end_time, 15);
    assert_eq!(quote.lines[1].start_time, 15);
    assert_eq!(quote.lines[1].end_time, 20);
    assert_eq!(
        quote.lines[0].exact_cost.to_string(),
        "0.8000000000000000 USD"
    );
    assert_eq!(
        quote.lines[1].exact_cost.to_string(),
        "2.4000000000000000 USD"
    );
    assert_eq!(quote.exact_cost, summary.exact_priced_subtotal);
    assert_eq!(
        quote.lines[0]
            .energy
            .checked_add(quote.lines[1].energy)
            .unwrap(),
        summary.priced_energy
    );
}

#[test]
fn an_unknown_split_is_held_until_late_boundary_evidence_resolves_it() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event(
                "vehicle-a",
                "connection-1",
                1,
                10,
                EventKind::Start,
                Some("0"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                3,
                20,
                EventKind::End,
                Some("10"),
            ),
        ],
    );
    confirm(&mut ledger, "vehicle-a", "connection-1");
    let held = ledger.pricing(&vehicle("vehicle-a"), &split()).unwrap();
    assert_eq!(
        held.sessions[0].quote,
        Err(PricingHold::UnresolvedRateBoundary {
            start_time: 10,
            end_time: 20
        })
    );
    assert_eq!(held.priced_energy, Energy::ZERO);
    assert_eq!(held.exact_priced_subtotal, ExactUsd::ZERO);
    assert_eq!(held.sessions[0].session.observed_ac, energy("10"));
    ingest(
        &mut ledger,
        [event(
            "vehicle-a",
            "connection-1",
            2,
            15,
            EventKind::Sample,
            Some("4"),
        )],
    );
    let resolved = ledger.pricing(&vehicle("vehicle-a"), &split()).unwrap();
    assert_eq!(resolved.rounded_priced_subtotal.as_cents(), 320);
    assert!(!resolved.has_unpriced_sessions());
    assert!(held.has_unpriced_sessions());
    assert_eq!(resolved.sessions[0].session.id, held.sessions[0].session.id);
}

#[test]
fn an_unpriced_interval_holds_the_entire_session_but_not_other_sessions() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event("vehicle-a", "held", 1, 5, EventKind::Start, Some("0")),
            event("vehicle-a", "held", 2, 10, EventKind::Sample, Some("2")),
            event("vehicle-a", "held", 3, 20, EventKind::End, Some("10")),
            event("vehicle-a", "priced", 4, 5, EventKind::Start, Some("0")),
            event("vehicle-a", "priced", 5, 10, EventKind::End, Some("1")),
        ],
    );
    for connection in ["held", "priced"] {
        confirm(&mut ledger, "vehicle-a", connection);
    }
    let summary = ledger.pricing(&vehicle("vehicle-a"), &split()).unwrap();
    assert_eq!(summary.priced_energy, energy("1"));
    assert_eq!(summary.rounded_priced_subtotal.as_cents(), 20);
    assert_eq!(summary.sessions.len(), 2);
    assert!(summary.has_unpriced_sessions());
    assert_eq!(
        summary.sessions[0].quote,
        Err(PricingHold::UnresolvedRateBoundary {
            start_time: 10,
            end_time: 20
        })
    );
}

#[test]
fn absent_rates_and_gaps_never_fall_back_to_a_nearby_rate() {
    let mut ledger = configured();
    ingest(&mut ledger, complete("vehicle-a", "connection-1", 1, "10"));
    confirm(&mut ledger, "vehicle-a", "connection-1");
    for windows in [
        vec![],
        vec![window("before", 0, 10, "0.1")],
        vec![window("after", 11, 30, "0.1")],
        vec![
            window("before", 0, 9, "0.1"),
            window("after", 11, 30, "0.1"),
        ],
    ] {
        let summary = ledger
            .pricing(&vehicle("vehicle-a"), &RateSchedule::new(windows).unwrap())
            .unwrap();
        assert_eq!(
            summary.sessions[0].quote,
            Err(PricingHold::MissingRate {
                start_time: 10,
                end_time: 20
            })
        );
    }
    let schedule = RateSchedule::new(vec![
        window("before", 0, 12, "0.1"),
        window("after", 18, 30, "0.1"),
    ])
    .unwrap();
    assert_eq!(
        ledger
            .pricing(&vehicle("vehicle-a"), &schedule)
            .unwrap()
            .sessions[0]
            .quote,
        Err(PricingHold::UnresolvedRateBoundary {
            start_time: 10,
            end_time: 20
        })
    );
}

#[test]
fn positive_energy_at_one_tick_is_held_even_with_a_known_rate() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event(
                "vehicle-a",
                "connection-1",
                1,
                10,
                EventKind::Start,
                Some("0"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                2,
                10,
                EventKind::End,
                Some("1"),
            ),
        ],
    );
    confirm(&mut ledger, "vehicle-a", "connection-1");
    let summary = ledger.pricing(&vehicle("vehicle-a"), &flat("0.2")).unwrap();
    assert!(summary.sessions[0].session.is_eligible());
    assert_eq!(
        summary.sessions[0].quote,
        Err(PricingHold::NonIncreasingTime {
            start_time: 10,
            end_time: 10
        })
    );
    assert_eq!(summary.priced_energy, Energy::ZERO);
}

#[test]
fn zero_consumption_needs_no_rate_and_zero_rates_add_no_fees() {
    let mut ledger = configured();
    ingest(&mut ledger, complete("vehicle-a", "connection-1", 1, "0"));
    confirm(&mut ledger, "vehicle-a", "connection-1");
    let empty_rates = RateSchedule::new(vec![]).unwrap();
    let summary = ledger.pricing(&vehicle("vehicle-a"), &empty_rates).unwrap();
    assert!(!summary.has_unpriced_sessions());
    assert!(summary.sessions[0].quote.as_ref().unwrap().lines.is_empty());
    assert_eq!(summary.exact_priced_subtotal, ExactUsd::ZERO);
    ingest(&mut ledger, complete("vehicle-a", "connection-2", 3, "10"));
    confirm(&mut ledger, "vehicle-a", "connection-2");
    let free = ledger.pricing(&vehicle("vehicle-a"), &flat("0")).unwrap();
    assert_eq!(free.priced_energy, energy("10"));
    assert_eq!(free.rounded_priced_subtotal.as_cents(), 0);
    assert!(!free.has_unpriced_sessions());
}

#[test]
fn rounding_occurs_once_after_summing_exact_session_costs() {
    let mut ledger = configured();
    for (position, connection) in [(1, "first"), (3, "second")] {
        ingest(
            &mut ledger,
            complete("vehicle-a", connection, position, "1"),
        );
        confirm(&mut ledger, "vehicle-a", connection);
    }
    let summary = ledger
        .pricing(&vehicle("vehicle-a"), &flat("0.004"))
        .unwrap();
    assert_eq!(
        summary.exact_priced_subtotal.to_string(),
        "0.0080000000000000 USD"
    );
    assert_eq!(summary.rounded_priced_subtotal.as_cents(), 1);
    for session in summary.sessions {
        assert_eq!(
            session
                .quote
                .unwrap()
                .exact_cost
                .round_half_up_to_cents()
                .as_cents(),
            0
        );
    }
}

#[test]
fn excluded_and_incomplete_evidence_never_gets_a_quote() {
    let base = complete("vehicle-a", "connection-1", 1, "10");
    let mut dc = base.clone();
    dc[1].charge_type = ChargeType::Dc;
    let mut ambiguous = base.clone();
    ambiguous[1].charge_type = ChargeType::Ambiguous;
    let mut invalid = base.clone();
    invalid[1].counter = Some(CounterReading::Rejected(CounterProblem::Invalid));
    let mut no_baseline = base.clone();
    no_baseline[0].counter = None;
    let mut rollback = base.clone();
    rollback[0].counter = Some(CounterReading::parse_kwh("11"));
    let mut conflict = base.clone();
    let mut disagreement = base[1].clone();
    disagreement.counter = Some(CounterReading::parse_kwh("9"));
    conflict.push(disagreement);
    for events in [
        dc,
        ambiguous,
        invalid,
        no_baseline,
        rollback,
        conflict,
        vec![base[0].clone()],
        vec![base[1].clone()],
    ] {
        let mut ledger = configured();
        ingest(&mut ledger, events);
        confirm(&mut ledger, "vehicle-a", "connection-1");
        let before = ledger.sessions(&vehicle("vehicle-a")).unwrap();
        let summary = ledger.pricing(&vehicle("vehicle-a"), &flat("0.2")).unwrap();
        assert_eq!(summary.sessions[0].session, before[0]);
        assert!(!summary.sessions[0].session.is_eligible());
        assert_eq!(
            summary.sessions[0].quote,
            Err(PricingHold::IneligibleSession)
        );
        assert_eq!(summary.priced_energy, Energy::ZERO);
        assert_eq!(summary.exact_priced_subtotal, ExactUsd::ZERO);
    }
    let mut other = configured();
    ingest(&mut other, base);
    classify(
        &mut other,
        "vehicle-a",
        "connection-1",
        ChargerClassification::OtherCharger,
    );
    let result = other.pricing(&vehicle("vehicle-a"), &flat("0.2")).unwrap();
    assert_eq!(
        result.sessions[0].quote,
        Err(PricingHold::IneligibleSession)
    );
    assert!(
        result.sessions[0]
            .session
            .exclusion_reasons
            .contains(&ExclusionReason::OtherCharger)
    );
}

#[test]
fn reverse_order_duplicates_and_window_order_preserve_identical_quotes() {
    let mut events = complete("vehicle-a", "connection-1", 1, "10");
    events[1].position = 3;
    events.push(event(
        "vehicle-a",
        "connection-1",
        2,
        15,
        EventKind::Sample,
        Some("4"),
    ));
    events.extend(complete("vehicle-b", "connection-1", 1, "4"));
    let mut forward = configured();
    let mut reverse = configured();
    ingest(&mut forward, events.clone());
    ingest(
        &mut reverse,
        events.iter().rev().cloned().chain(events.iter().cloned()),
    );
    for ledger in [&mut forward, &mut reverse] {
        for car in ["vehicle-a", "vehicle-b"] {
            confirm(ledger, car, "connection-1");
        }
    }
    let reversed_rates =
        RateSchedule::new(split().windows().iter().rev().cloned().collect()).unwrap();
    for car in ["vehicle-a", "vehicle-b"] {
        assert_eq!(
            forward.pricing(&vehicle(car), &split()).unwrap(),
            reverse.pricing(&vehicle(car), &reversed_rates).unwrap()
        );
    }
}

#[test]
fn pauses_and_new_connections_do_not_invent_or_bridge_energy() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event("vehicle-a", "first", 1, 10, EventKind::Start, Some("100")),
            event("vehicle-a", "first", 2, 12, EventKind::Pause, Some("102")),
            event("vehicle-a", "first", 3, 14, EventKind::Resume, None),
            event("vehicle-a", "first", 4, 20, EventKind::End, Some("110")),
            event("vehicle-a", "second", 5, 21, EventKind::Start, Some("1")),
            event("vehicle-a", "second", 6, 25, EventKind::End, Some("5")),
        ],
    );
    for connection in ["first", "second"] {
        confirm(&mut ledger, "vehicle-a", connection);
    }
    let summary = ledger.pricing(&vehicle("vehicle-a"), &flat("0.2")).unwrap();
    assert_eq!(summary.priced_energy, energy("14"));
    assert_eq!(summary.rounded_priced_subtotal.as_cents(), 280);
    assert_eq!(
        summary.sessions[0].quote.as_ref().unwrap().lines[1].start_time,
        12
    );
    assert_eq!(
        summary.sessions[1].quote.as_ref().unwrap().lines[0].energy,
        energy("4")
    );
}

#[test]
fn tariff_revisions_produce_new_results_without_mutating_prior_snapshots() {
    let mut ledger = configured();
    ingest(&mut ledger, complete("vehicle-a", "connection-1", 1, "10"));
    confirm(&mut ledger, "vehicle-a", "connection-1");
    let before = ledger.clone();
    let original = ledger.pricing(&vehicle("vehicle-a"), &flat("0.2")).unwrap();
    let revised_rates = RateSchedule::new(vec![window("fictional-flat-v2", 0, 30, "0.3")]).unwrap();
    let revised = ledger
        .pricing(&vehicle("vehicle-a"), &revised_rates)
        .unwrap();
    assert_eq!(original.rounded_priced_subtotal.as_cents(), 200);
    assert_eq!(revised.rounded_priced_subtotal.as_cents(), 300);
    assert_eq!(
        original.sessions[0].quote.as_ref().unwrap().lines[0]
            .rate_window
            .version(),
        "fictional-flat-v1"
    );
    assert_eq!(
        revised.sessions[0].quote.as_ref().unwrap().lines[0]
            .rate_window
            .version(),
        "fictional-flat-v2"
    );
    assert_eq!(ledger, before);
}

#[test]
fn aggregate_overflows_fail_without_mutating_evidence_or_returning_a_partial_total() {
    let mut ledger = configured();
    for (position, connection) in [(1, "first"), (3, "second")] {
        ingest(
            &mut ledger,
            complete("vehicle-a", connection, position, "18446744073709.551615"),
        );
        confirm(&mut ledger, "vehicle-a", connection);
    }
    let before = ledger.clone();
    assert_eq!(
        ledger.pricing(&vehicle("vehicle-a"), &flat("1844674407.3709551615")),
        Err(PricingError::Money(MoneyError::CostOverflow))
    );
    assert_eq!(
        ledger.pricing(&vehicle("vehicle-a"), &flat("0.2")),
        Err(PricingError::EnergyOverflow)
    );
    assert_eq!(ledger, before);
}

#[test]
fn adjacent_same_rate_versions_still_need_a_boundary_and_extreme_ticks_do_not_overflow() {
    let mut ledger = configured();
    ingest(&mut ledger, complete("vehicle-a", "connection-1", 1, "10"));
    confirm(&mut ledger, "vehicle-a", "connection-1");
    let adjacent = RateSchedule::new(vec![
        window("v1", 0, 15, "0.2"),
        window("v2", 15, 30, "0.2"),
    ])
    .unwrap();
    assert!(
        ledger
            .pricing(&vehicle("vehicle-a"), &adjacent)
            .unwrap()
            .has_unpriced_sessions()
    );
    let mut extreme = configured();
    ingest(
        &mut extreme,
        [
            event(
                "vehicle-a",
                "connection-1",
                1,
                i64::MIN,
                EventKind::Start,
                Some("0"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                2,
                i64::MAX,
                EventKind::End,
                Some("10"),
            ),
        ],
    );
    confirm(&mut extreme, "vehicle-a", "connection-1");
    let rates = RateSchedule::new(vec![window("extreme", i64::MIN, i64::MAX, "0.2")]).unwrap();
    assert_eq!(
        extreme
            .pricing(&vehicle("vehicle-a"), &rates)
            .unwrap()
            .rounded_priced_subtotal
            .as_cents(),
        200
    );
}

#[test]
fn unknown_vehicle_fails_and_empty_registered_vehicle_has_an_empty_subtotal() {
    let ledger = configured();
    let before = ledger.clone();
    assert_eq!(
        ledger.pricing(&vehicle("unknown"), &flat("0.2")),
        Err(PricingError::Ledger(LedgerError::UnknownVehicle))
    );
    let empty = ledger.pricing(&vehicle("vehicle-a"), &flat("0.2")).unwrap();
    assert!(empty.sessions.is_empty());
    assert!(!empty.has_unpriced_sessions());
    assert_eq!(empty.priced_energy, Energy::ZERO);
    assert_eq!(empty.rounded_priced_subtotal.as_cents(), 0);
    assert_eq!(ledger, before);
}
