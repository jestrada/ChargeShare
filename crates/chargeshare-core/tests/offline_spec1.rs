use chargeshare_core::*;

fn vehicle(alias: &str) -> VehicleId {
    VehicleId::new(alias).unwrap()
}
fn owner(alias: &str) -> OwnerId {
    OwnerId::new(alias).unwrap()
}
fn connection(alias: &str) -> ConnectionId {
    ConnectionId::new(alias).unwrap()
}
fn energy(value: &str) -> Energy {
    Energy::parse_kwh(value).unwrap()
}
fn configured() -> Ledger {
    let mut ledger = Ledger::default();
    ledger
        .register(vehicle("vehicle-a"), owner("owner-a"))
        .unwrap();
    ledger
        .register(vehicle("vehicle-b"), owner("owner-b"))
        .unwrap();
    ledger
}
fn event(
    v: &str,
    c: &str,
    position: u64,
    time: i64,
    kind: EventKind,
    counter: Option<&str>,
) -> Event {
    Event {
        vehicle: Some(vehicle(v)),
        connection: connection(c),
        position,
        time,
        kind,
        charge_type: ChargeType::Ac,
        counter: counter.map(CounterReading::parse_kwh),
        location: LocationSample::Valid(LocationPoint::new(0.0, 0.0).unwrap()),
    }
}
fn complete(v: &str, c: &str, position: u64, start: &str, end: &str) -> Vec<Event> {
    vec![
        event(v, c, position, 10, EventKind::Start, Some(start)),
        event(v, c, position + 1, 20, EventKind::End, Some(end)),
    ]
}
fn ingest(ledger: &mut Ledger, events: impl IntoIterator<Item = Event>) {
    for event in events {
        ledger.ingest(event).unwrap();
    }
}
fn configure_shared_charger(ledger: &mut Ledger, car: &str) {
    let owner = OwnerId::new(if car == "vehicle-a" {
        "owner-a"
    } else {
        "owner-b"
    })
    .unwrap();
    ledger.configure_charger(
        owner,
        ChargerGeofence::new(LocationPoint::new(0.0, 0.0).unwrap(), 100).unwrap(),
    );
}
fn session(ledger: &Ledger, v: &str) -> Session {
    ledger.sessions(&vehicle(v)).unwrap().remove(0)
}

#[test]
fn interleaved_ten_and_four_are_separate_and_independently_classified() {
    let mut ledger = configured();
    let a = complete("vehicle-a", "connection-1", 1, "0", "10");
    let b = complete("vehicle-b", "connection-1", 1, "0", "4");
    ingest(
        &mut ledger,
        [a[0].clone(), b[0].clone(), a[1].clone(), b[1].clone()],
    );
    let a_summary = ledger.summary(&vehicle("vehicle-a")).unwrap();
    let b_summary = ledger.summary(&vehicle("vehicle-b")).unwrap();
    assert_eq!(a_summary.observed_ac, energy("10"));
    assert_eq!(b_summary.observed_ac, energy("4"));
    assert_eq!(a_summary.owner, owner("owner-a"));
    assert_eq!(b_summary.owner, owner("owner-b"));
    assert_eq!(a_summary.eligible_shared_charger, Energy::ZERO);
    assert_eq!(b_summary.eligible_shared_charger, Energy::ZERO);
    assert_eq!(session(&ledger, "vehicle-a").evidence_label, EVIDENCE_LABEL);
    assert!(a_summary.evidence_label.contains("Synthetic review"));
    assert!(a_summary.evidence_label.contains("accuracy unvalidated"));
    configure_shared_charger(&mut ledger, "vehicle-a");
    assert_eq!(
        ledger
            .summary(&vehicle("vehicle-a"))
            .unwrap()
            .eligible_shared_charger,
        energy("10")
    );
    assert_eq!(
        ledger
            .summary(&vehicle("vehicle-b"))
            .unwrap()
            .eligible_shared_charger,
        Energy::ZERO
    );
    configure_shared_charger(&mut ledger, "vehicle-b");
    assert_eq!(
        ledger
            .summary(&vehicle("vehicle-b"))
            .unwrap()
            .eligible_shared_charger,
        energy("4")
    );
}

#[test]
fn rejected_identity_and_duplicate_registration_leave_all_state_unchanged() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        complete("vehicle-a", "connection-1", 1, "0", "10"),
    );
    let before = ledger.clone();
    assert_eq!(
        ledger.register(vehicle("vehicle-a"), owner("owner-b")),
        Err(LedgerError::DuplicateVehicle)
    );
    assert_eq!(
        ledger.register(vehicle("vehicle-a"), owner("owner-a")),
        Err(LedgerError::DuplicateVehicle)
    );
    let mut absent = event(
        "vehicle-a",
        "connection-1",
        3,
        30,
        EventKind::Sample,
        Some("999"),
    );
    absent.vehicle = None;
    assert_eq!(ledger.ingest(absent), Err(LedgerError::MissingVehicle));
    let unknown = event(
        "fictional-unknown",
        "connection-1",
        3,
        30,
        EventKind::Sample,
        Some("999"),
    );
    assert_eq!(ledger.ingest(unknown), Err(LedgerError::UnknownVehicle));
    assert_eq!(ledger, before);
    assert_eq!(LedgerError::UnknownVehicle.to_string(), "UnknownVehicle");
    for alias in ["", " real data ", "fictional@example.invalid"] {
        assert!(VehicleId::new(alias).is_err());
    }
}

#[test]
fn energy_parsing_is_exact_checked_and_never_rounds() {
    assert_eq!(energy("0.000001").to_string(), "0.000001 kWh");
    assert_eq!(energy("0001.23").to_string(), "1.230000 kWh");
    assert_eq!(energy("0.1").checked_add(energy("0.2")), Ok(energy("0.3")));
    assert_eq!(
        energy("18446744073709.551615").checked_add(energy("0.000001")),
        Err(CounterProblem::Overflow)
    );
    for value in [
        "NaN", "inf", "", "1e3", "+1", " 1", "1 ", ".1", "1.", "1.2.3",
    ] {
        assert_eq!(
            Energy::parse_kwh(value),
            Err(CounterProblem::Invalid),
            "{value}"
        );
    }
    assert_eq!(Energy::parse_kwh("-1"), Err(CounterProblem::Negative));
    assert_eq!(
        Energy::parse_kwh("0.0000001"),
        Err(CounterProblem::UnsupportedPrecision)
    );
    assert_eq!(
        Energy::parse_kwh("18446744073709.551616"),
        Err(CounterProblem::Overflow)
    );
    assert_eq!(
        Energy::parse_kwh("18446744073709551616"),
        Err(CounterProblem::Overflow)
    );
}

#[test]
fn scoped_reads_never_mutate_other_vehicle() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        complete("vehicle-a", "connection-1", 1, "0", "10"),
    );
    ingest(
        &mut ledger,
        complete("vehicle-b", "connection-1", 1, "0", "4"),
    );
    let before = ledger.clone();
    assert_eq!(
        ledger.sessions(&vehicle("unknown")),
        Err(LedgerError::UnknownVehicle)
    );
    assert_eq!(
        ledger.summary(&vehicle("unknown")),
        Err(LedgerError::UnknownVehicle)
    );
    assert_eq!(ledger, before);
    assert!(
        ledger
            .sessions(&vehicle("vehicle-a"))
            .unwrap()
            .iter()
            .all(|s| s.id.vehicle == vehicle("vehicle-a"))
    );
    assert_eq!(
        ledger
            .summary(&vehicle("vehicle-b"))
            .unwrap()
            .eligible_shared_charger,
        Energy::ZERO
    );
}

#[test]
fn shuffled_duplicates_and_late_evidence_preserve_ids_classifications_flags_and_totals() {
    let events = vec![
        event(
            "vehicle-a",
            "connection-1",
            1,
            10,
            EventKind::Start,
            Some("0"),
        ),
        event(
            "vehicle-b",
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
            15,
            EventKind::Sample,
            Some("6"),
        ),
        event(
            "vehicle-a",
            "connection-1",
            3,
            20,
            EventKind::End,
            Some("10"),
        ),
        event(
            "vehicle-b",
            "connection-1",
            2,
            20,
            EventKind::End,
            Some("4"),
        ),
    ];
    let mut ordered = configured();
    ingest(&mut ordered, events.clone());
    configure_shared_charger(&mut ordered, "vehicle-a");
    let mut late = configured();
    ingest(&mut late, [events[3].clone()]);
    let stable_id = session(&late, "vehicle-a").id;
    configure_shared_charger(&mut late, "vehicle-a");
    ingest(
        &mut late,
        events.iter().rev().cloned().chain(events.clone()),
    );
    assert_eq!(session(&late, "vehicle-a").id, stable_id);
    for v in ["vehicle-a", "vehicle-b"] {
        assert_eq!(late.sessions(&vehicle(v)), ordered.sessions(&vehicle(v)));
        assert_eq!(late.summary(&vehicle(v)), ordered.summary(&vehicle(v)));
    }
}

#[test]
fn identical_events_across_vehicles_are_not_deduplicated_together() {
    let mut ledger = configured();
    for v in ["vehicle-a", "vehicle-b"] {
        ingest(&mut ledger, complete(v, "connection-1", 1, "0", "10"));
    }
    assert_eq!(
        ledger.summary(&vehicle("vehicle-a")).unwrap().observed_ac,
        energy("10")
    );
    assert_eq!(
        ledger.summary(&vehicle("vehicle-b")).unwrap().observed_ac,
        energy("10")
    );
    assert_ne!(
        session(&ledger, "vehicle-a").id,
        session(&ledger, "vehicle-b").id
    );
}

#[test]
fn same_position_conflicts_hold_all_affected_connections_independent_of_arrival() {
    let mut evidence = complete("vehicle-a", "connection-1", 1, "0", "10");
    evidence.push(event(
        "vehicle-a",
        "connection-1",
        2,
        20,
        EventKind::End,
        Some("9"),
    ));
    evidence.push(event(
        "vehicle-a",
        "connection-2",
        2,
        25,
        EventKind::Start,
        Some("8"),
    ));
    evidence.extend(complete("vehicle-b", "connection-1", 1, "0", "4"));
    let mut forward = configured();
    ingest(&mut forward, evidence.clone());
    let mut reverse = configured();
    ingest(&mut reverse, evidence.into_iter().rev());
    for ledger in [&mut forward, &mut reverse] {
        configure_shared_charger(ledger, "vehicle-a");
    }
    assert_eq!(forward, reverse);
    assert_eq!(
        forward.sessions(&vehicle("vehicle-a")),
        reverse.sessions(&vehicle("vehicle-a"))
    );
    for s in forward.sessions(&vehicle("vehicle-a")).unwrap() {
        assert!(s.quality_flags.contains(&QualityFlag::ConflictingEvidence));
        assert!(!s.is_eligible());
    }
    assert_eq!(
        forward.summary(&vehicle("vehicle-b")).unwrap().observed_ac,
        energy("4")
    );
    assert!(
        !session(&forward, "vehicle-b")
            .quality_flags
            .contains(&QualityFlag::ConflictingEvidence)
    );
}

#[test]
fn pause_resume_stays_in_connection_and_new_connection_never_bridges_counters() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event(
                "vehicle-a",
                "connection-1",
                1,
                1,
                EventKind::Start,
                Some("100"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                2,
                2,
                EventKind::Pause,
                Some("102"),
            ),
            event("vehicle-a", "connection-1", 3, 3, EventKind::Resume, None),
            event(
                "vehicle-a",
                "connection-1",
                4,
                4,
                EventKind::End,
                Some("110"),
            ),
            event(
                "vehicle-a",
                "connection-2",
                5,
                5,
                EventKind::Start,
                Some("1"),
            ),
            event("vehicle-a", "connection-2", 6, 6, EventKind::End, Some("5")),
        ],
    );
    let sessions = ledger.sessions(&vehicle("vehicle-a")).unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].observed_ac, energy("10"));
    assert_eq!(sessions[1].observed_ac, energy("4"));
    assert!(sessions.iter().all(|s| s.quality_flags.is_empty()));
    assert_eq!(
        ledger.summary(&vehicle("vehicle-a")).unwrap().observed_ac,
        energy("14")
    );
}

#[test]
fn dc_ambiguous_and_mixed_type_evidence_is_explicitly_excluded() {
    for (kind, reason) in [
        (ChargeType::Dc, ExclusionReason::DcEvidence),
        (ChargeType::Ambiguous, ExclusionReason::AmbiguousChargeType),
    ] {
        let mut ledger = configured();
        let mut events = complete("vehicle-a", "connection-1", 1, "0", "10");
        for event in &mut events {
            event.charge_type = kind;
        }
        ingest(&mut ledger, events);
        configure_shared_charger(&mut ledger, "vehicle-a");
        let s = session(&ledger, "vehicle-a");
        assert!(s.exclusion_reasons.contains(&reason));
        assert_eq!(s.observed_ac, Energy::ZERO);
        assert!(!s.is_eligible());
    }
    let mut ledger = configured();
    let mut marker = event("vehicle-a", "connection-1", 2, 15, EventKind::Pause, None);
    marker.charge_type = ChargeType::Ambiguous;
    ingest(
        &mut ledger,
        complete("vehicle-a", "connection-1", 1, "0", "10")
            .into_iter()
            .map(|mut e| {
                if e.kind == EventKind::End {
                    e.position = 3;
                }
                e
            })
            .chain([marker]),
    );
    configure_shared_charger(&mut ledger, "vehicle-a");
    assert_eq!(session(&ledger, "vehicle-a").observed_ac, Energy::ZERO);
    assert!(!session(&ledger, "vehicle-a").is_eligible());
}

#[test]
fn rollback_counts_only_valid_positive_deltas_and_classification_cannot_clear_flags() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event(
                "vehicle-a",
                "connection-1",
                1,
                1,
                EventKind::Start,
                Some("0"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                2,
                2,
                EventKind::Sample,
                Some("5"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                3,
                3,
                EventKind::Sample,
                Some("2"),
            ),
            event("vehicle-a", "connection-1", 4, 4, EventKind::End, Some("4")),
        ],
    );
    let flags = session(&ledger, "vehicle-a").quality_flags;
    configure_shared_charger(&mut ledger, "vehicle-a");
    let s = session(&ledger, "vehicle-a");
    assert_eq!(s.quality_flags, flags);
    assert!(flags.contains(&QualityFlag::CounterRollback));
    assert_eq!(s.observed_ac, energy("7"));
    assert_eq!(
        ledger
            .summary(&vehicle("vehicle-a"))
            .unwrap()
            .eligible_shared_charger,
        Energy::ZERO
    );
}

#[test]
fn invalid_negative_precision_and_overflow_counters_retain_safe_flags_no_invented_delta() {
    for (value, problem) in [
        ("NaN", CounterProblem::Invalid),
        ("-1", CounterProblem::Negative),
        ("1.0000001", CounterProblem::UnsupportedPrecision),
        ("18446744073709551616", CounterProblem::Overflow),
    ] {
        let mut ledger = configured();
        ingest(
            &mut ledger,
            [
                event(
                    "vehicle-a",
                    "connection-1",
                    1,
                    1,
                    EventKind::Start,
                    Some("0"),
                ),
                event(
                    "vehicle-a",
                    "connection-1",
                    2,
                    2,
                    EventKind::Sample,
                    Some(value),
                ),
                event(
                    "vehicle-a",
                    "connection-1",
                    3,
                    3,
                    EventKind::Sample,
                    Some("4"),
                ),
                event("vehicle-a", "connection-1", 4, 4, EventKind::End, Some("6")),
            ],
        );
        configure_shared_charger(&mut ledger, "vehicle-a");
        let s = session(&ledger, "vehicle-a");
        assert!(
            s.quality_flags
                .contains(&QualityFlag::InvalidCounter(problem))
        );
        assert_eq!(s.observed_ac, energy("2"));
        assert!(!s.is_eligible());
        assert!(!format!("{:?}", CounterReading::parse_kwh(value)).contains(value));
    }
}

#[test]
fn missing_baseline_terminal_and_boundaries_stay_visible() {
    let cases = [
        (
            vec![
                event("vehicle-a", "connection-1", 1, 1, EventKind::Start, None),
                event("vehicle-a", "connection-1", 2, 2, EventKind::End, Some("4")),
            ],
            QualityFlag::MissingBaseline,
        ),
        (
            vec![
                event(
                    "vehicle-a",
                    "connection-1",
                    1,
                    1,
                    EventKind::Start,
                    Some("0"),
                ),
                event("vehicle-a", "connection-1", 2, 2, EventKind::End, None),
            ],
            QualityFlag::MissingTerminalSample,
        ),
        (
            vec![
                event(
                    "vehicle-a",
                    "connection-1",
                    1,
                    1,
                    EventKind::Sample,
                    Some("1"),
                ),
                event("vehicle-a", "connection-1", 2, 2, EventKind::End, Some("4")),
            ],
            QualityFlag::MissingConnectionStart,
        ),
        (
            vec![
                event(
                    "vehicle-a",
                    "connection-1",
                    1,
                    1,
                    EventKind::Start,
                    Some("0"),
                ),
                event(
                    "vehicle-a",
                    "connection-1",
                    2,
                    2,
                    EventKind::Sample,
                    Some("4"),
                ),
            ],
            QualityFlag::MissingConnectionEnd,
        ),
    ];
    for (events, flag) in cases {
        let mut ledger = configured();
        ingest(&mut ledger, events);
        configure_shared_charger(&mut ledger, "vehicle-a");
        assert!(session(&ledger, "vehicle-a").quality_flags.contains(&flag));
        assert_eq!(
            ledger
                .summary(&vehicle("vehicle-a"))
                .unwrap()
                .eligible_shared_charger,
            Energy::ZERO
        );
        assert!(
            !ledger
                .summary(&vehicle("vehicle-a"))
                .unwrap()
                .held_or_excluded
                .is_empty()
        );
    }
}

#[test]
fn silence_never_fabricates_end_time_baseline_or_consumption() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event(
                "vehicle-a",
                "connection-1",
                1,
                1,
                EventKind::Sample,
                Some("2"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                2,
                1_000_000,
                EventKind::Sample,
                Some("5"),
            ),
        ],
    );
    let s = session(&ledger, "vehicle-a");
    assert_eq!(s.start_time, None);
    assert_eq!(s.end_time, None);
    assert_eq!(s.observed_ac, energy("3"));
    for flag in [
        QualityFlag::MissingBaseline,
        QualityFlag::MissingTerminalSample,
        QualityFlag::MissingConnectionStart,
        QualityFlag::MissingConnectionEnd,
    ] {
        assert!(s.quality_flags.contains(&flag));
    }
    assert_eq!(
        ledger.sessions(&vehicle("vehicle-a")).unwrap(),
        ledger.sessions(&vehicle("vehicle-a")).unwrap()
    );
}

#[test]
fn equal_time_positions_are_ordered_deterministically_and_bad_boundaries_hold() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event(
                "vehicle-a",
                "connection-1",
                3,
                1,
                EventKind::End,
                Some("10"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                1,
                1,
                EventKind::Start,
                Some("0"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                2,
                1,
                EventKind::Sample,
                Some("6"),
            ),
        ],
    );
    assert_eq!(session(&ledger, "vehicle-a").observed_ac, energy("10"));
    assert!(session(&ledger, "vehicle-a").quality_flags.is_empty());
    ingest(
        &mut ledger,
        [event(
            "vehicle-a",
            "connection-1",
            4,
            2,
            EventKind::Sample,
            Some("11"),
        )],
    );
    assert!(
        session(&ledger, "vehicle-a")
            .quality_flags
            .contains(&QualityFlag::InvalidBoundaryOrder)
    );
}

#[test]
fn aggregate_overflow_is_explicit_and_other_charger_is_excluded() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        complete("vehicle-a", "connection-1", 1, "0", "18446744073709.551615"),
    );
    ledger.configure_charger(
        owner("owner-a"),
        ChargerGeofence::new(LocationPoint::new(1.0, 1.0).unwrap(), 100).unwrap(),
    );
    assert!(
        session(&ledger, "vehicle-a")
            .exclusion_reasons
            .contains(&ExclusionReason::OtherCharger)
    );
    ingest(
        &mut ledger,
        complete("vehicle-a", "connection-2", 3, "0", "0.000001"),
    );
    assert_eq!(
        ledger.summary(&vehicle("vehicle-a")),
        Err(LedgerError::EnergyOverflow)
    );
    assert_eq!(
        ledger.summary(&vehicle("vehicle-b")).unwrap().observed_ac,
        Energy::ZERO
    );
}

#[test]
fn session_delta_accumulation_overflow_fails_explicitly() {
    let mut ledger = configured();
    ingest(
        &mut ledger,
        [
            event(
                "vehicle-a",
                "connection-1",
                1,
                1,
                EventKind::Start,
                Some("0"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                2,
                2,
                EventKind::Sample,
                Some("18446744073709.551615"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                3,
                3,
                EventKind::Sample,
                Some("0"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                4,
                4,
                EventKind::End,
                Some("0.000001"),
            ),
        ],
    );
    assert_eq!(
        ledger.sessions(&vehicle("vehicle-a")),
        Err(LedgerError::EnergyOverflow)
    );
}

#[test]
fn interior_conflicts_break_counter_chain_and_retain_all_safe_evidence_reasons() {
    for (charge_type, counter, reason) in [
        (ChargeType::Dc, "5", Some(ExclusionReason::DcEvidence)),
        (
            ChargeType::Ambiguous,
            "5",
            Some(ExclusionReason::AmbiguousChargeType),
        ),
        (ChargeType::Ac, "NaN", None),
    ] {
        let first = event(
            "vehicle-a",
            "connection-1",
            2,
            15,
            EventKind::Sample,
            Some("5"),
        );
        let mut conflict = first.clone();
        conflict.charge_type = charge_type;
        conflict.counter = Some(CounterReading::parse_kwh(counter));
        let evidence = [
            event(
                "vehicle-a",
                "connection-1",
                1,
                10,
                EventKind::Start,
                Some("0"),
            ),
            first,
            conflict,
            event(
                "vehicle-a",
                "connection-1",
                3,
                20,
                EventKind::Sample,
                Some("8"),
            ),
            event(
                "vehicle-a",
                "connection-1",
                4,
                25,
                EventKind::End,
                Some("10"),
            ),
        ];
        let mut forward = configured();
        ingest(&mut forward, evidence.clone());
        let mut reverse = configured();
        ingest(&mut reverse, evidence.into_iter().rev());
        configure_shared_charger(&mut forward, "vehicle-a");
        configure_shared_charger(&mut reverse, "vehicle-a");
        assert_eq!(
            forward.sessions(&vehicle("vehicle-a")),
            reverse.sessions(&vehicle("vehicle-a"))
        );
        let s = session(&forward, "vehicle-a");
        assert_eq!(s.observed_ac, energy("2"));
        assert!(s.quality_flags.contains(&QualityFlag::ConflictingEvidence));
        if let Some(reason) = reason {
            assert!(s.exclusion_reasons.contains(&reason));
        } else {
            assert!(
                s.quality_flags
                    .contains(&QualityFlag::InvalidCounter(CounterProblem::Invalid))
            );
        }
        assert!(!s.is_eligible());
    }
}

fn fictional_geofence(latitude: f64, longitude: f64) -> ChargerGeofence {
    ChargerGeofence::new(LocationPoint::new(latitude, longitude).unwrap(), 100).unwrap()
}

#[test]
fn automatic_location_uses_every_sample_and_preserves_inclusive_radius() {
    let inside = LocationSample::Valid(LocationPoint::new(12.3456789, 45.6789123).unwrap());
    let outside = LocationSample::Valid(LocationPoint::new(12.3556789, 45.6789123).unwrap());
    let edge = LocationSample::Valid(
        LocationPoint::new(
            12.3456789 + (100.0_f64 / 6_371_008.8).to_degrees(),
            45.6789123,
        )
        .unwrap(),
    );
    for (locations, classification, reason, eligible) in [
        (
            [inside, inside, inside],
            ChargerClassification::SharedCharger,
            ClassificationReason::InsideGeofence,
            "10",
        ),
        (
            [outside, outside, outside],
            ChargerClassification::OtherCharger,
            ClassificationReason::OutsideGeofence,
            "0",
        ),
        (
            [edge, edge, edge],
            ChargerClassification::SharedCharger,
            ClassificationReason::InsideGeofence,
            "10",
        ),
        (
            [LocationSample::Missing; 3],
            ChargerClassification::Held,
            ClassificationReason::LocationUnavailable,
            "0",
        ),
        (
            [LocationSample::Invalid; 3],
            ChargerClassification::Held,
            ClassificationReason::LocationUnavailable,
            "0",
        ),
        (
            [inside, LocationSample::Missing, inside],
            ChargerClassification::Held,
            ClassificationReason::LocationAmbiguous,
            "0",
        ),
        (
            [inside, LocationSample::Invalid, inside],
            ChargerClassification::Held,
            ClassificationReason::LocationAmbiguous,
            "0",
        ),
        (
            [inside, outside, inside],
            ChargerClassification::Held,
            ClassificationReason::LocationAmbiguous,
            "0",
        ),
    ] {
        let mut ledger = configured();
        ledger.configure_charger(owner("owner-a"), fictional_geofence(12.3456789, 45.6789123));
        for (index, location) in locations.into_iter().enumerate() {
            let mut sample = event(
                "vehicle-a",
                "location-session",
                index as u64,
                index as i64,
                [EventKind::Start, EventKind::Sample, EventKind::End][index],
                Some(["0", "4", "10"][index]),
            );
            sample.location = location;
            ledger.ingest(sample).unwrap();
        }
        let result = session(&ledger, "vehicle-a");
        assert_eq!(result.classification, classification);
        assert_eq!(result.classification_reason, reason);
        assert!(result.quality_flags.is_empty());
        assert_eq!(
            ledger
                .summary(&vehicle("vehicle-a"))
                .unwrap()
                .eligible_shared_charger,
            energy(eligible)
        );
    }
    let fence = fictional_geofence(12.3456789, 45.6789123);
    assert!(
        !fence.contains(
            LocationPoint::new(
                12.3456789 + (100.001_f64 / 6_371_008.8).to_degrees(),
                45.6789123
            )
            .unwrap()
        )
    );
    for (latitude, longitude) in [
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
        (90.1, 0.0),
        (0.0, -180.1),
    ] {
        assert!(LocationPoint::new(latitude, longitude).is_none());
    }
}

#[test]
fn later_location_enrichment_settles_holds_without_duplicating_energy_or_boundaries() {
    for unavailable in [LocationSample::Missing, LocationSample::Invalid] {
        let mut ledger = configured();
        ledger.configure_charger(owner("owner-a"), fictional_geofence(0.0, 0.0));
        let valid = complete("vehicle-a", "location-session", 1, "0", "10");
        for mut sample in valid.clone() {
            sample.location = unavailable;
            ledger.ingest(sample).unwrap();
        }
        assert_eq!(
            session(&ledger, "vehicle-a").classification_reason,
            ClassificationReason::LocationUnavailable
        );
        ledger.ingest(valid[0].clone()).unwrap();
        assert_eq!(
            session(&ledger, "vehicle-a").classification_reason,
            ClassificationReason::LocationAmbiguous
        );
        ledger.ingest(valid[1].clone()).unwrap();
        let settled = ledger.summary(&vehicle("vehicle-a")).unwrap();
        assert_eq!(settled.eligible_shared_charger, energy("10"));
        ingest(&mut ledger, valid.into_iter().rev());
        assert_eq!(ledger.summary(&vehicle("vehicle-a")).unwrap(), settled);
        assert!(session(&ledger, "vehicle-a").quality_flags.is_empty());
    }
}

#[test]
fn automatic_classification_preserves_conflict_dc_and_ambiguous_charge_exclusions() {
    for charge_type in [ChargeType::Dc, ChargeType::Ambiguous] {
        let mut ledger = configured();
        ledger.configure_charger(owner("owner-a"), fictional_geofence(0.0, 0.0));
        let mut input = complete("vehicle-a", "location-session", 1, "0", "10");
        for event in &mut input {
            event.charge_type = charge_type;
        }
        ingest(&mut ledger, input);
        assert_eq!(
            session(&ledger, "vehicle-a").classification,
            ChargerClassification::SharedCharger
        );
        assert!(!session(&ledger, "vehicle-a").is_eligible());
    }
    let mut ledger = configured();
    ledger.configure_charger(owner("owner-a"), fictional_geofence(0.0, 0.0));
    let input = complete("vehicle-a", "location-session", 1, "0", "10");
    ingest(&mut ledger, input.clone());
    let mut conflicting = input[1].clone();
    conflicting.location = LocationSample::Valid(LocationPoint::new(0.0001, 0.0001).unwrap());
    ledger.ingest(conflicting).unwrap();
    let result = session(&ledger, "vehicle-a");
    assert_eq!(
        result.classification_reason,
        ClassificationReason::LocationAmbiguous
    );
    assert!(
        result
            .quality_flags
            .contains(&QualityFlag::ConflictingEvidence)
    );
    assert!(!result.is_eligible());
}

#[test]
fn owner_geofence_changes_reclassify_all_owned_vehicles_and_leave_other_owners_isolated() {
    let mut ledger = configured();
    ledger
        .register(vehicle("vehicle-a2"), owner("owner-a"))
        .unwrap();
    ledger.configure_charger(owner("owner-a"), fictional_geofence(0.0, 0.0));
    ledger.configure_charger(owner("owner-b"), fictional_geofence(1.0, 1.0));
    for car in ["vehicle-a", "vehicle-a2", "vehicle-b"] {
        ingest(&mut ledger, complete(car, "location-session", 1, "0", "10"));
    }
    let other = ledger.summary(&vehicle("vehicle-b")).unwrap();
    for car in ["vehicle-a", "vehicle-a2"] {
        assert_eq!(
            ledger
                .summary(&vehicle(car))
                .unwrap()
                .eligible_shared_charger,
            energy("10")
        );
    }
    ledger.configure_charger(owner("owner-a"), fictional_geofence(2.0, 2.0));
    for car in ["vehicle-a", "vehicle-a2"] {
        assert_eq!(
            session(&ledger, car).classification,
            ChargerClassification::OtherCharger
        );
    }
    assert_eq!(ledger.summary(&vehicle("vehicle-b")).unwrap(), other);
}
