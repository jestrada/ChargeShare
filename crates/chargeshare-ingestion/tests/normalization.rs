use chargeshare_core::{
    ChargeType, ConnectionId, CounterProblem, CounterReading, Energy, Event, EventKind,
    ExclusionReason, Ledger, OwnerId, QualityFlag, VehicleId,
};
use chargeshare_ingestion::{
    AcceptedEvidence, Annotation, DeviceMapping, INPUT_CONTRACT_VERSION, MAX_ANNOTATIONS,
    MAX_MANIFEST_BYTES, MAX_MAPPINGS, MAX_PAYLOAD_BYTES, Manifest, ManifestConfig,
    NORMALIZER_VERSION, NormalizedChargeType, NormalizedCounter, NormalizedEventKind,
    RejectionReason, SafeCounterProblem, TIMESTAMP_VERSION, normalize,
};
use serde_json::{Value, json};

fn manifest_config() -> ManifestConfig {
    serde_json::from_slice(include_bytes!("fixtures/manifest-v1.json")).unwrap()
}

fn manifest() -> Manifest {
    Manifest::new(manifest_config()).unwrap()
}

fn receiver_payload(device: &str, created_at: &str, counter: Option<&str>) -> Vec<u8> {
    let mut fields = vec![json!({
        "key": "DetailedChargeState",
        "value": {"detailedChargeStateValue": "DetailedChargeStateComplete"}
    })];
    if let Some(counter) = counter {
        fields.push(json!({"key": "ACChargingEnergyIn", "value": {"stringValue": counter}}));
    }
    serde_json::to_vec(&json!({
        "data": fields,
        "createdAt": created_at,
        "vin": device,
        "isResend": false
    }))
    .unwrap()
}

fn evidence(counter: Option<&str>) -> AcceptedEvidence {
    normalize(
        &manifest(),
        Some(b"device-1"),
        &receiver_payload("device-1", "2026-09-01T12:00:00Z", counter),
    )
    .unwrap()
}

fn ledger_for(manifest: &Manifest) -> Ledger {
    let mut ledger = Ledger::default();
    for mapping in &manifest.config().mappings {
        ledger
            .register(
                VehicleId::new(&mapping.vehicle).unwrap(),
                OwnerId::new(&mapping.owner).unwrap(),
            )
            .unwrap();
    }
    ledger
}

fn core_event(
    vehicle: &str,
    connection: &str,
    position: u64,
    time: i64,
    kind: EventKind,
    charge_type: ChargeType,
    counter: Option<&str>,
) -> Event {
    Event {
        vehicle: Some(VehicleId::new(vehicle).unwrap()),
        connection: ConnectionId::new(connection).unwrap(),
        position,
        time,
        kind,
        charge_type,
        counter: counter.map(CounterReading::parse_kwh),
    }
}

#[test]
fn candidate_golden_records_equal_direct_core_events_with_separate_provenance() {
    let manifest = manifest();
    let fixture: Value =
        serde_json::from_slice(include_bytes!("fixtures/candidate-v.json")).unwrap();
    assert_eq!(
        fixture["provenance"]["actual_receiver_transport_verified"],
        false
    );
    assert_eq!(
        fixture["provenance"]["receiver_revision"],
        "bd076fe1494841707528449560c4a19d0d426da4"
    );
    let mut normalized = ledger_for(&manifest);
    let mut direct = ledger_for(&manifest);
    for (index, record) in fixture["records"].as_array().unwrap().iter().enumerate() {
        let device = record["key"].as_str().unwrap();
        let accepted = normalize(
            &manifest,
            Some(device.as_bytes()),
            &serde_json::to_vec(&record["payload"]).unwrap(),
        )
        .unwrap();
        let vehicle = if index % 2 == 0 {
            "vehicle-a"
        } else {
            "vehicle-b"
        };
        let sample = index / 2;
        let counter = [
            ["100.00", "100.25", "100.50"],
            ["200.00", "200.125", "200.25"],
        ][index % 2][sample];
        let expected = core_event(
            vehicle,
            "connection-1",
            sample as u64 + 1,
            1_788_264_000 + sample as i64 * 300,
            [EventKind::Start, EventKind::Sample, EventKind::End][sample],
            ChargeType::Ac,
            Some(counter),
        );
        assert_eq!(accepted.event.to_core().unwrap(), expected);
        assert_eq!(accepted.provenance.device, device);
        assert_eq!(accepted.provenance.input_contract, INPUT_CONTRACT_VERSION);
        assert_eq!(accepted.provenance.normalizer_version, NORMALIZER_VERSION);
        assert_eq!(accepted.provenance.timestamp_version, TIMESTAMP_VERSION);
        assert!(!accepted.provenance.is_resend);
        manifest.validate_evidence(&accepted).unwrap();
        normalized
            .ingest(accepted.event.to_core().unwrap())
            .unwrap();
        direct.ingest(expected).unwrap();
    }
    assert_eq!(normalized, direct);
    for (vehicle, expected_energy) in [("vehicle-a", "0.5"), ("vehicle-b", "0.25")] {
        let summary = normalized
            .summary(&VehicleId::new(vehicle).unwrap())
            .unwrap();
        assert_eq!(
            summary.observed_ac,
            Energy::parse_kwh(expected_energy).unwrap()
        );
        assert_eq!(summary.eligible_shared_charger, Energy::ZERO);
        assert!(summary.held_or_excluded[0].quality_flags.is_empty());
        assert!(
            summary.held_or_excluded[0]
                .exclusion_reasons
                .contains(&ExclusionReason::UnconfirmedCharger)
        );
    }
}

#[test]
fn fictional_mapping_aliases_versions_and_full_u64_positions_are_validated() {
    for field in [
        "device",
        "vehicle",
        "owner",
        "connection",
        "mapping_version",
        "manifest_version",
    ] {
        for value in ["", "bad/alias", "nonascii-é", &"x".repeat(65)] {
            let mut config = serde_json::to_value(manifest_config()).unwrap();
            match field {
                "device" | "vehicle" | "owner" => config["mappings"][0][field] = json!(value),
                "connection" => config["annotations"][0][field] = json!(value),
                _ => config[field] = json!(value),
            }
            assert_eq!(
                Manifest::from_json(&serde_json::to_vec(&config).unwrap()).unwrap_err(),
                RejectionReason::InvalidManifest
            );
        }
    }
    let mut config = manifest_config();
    config.annotations[0].position = u64::MAX.to_string();
    let manifest = Manifest::new(config).unwrap();
    let accepted = normalize(
        &manifest,
        Some(b"device-1"),
        &receiver_payload("device-1", "2026-09-01T12:00:00Z", Some("0")),
    )
    .unwrap();
    assert_eq!(accepted.event.to_core().unwrap().position, u64::MAX);
    assert_eq!(
        serde_json::to_value(&accepted.event).unwrap()["position"],
        u64::MAX.to_string()
    );
    for position in ["01", "+1", "-1", "1.0", "18446744073709551616", ""] {
        let mut config = manifest_config();
        config.annotations[0].position = position.to_owned();
        assert_eq!(
            Manifest::new(config).unwrap_err(),
            RejectionReason::InvalidManifest
        );
    }
    for field in ["input_contract", "normalizer_version", "timestamp_version"] {
        let mut config = serde_json::to_value(manifest_config()).unwrap();
        config[field] = json!("unsupported-v999");
        assert_eq!(
            Manifest::from_json(&serde_json::to_vec(&config).unwrap()).unwrap_err(),
            RejectionReason::IncompatibleVersion
        );
    }
}

#[test]
fn duplicate_devices_vehicles_and_source_associations_fail_before_normalization() {
    let mut config = manifest_config();
    config.mappings.push(config.mappings[0].clone());
    assert_eq!(
        Manifest::new(config).unwrap_err(),
        RejectionReason::DuplicateMapping
    );
    let mut config = manifest_config();
    config.mappings[1].vehicle = config.mappings[0].vehicle.clone();
    assert_eq!(
        Manifest::new(config).unwrap_err(),
        RejectionReason::DuplicateMapping
    );
    for contradictory in [false, true] {
        let mut config = manifest_config();
        let mut annotation = config.annotations[0].clone();
        if contradictory {
            annotation.connection = "contradictory-connection".to_owned();
            annotation.position = "42".to_owned();
        }
        config.annotations.push(annotation);
        assert_eq!(
            Manifest::new(config).unwrap_err(),
            RejectionReason::DuplicateAssociation
        );
    }
    let mut config = manifest_config();
    config.annotations[0].device = "unmapped-device".to_owned();
    assert_eq!(
        Manifest::new(config).unwrap_err(),
        RejectionReason::InvalidManifest
    );
    let mut config = manifest_config();
    config.mappings[1].owner = config.mappings[0].owner.clone();
    Manifest::new(config).unwrap();
}

#[test]
fn canonical_manifest_configuration_is_independent_of_input_order() {
    let first = manifest();
    let mut config = manifest_config();
    config.mappings.reverse();
    config.annotations.reverse();
    let second = Manifest::new(config).unwrap();
    assert_eq!(first.config(), second.config());
    assert_eq!(
        serde_json::to_vec(first.config()).unwrap(),
        serde_json::to_vec(second.config()).unwrap()
    );
}

#[test]
fn missing_unknown_and_disagreeing_identities_return_only_fixed_reasons() {
    let manifest = manifest();
    let known = receiver_payload("device-1", "2026-09-01T12:00:00Z", Some("0"));
    for key in [None, Some(b"".as_slice())] {
        assert_eq!(
            normalize(&manifest, key, &known).unwrap_err(),
            RejectionReason::MissingIdentity
        );
    }
    assert_eq!(
        normalize(&manifest, Some(b"device-2"), &known).unwrap_err(),
        RejectionReason::IdentityMismatch
    );
    let unknown = "fictional-private-identity";
    let payload = receiver_payload(
        unknown,
        "2026-09-01T12:00:00Z",
        Some("fictional-private-counter"),
    );
    let reason = normalize(&manifest, Some(unknown.as_bytes()), &payload).unwrap_err();
    assert_eq!(reason, RejectionReason::UnknownIdentity);
    assert!(!format!("{reason:?} {reason}").contains(unknown));
    for identity in [Value::Null, json!("")] {
        let mut payload: Value = serde_json::from_slice(&known).unwrap();
        payload["vin"] = identity;
        assert_eq!(
            normalize(
                &manifest,
                Some(b"device-1"),
                &serde_json::to_vec(&payload).unwrap()
            )
            .unwrap_err(),
            RejectionReason::MissingIdentity
        );
    }
    let mut payload: Value = serde_json::from_slice(&known).unwrap();
    payload.as_object_mut().unwrap().remove("vin");
    assert_eq!(
        normalize(
            &manifest,
            Some(b"device-1"),
            &serde_json::to_vec(&payload).unwrap()
        )
        .unwrap_err(),
        RejectionReason::MissingIdentity
    );
}

#[test]
fn exact_utc_seconds_reject_fractional_offset_invalid_and_unassociated_times() {
    let manifest = manifest();
    for time in [
        "2026-09-01T12:00:00.0Z",
        "2026-09-01T12:00:00.123456Z",
        "2026-09-01T12:00:00+00:00",
        "2026-09-01T12:00:00z",
        "2026-09-01t12:00:00Z",
        "2026-02-29T12:00:00Z",
        "1900-02-29T12:00:00Z",
        "2026-13-01T12:00:00Z",
        "2026-09-00T12:00:00Z",
        "2026-09-01T24:00:00Z",
        "2026-09-01T12:60:00Z",
        "2026-09-01T12:00:60Z",
        "0000-01-01T00:00:00Z",
        "fictional-private-time",
    ] {
        assert_eq!(
            normalize(
                &manifest,
                Some(b"device-1"),
                &receiver_payload("device-1", time, Some("0"))
            )
            .unwrap_err(),
            RejectionReason::InvalidTimestamp
        );
        let mut config = manifest_config();
        config.annotations[0].created_at = time.to_owned();
        assert_eq!(
            Manifest::new(config).unwrap_err(),
            RejectionReason::InvalidManifest
        );
    }
    assert_eq!(
        normalize(
            &manifest,
            Some(b"device-1"),
            &receiver_payload("device-1", "2026-09-01T12:00:01Z", Some("0"))
        )
        .unwrap_err(),
        RejectionReason::MissingAssociation
    );
    for (time, epoch) in [
        ("0001-01-01T00:00:00Z", -62_135_596_800),
        ("1969-12-31T23:59:59Z", -1),
        ("1970-01-01T00:00:00Z", 0),
        ("2000-02-29T00:00:00Z", 951_782_400),
        ("9999-12-31T23:59:59Z", 253_402_300_799),
    ] {
        let mut config = manifest_config();
        config.annotations[0].created_at = time.to_owned();
        let manifest = Manifest::new(config).unwrap();
        let accepted = normalize(
            &manifest,
            Some(b"device-1"),
            &receiver_payload("device-1", time, Some("0")),
        )
        .unwrap();
        assert_eq!(accepted.event.time, epoch);
        assert_eq!(accepted.provenance.created_at, time);
    }
}

#[test]
fn exact_counter_values_and_core_rejection_categories_survive_without_rejected_text() {
    for (input, canonical) in [
        ("0", "0.000000"),
        ("0001.2300", "1.230000"),
        ("0.000001", "0.000001"),
        ("18446744073709.551615", "18446744073709.551615"),
    ] {
        let accepted = evidence(Some(input));
        assert_eq!(
            accepted.event.counter,
            Some(NormalizedCounter::Valid {
                kwh: canonical.to_owned()
            })
        );
        assert_eq!(
            accepted.event.to_core().unwrap().counter,
            Some(CounterReading::parse_kwh(input))
        );
        assert_eq!(
            serde_json::to_value(&accepted.event).unwrap()["counter"]["kwh"],
            canonical
        );
    }
    for (input, safe_reason, core_reason) in [
        (
            "fictional-private-counter",
            SafeCounterProblem::Invalid,
            CounterProblem::Invalid,
        ),
        (
            "-fictional-private-counter",
            SafeCounterProblem::Negative,
            CounterProblem::Negative,
        ),
        (
            "1.0000001",
            SafeCounterProblem::UnsupportedPrecision,
            CounterProblem::UnsupportedPrecision,
        ),
        (
            "18446744073709.551616",
            SafeCounterProblem::Overflow,
            CounterProblem::Overflow,
        ),
        (
            "18446744073709551616",
            SafeCounterProblem::Overflow,
            CounterProblem::Overflow,
        ),
        ("1e3", SafeCounterProblem::Invalid, CounterProblem::Invalid),
        ("1.", SafeCounterProblem::Invalid, CounterProblem::Invalid),
    ] {
        let accepted = evidence(Some(input));
        assert_eq!(
            accepted.event.counter,
            Some(NormalizedCounter::Rejected {
                reason: safe_reason
            })
        );
        assert_eq!(
            accepted.event.to_core().unwrap().counter,
            Some(CounterReading::Rejected(core_reason))
        );
        assert!(!serde_json::to_string(&accepted).unwrap().contains(input));
        assert!(!format!("{accepted:?}").contains(input));
    }
    assert_eq!(evidence(None).event.counter, None);
}

#[test]
fn unsupported_typed_fields_and_duplicate_json_members_cannot_be_accepted() {
    let manifest = manifest();
    let original: Value = serde_json::from_slice(&receiver_payload(
        "device-1",
        "2026-09-01T12:00:00Z",
        Some("0"),
    ))
    .unwrap();
    for value in [
        json!(1),
        json!(1.25),
        Value::Null,
        json!({"doubleValue": 1.25}),
        json!({"stringValue": null}),
        json!({"stringValue":"0", "detailedChargeStateValue":"DetailedChargeStateComplete"}),
    ] {
        let mut payload = original.clone();
        payload["data"][1]["value"] = value;
        assert_eq!(
            normalize(
                &manifest,
                Some(b"device-1"),
                &serde_json::to_vec(&payload).unwrap()
            )
            .unwrap_err(),
            RejectionReason::UnsupportedPayload
        );
    }
    for (path, value) in [
        ("createdAt", json!(1_788_264_000)),
        ("isResend", json!("false")),
        ("vin", json!(42)),
        ("location", json!("fictional-private-location")),
    ] {
        let mut payload = original.clone();
        payload[path] = value;
        assert_eq!(
            normalize(
                &manifest,
                Some(b"device-1"),
                &serde_json::to_vec(&payload).unwrap()
            )
            .unwrap_err(),
            RejectionReason::UnsupportedPayload
        );
    }
    for raw in [
        r#"{"data":[],"createdAt":"2026-09-01T12:00:00Z","vin":"device-1","vin":"device-1","isResend":false}"#,
        r#"{"data":[{"key":"ACChargingEnergyIn","key":"ACChargingEnergyIn","value":{"stringValue":"0"}}],"createdAt":"2026-09-01T12:00:00Z","vin":"device-1","isResend":false}"#,
        r#"{"data":[{"key":"ACChargingEnergyIn","value":{"stringValue":"0","stringValue":"fictional-private-counter"}}],"createdAt":"2026-09-01T12:00:00Z","vin":"device-1","isResend":false}"#,
    ] {
        assert_eq!(
            normalize(&manifest, Some(b"device-1"), raw.as_bytes()).unwrap_err(),
            RejectionReason::UnsupportedPayload
        );
    }
    for key in ["VehicleName", "BatteryLevel", "ACChargingEnergyIn"] {
        let mut payload = original.clone();
        payload["data"] = json!([
            {"key":key,"value":{"stringValue":"fictional-private-value"}},
            {"key":key,"value":{"stringValue":"fictional-private-value"}}
        ]);
        assert_eq!(
            normalize(
                &manifest,
                Some(b"device-1"),
                &serde_json::to_vec(&payload).unwrap()
            )
            .unwrap_err(),
            RejectionReason::DuplicateSourceField
        );
    }
    for state in [
        json!("Complete"),
        json!(42),
        json!({"stringValue":"DetailedChargeStateComplete"}),
    ] {
        let mut payload = original.clone();
        payload["data"][0]["value"] = state;
        assert_eq!(
            normalize(
                &manifest,
                Some(b"device-1"),
                &serde_json::to_vec(&payload).unwrap()
            )
            .unwrap_err(),
            RejectionReason::UnsupportedPayload
        );
    }
}

#[test]
fn discarded_names_battery_fields_and_rejected_payload_text_never_enter_safe_evidence() {
    let manifest = manifest();
    let sentinel = "fictional-private-free-text";
    let mut payload: Value = serde_json::from_slice(&receiver_payload(
        "device-1",
        "2026-09-01T12:00:00Z",
        Some(sentinel),
    ))
    .unwrap();
    payload["data"].as_array_mut().unwrap().extend([
        json!({"key":"VehicleName","value":{"stringValue":sentinel}}),
        json!({"key":"BatteryLevel","value":{"stringValue":sentinel}}),
    ]);
    let accepted = normalize(
        &manifest,
        Some(b"device-1"),
        &serde_json::to_vec(&payload).unwrap(),
    )
    .unwrap();
    assert!(!serde_json::to_string(&accepted).unwrap().contains(sentinel));
    assert!(!format!("{accepted:?}").contains(sentinel));
    payload["data"]
        .as_array_mut()
        .unwrap()
        .push(json!({"key":"Location","value":{"stringValue":sentinel}}));
    let reason = normalize(
        &manifest,
        Some(b"device-1"),
        &serde_json::to_vec(&payload).unwrap(),
    )
    .unwrap_err();
    assert!(!format!("{reason:?} {reason}").contains(sentinel));
    let mut config = manifest_config();
    config.mappings[0].device = sentinel.to_owned();
    config.mapping_version = sentinel.to_owned();
    assert!(
        !format!(
            "{config:?} {:?} {:?}",
            config.mappings[0], config.annotations[0]
        )
        .contains(sentinel)
    );
    let mut accepted = evidence(Some("0"));
    accepted.event.vehicle = sentinel.to_owned();
    accepted.event.connection = sentinel.to_owned();
    accepted.event.position = sentinel.to_owned();
    accepted.event.counter = Some(NormalizedCounter::Valid {
        kwh: sentinel.to_owned(),
    });
    accepted.provenance.owner = sentinel.to_owned();
    accepted.provenance.device = sentinel.to_owned();
    accepted.provenance.created_at = sentinel.to_owned();
    assert!(!format!("{accepted:?}").contains(sentinel));
    assert!(manifest.validate_evidence(&accepted).is_err());
}

#[test]
fn interior_omission_keeps_current_core_chain_and_adds_no_quality_flag() {
    let manifest = manifest();
    let mut normalized = ledger_for(&manifest);
    let mut direct = ledger_for(&manifest);
    for (time, kind, counter, position) in [
        ("2026-09-01T12:00:00Z", EventKind::Start, Some("0"), 1),
        ("2026-09-01T12:05:00Z", EventKind::Sample, None, 2),
        ("2026-09-01T12:10:00Z", EventKind::End, Some("10"), 3),
    ] {
        let accepted = normalize(
            &manifest,
            Some(b"device-1"),
            &receiver_payload("device-1", time, counter),
        )
        .unwrap();
        assert_eq!(accepted.event.counter.is_none(), counter.is_none());
        normalized
            .ingest(accepted.event.to_core().unwrap())
            .unwrap();
        direct
            .ingest(core_event(
                "vehicle-a",
                "connection-1",
                position,
                1_788_264_000 + (position as i64 - 1) * 300,
                kind,
                ChargeType::Ac,
                counter,
            ))
            .unwrap();
    }
    assert_eq!(normalized, direct);
    let sessions = normalized
        .sessions(&VehicleId::new("vehicle-a").unwrap())
        .unwrap();
    assert!(sessions[0].quality_flags.is_empty());
    assert_eq!(sessions[0].observed_ac, Energy::parse_kwh("10").unwrap());
}

#[test]
fn missing_boundaries_rollback_invalid_dc_and_ambiguous_events_match_core_semantics() {
    for (start, end, charge_type, core_charge_type, flag) in [
        (
            None,
            Some("10"),
            NormalizedChargeType::Ac,
            ChargeType::Ac,
            Some(QualityFlag::MissingBaseline),
        ),
        (
            Some("0"),
            None,
            NormalizedChargeType::Ac,
            ChargeType::Ac,
            Some(QualityFlag::MissingTerminalSample),
        ),
        (
            Some("10"),
            Some("5"),
            NormalizedChargeType::Ac,
            ChargeType::Ac,
            Some(QualityFlag::CounterRollback),
        ),
        (
            Some("0"),
            Some("fictional-invalid-counter"),
            NormalizedChargeType::Ac,
            ChargeType::Ac,
            Some(QualityFlag::InvalidCounter(CounterProblem::Invalid)),
        ),
        (
            Some("0"),
            Some("10"),
            NormalizedChargeType::Dc,
            ChargeType::Dc,
            None,
        ),
        (
            Some("0"),
            Some("10"),
            NormalizedChargeType::Ambiguous,
            ChargeType::Ambiguous,
            None,
        ),
    ] {
        let mut config = manifest_config();
        for annotation in &mut config.annotations {
            annotation.charge_type = charge_type;
        }
        let manifest = Manifest::new(config).unwrap();
        let mut normalized = ledger_for(&manifest);
        let mut direct = ledger_for(&manifest);
        for (time, kind, position, epoch, counter) in [
            (
                "2026-09-01T12:00:00Z",
                EventKind::Start,
                1,
                1_788_264_000,
                start,
            ),
            (
                "2026-09-01T12:10:00Z",
                EventKind::End,
                3,
                1_788_264_600,
                end,
            ),
        ] {
            let accepted = normalize(
                &manifest,
                Some(b"device-1"),
                &receiver_payload("device-1", time, counter),
            )
            .unwrap();
            normalized
                .ingest(accepted.event.to_core().unwrap())
                .unwrap();
            direct
                .ingest(core_event(
                    "vehicle-a",
                    "connection-1",
                    position,
                    epoch,
                    kind,
                    core_charge_type,
                    counter,
                ))
                .unwrap();
        }
        assert_eq!(normalized, direct);
        let session = normalized
            .sessions(&VehicleId::new("vehicle-a").unwrap())
            .unwrap()
            .remove(0);
        if let Some(flag) = flag {
            assert!(session.quality_flags.contains(&flag));
        }
        assert!(!session.is_eligible());
    }
}

#[test]
fn completion_resend_and_unused_end_annotations_do_not_create_boundaries() {
    let mut config = manifest_config();
    config.annotations[0].kind = NormalizedEventKind::Sample;
    let manifest = Manifest::new(config).unwrap();
    let mut payload: Value = serde_json::from_slice(&receiver_payload(
        "device-1",
        "2026-09-01T12:00:00Z",
        Some("0"),
    ))
    .unwrap();
    let initial = normalize(
        &manifest,
        Some(b"device-1"),
        &serde_json::to_vec(&payload).unwrap(),
    )
    .unwrap();
    payload["isResend"] = json!(true);
    let resent = normalize(
        &manifest,
        Some(b"device-1"),
        &serde_json::to_vec(&payload).unwrap(),
    )
    .unwrap();
    assert_eq!(initial.event, resent.event);
    assert_ne!(initial.provenance.is_resend, resent.provenance.is_resend);
    assert_eq!(initial.event.kind, NormalizedEventKind::Sample);
    let mut ledger = ledger_for(&manifest);
    assert!(
        ledger
            .sessions(&VehicleId::new("vehicle-a").unwrap())
            .unwrap()
            .is_empty()
    );
    ledger.ingest(initial.event.to_core().unwrap()).unwrap();
    ledger.ingest(resent.event.to_core().unwrap()).unwrap();
    let session = ledger
        .sessions(&VehicleId::new("vehicle-a").unwrap())
        .unwrap()
        .remove(0);
    assert_eq!(session.end_time, None);
    assert!(
        session
            .quality_flags
            .contains(&QualityFlag::MissingConnectionStart)
    );
    assert!(
        session
            .quality_flags
            .contains(&QualityFlag::MissingConnectionEnd)
    );
}

#[test]
fn same_position_variants_preserve_cross_connection_conflicts_and_vehicle_isolation() {
    let mut config = manifest_config();
    config.annotations[1].position = "1".to_owned();
    config.annotations[1].connection = "connection-2".to_owned();
    let manifest = Manifest::new(config).unwrap();
    let first = normalize(
        &manifest,
        Some(b"device-1"),
        &receiver_payload("device-1", "2026-09-01T12:00:00Z", Some("0")),
    )
    .unwrap();
    let conflicting = normalize(
        &manifest,
        Some(b"device-1"),
        &receiver_payload("device-1", "2026-09-01T12:05:00Z", Some("1")),
    )
    .unwrap();
    let other_vehicle = normalize(
        &manifest,
        Some(b"device-2"),
        &receiver_payload("device-2", "2026-09-01T12:00:00Z", Some("0")),
    )
    .unwrap();
    assert_eq!(first.event.position, conflicting.event.position);
    assert_ne!(first.event, conflicting.event);
    assert_ne!(first.event, other_vehicle.event);
    let mut forward = ledger_for(&manifest);
    let mut reverse = ledger_for(&manifest);
    for accepted in [&first, &conflicting, &other_vehicle] {
        forward.ingest(accepted.event.to_core().unwrap()).unwrap();
    }
    for accepted in [&other_vehicle, &conflicting, &first, &first] {
        reverse.ingest(accepted.event.to_core().unwrap()).unwrap();
    }
    assert_eq!(forward, reverse);
    let sessions = forward
        .sessions(&VehicleId::new("vehicle-a").unwrap())
        .unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().all(|session| {
        session
            .quality_flags
            .contains(&QualityFlag::ConflictingEvidence)
    }));
    let other = forward
        .sessions(&VehicleId::new("vehicle-b").unwrap())
        .unwrap()
        .remove(0);
    assert!(
        !other
            .quality_flags
            .contains(&QualityFlag::ConflictingEvidence)
    );
}

#[test]
fn public_evidence_revalidation_rejects_every_frozen_association_change() {
    let manifest = manifest();
    let accepted = evidence(Some("0.000001"));
    let serialized = serde_json::to_value(&accepted).unwrap();
    for (section, field, replacement) in [
        ("event", "vehicle", json!("vehicle-b")),
        ("event", "connection", json!("different-connection")),
        ("event", "position", json!("42")),
        ("event", "time", json!(0)),
        ("event", "kind", json!("end")),
        ("event", "charge_type", json!("dc")),
        (
            "event",
            "counter",
            json!({"status":"valid","kwh":"0.0000001"}),
        ),
        ("provenance", "device", json!("device-2")),
        ("provenance", "owner", json!("owner-b")),
        ("provenance", "created_at", json!("2026-09-01T12:05:00Z")),
        ("provenance", "input_contract", json!("different-version")),
        ("provenance", "mapping_version", json!("different-version")),
        ("provenance", "manifest_version", json!("different-version")),
        (
            "provenance",
            "normalizer_version",
            json!("different-version"),
        ),
        (
            "provenance",
            "timestamp_version",
            json!("different-version"),
        ),
    ] {
        let mut changed = serialized.clone();
        changed[section][field] = replacement;
        let changed: AcceptedEvidence = serde_json::from_value(changed).unwrap();
        assert!(manifest.validate_evidence(&changed).is_err());
    }
    let restored: AcceptedEvidence = serde_json::from_value(serialized).unwrap();
    assert_eq!(restored, accepted);
    manifest.validate_evidence(&restored).unwrap();
}

#[test]
fn explicit_input_bounds_and_manifest_duplicate_fields_fail_safely() {
    let manifest = manifest();
    assert_eq!(
        normalize(
            &manifest,
            Some(b"device-1"),
            &vec![b' '; MAX_PAYLOAD_BYTES + 1]
        )
        .unwrap_err(),
        RejectionReason::InputTooLarge
    );
    assert_eq!(
        Manifest::from_json(&vec![b' '; MAX_MANIFEST_BYTES + 1]).unwrap_err(),
        RejectionReason::InputTooLarge
    );
    assert_eq!(
        normalize(
            &manifest,
            Some(&[b'x'; 65]),
            &receiver_payload("device-1", "2026-09-01T12:00:00Z", Some("0"))
        )
        .unwrap_err(),
        RejectionReason::InputTooLarge
    );
    let original = String::from_utf8(serde_json::to_vec(&manifest_config()).unwrap()).unwrap();
    let duplicate = original.replacen('{', r#"{"mapping_version":"fictional-private-version","#, 1);
    assert_eq!(
        Manifest::from_json(duplicate.as_bytes()).unwrap_err(),
        RejectionReason::InvalidManifest
    );
    let mut config = manifest_config();
    config.mappings.clear();
    assert_eq!(
        Manifest::new(config).unwrap_err(),
        RejectionReason::InvalidManifest
    );
}

#[test]
fn pause_resume_annotations_are_explicit_and_do_not_bridge_connections() {
    let mut config = manifest_config();
    config.annotations.extend([
        Annotation {
            device: "device-1".to_owned(),
            created_at: "2026-09-01T12:02:00Z".to_owned(),
            position: "4".to_owned(),
            connection: "connection-1".to_owned(),
            kind: NormalizedEventKind::Pause,
            charge_type: NormalizedChargeType::Ac,
        },
        Annotation {
            device: "device-1".to_owned(),
            created_at: "2026-09-01T12:03:00Z".to_owned(),
            position: "5".to_owned(),
            connection: "connection-1".to_owned(),
            kind: NormalizedEventKind::Resume,
            charge_type: NormalizedChargeType::Ac,
        },
        Annotation {
            device: "device-1".to_owned(),
            created_at: "2026-09-01T13:00:00Z".to_owned(),
            position: "6".to_owned(),
            connection: "connection-2".to_owned(),
            kind: NormalizedEventKind::Start,
            charge_type: NormalizedChargeType::Ac,
        },
        Annotation {
            device: "device-1".to_owned(),
            created_at: "2026-09-01T13:10:00Z".to_owned(),
            position: "7".to_owned(),
            connection: "connection-2".to_owned(),
            kind: NormalizedEventKind::End,
            charge_type: NormalizedChargeType::Ac,
        },
    ]);
    let manifest = Manifest::new(config).unwrap();
    let mut ledger = ledger_for(&manifest);
    for (time, counter) in [
        ("2026-09-01T13:10:00Z", "104"),
        ("2026-09-01T12:10:00Z", "10"),
        ("2026-09-01T12:03:00Z", "3"),
        ("2026-09-01T12:00:00Z", "0"),
        ("2026-09-01T13:00:00Z", "100"),
        ("2026-09-01T12:02:00Z", "3"),
    ] {
        let accepted = normalize(
            &manifest,
            Some(b"device-1"),
            &receiver_payload("device-1", time, Some(counter)),
        )
        .unwrap();
        ledger.ingest(accepted.event.to_core().unwrap()).unwrap();
    }
    let sessions = ledger
        .sessions(&VehicleId::new("vehicle-a").unwrap())
        .unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].observed_ac, Energy::parse_kwh("10").unwrap());
    assert_eq!(sessions[1].observed_ac, Energy::parse_kwh("4").unwrap());
    assert!(
        sessions
            .iter()
            .all(|session| session.quality_flags.is_empty())
    );
}

#[test]
fn shared_owner_registration_is_unambiguous_and_empty_annotations_create_no_evidence() {
    let mut config = manifest_config();
    config.annotations.clear();
    config.mappings.push(DeviceMapping {
        device: "device-3".to_owned(),
        vehicle: "vehicle-c".to_owned(),
        owner: "owner-a".to_owned(),
    });
    let manifest = Manifest::new(config).unwrap();
    assert_eq!(
        normalize(
            &manifest,
            Some(b"device-1"),
            &receiver_payload("device-1", "2026-09-01T12:00:00Z", Some("0"))
        )
        .unwrap_err(),
        RejectionReason::MissingAssociation
    );
    assert!(
        ledger_for(&manifest)
            .sessions(&VehicleId::new("vehicle-a").unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn independent_ten_and_four_kwh_match_direct_core_in_every_arrival_permutation() {
    let manifest = manifest();
    let observations = [
        (
            "device-1",
            "vehicle-a",
            "2026-09-01T12:00:00Z",
            1,
            1_788_264_000,
            EventKind::Start,
            "0",
        ),
        (
            "device-2",
            "vehicle-b",
            "2026-09-01T12:00:00Z",
            1,
            1_788_264_000,
            EventKind::Start,
            "0",
        ),
        (
            "device-1",
            "vehicle-a",
            "2026-09-01T12:10:00Z",
            3,
            1_788_264_600,
            EventKind::End,
            "10",
        ),
        (
            "device-2",
            "vehicle-b",
            "2026-09-01T12:10:00Z",
            3,
            1_788_264_600,
            EventKind::End,
            "4",
        ),
    ];
    let mut reference = ledger_for(&manifest);
    let mut accepted = Vec::new();
    for (device, vehicle, time, position, epoch, kind, counter) in observations {
        accepted.push(
            normalize(
                &manifest,
                Some(device.as_bytes()),
                &receiver_payload(device, time, Some(counter)),
            )
            .unwrap(),
        );
        reference
            .ingest(core_event(
                vehicle,
                "connection-1",
                position,
                epoch,
                kind,
                ChargeType::Ac,
                Some(counter),
            ))
            .unwrap();
    }
    let mut tested = 0;
    for first in 0..4 {
        for second in 0..4 {
            for third in 0..4 {
                for fourth in 0..4 {
                    let order = [first, second, third, fourth];
                    let unique: std::collections::BTreeSet<_> = order.into_iter().collect();
                    if unique.len() != 4 {
                        continue;
                    }
                    let mut replayed = ledger_for(&manifest);
                    for index in order {
                        replayed
                            .ingest(accepted[index].event.to_core().unwrap())
                            .unwrap();
                        replayed
                            .ingest(accepted[index].event.to_core().unwrap())
                            .unwrap();
                    }
                    assert_eq!(replayed, reference);
                    for (vehicle, owner, expected) in [
                        ("vehicle-a", "owner-a", "10"),
                        ("vehicle-b", "owner-b", "4"),
                    ] {
                        let summary = replayed.summary(&VehicleId::new(vehicle).unwrap()).unwrap();
                        assert_eq!(summary.owner, OwnerId::new(owner).unwrap());
                        assert_eq!(summary.observed_ac, Energy::parse_kwh(expected).unwrap());
                        assert_eq!(summary.eligible_shared_charger, Energy::ZERO);
                        assert!(summary.held_or_excluded[0].quality_flags.is_empty());
                    }
                    tested += 1;
                }
            }
        }
    }
    assert_eq!(tested, 24);
}

#[test]
fn normalized_text_fields_reject_noncanonical_or_unvalidated_corruption() {
    let original = evidence(Some("1.000001")).event;
    for field in ["vehicle", "connection"] {
        for invalid in ["", "fictional/private", "é", &"x".repeat(65)] {
            let mut value = serde_json::to_value(&original).unwrap();
            value[field] = json!(invalid);
            let changed: chargeshare_ingestion::NormalizedEvent =
                serde_json::from_value(value).unwrap();
            assert_eq!(
                changed.to_core().unwrap_err(),
                RejectionReason::InvalidNormalizedEvent
            );
        }
    }
    for invalid in ["01", "+1", "-1", "1.0", "18446744073709551616", ""] {
        let mut changed = original.clone();
        changed.position = invalid.to_owned();
        assert_eq!(
            changed.to_core().unwrap_err(),
            RejectionReason::InvalidNormalizedEvent
        );
    }
    for invalid in [
        "1",
        "01.000000",
        "1.0000000",
        "-1",
        "fictional-private-counter",
    ] {
        let mut changed = original.clone();
        changed.counter = Some(NormalizedCounter::Valid {
            kwh: invalid.to_owned(),
        });
        assert_eq!(
            changed.to_core().unwrap_err(),
            RejectionReason::InvalidNormalizedEvent
        );
    }
    let mut value = serde_json::to_value(&original).unwrap();
    value["position"] = json!(1);
    assert!(serde_json::from_value::<chargeshare_ingestion::NormalizedEvent>(value).is_err());
    let mut value = serde_json::to_value(&original).unwrap();
    value["counter"]["kwh"] = json!(1.000001);
    assert!(serde_json::from_value::<chargeshare_ingestion::NormalizedEvent>(value).is_err());
}

#[test]
fn explicit_mapping_and_annotation_count_limits_fail_before_duplicate_validation() {
    let mut config = manifest_config();
    config.mappings = vec![config.mappings[0].clone(); MAX_MAPPINGS + 1];
    assert_eq!(
        Manifest::new(config).unwrap_err(),
        RejectionReason::InvalidManifest
    );
    let mut config = manifest_config();
    config.annotations = vec![config.annotations[0].clone(); MAX_ANNOTATIONS + 1];
    assert_eq!(
        Manifest::new(config).unwrap_err(),
        RejectionReason::InvalidManifest
    );
}
