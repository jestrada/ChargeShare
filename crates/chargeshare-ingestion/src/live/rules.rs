use super::*;
use chargeshare_core::{Energy, Ledger, OwnerId, VehicleId};
use serde_json::json;

fn config_json() -> serde_json::Value {
    json!({
        "kafka_bootstrap": "kafka:9092", "topic_namespace": "unit", "source_epoch": "live-unit-v2",
        "database": "/private/unit/ingestion.sqlite3",
        "mappings": [{"device": "device-a", "vehicle": "vehicle-a", "owner": "owner-a"}],
        "shared_charger": {"charger_id": "charger-a", "geofence": {
            "center": {"status": "valid", "latitude": "0", "longitude": "0"}
        }, "rate_model_versions": []}, "rate_models": []
    })
}

fn config() -> crate::registry_bootstrap::RegistryBootstrap {
    crate::registry_bootstrap::RegistryBootstrap::from_json(
        &serde_json::to_vec(&config_json()).unwrap(),
    )
    .unwrap()
}

fn signal(time: &str) -> Signal {
    Signal {
        vehicle: "vehicle-a".to_owned(),
        created_at: time.to_owned(),
        time: parse_utc_timestamp(time).unwrap().0,
        state: None,
        counter: None,
        ac_power: None,
        location: None,
        odometer: None,
        fast_charger: None,
    }
}

fn parked() -> Signal {
    let mut signal = signal("2026-01-01T00:00:00Z");
    signal.state = Some(ChargeState::Disconnected);
    signal.location = Some(NormalizedLocation::InsideGeofence);
    signal.odometer = Some(OdometerReading::Valid(DecimalValue::parse("100").unwrap()));
    signal
}

fn event(
    manifest: &crate::Manifest,
    state: &mut AdapterState,
    signal: &Signal,
) -> crate::NormalizedEvent {
    let RecordDisposition::Accepted(evidence) = adapt(manifest, state, signal, false).unwrap()
    else {
        panic!("expected an associated event")
    };
    evidence.event
}

#[test]
fn parked_location_survives_unlimited_time_and_checkpoint_without_coordinates() {
    let manifest = config().legacy_manifest().unwrap();
    let mut state = AdapterState::default();
    adapt(&manifest, &mut state, &parked(), false).unwrap();
    let retained = encode(&state).unwrap();
    assert!(!retained.contains("latitude") && !retained.contains("longitude"));
    assert!(retained.contains("odometer") && retained.contains("time"));
    state = serde_json::from_str(&retained).unwrap();
    let mut charging = signal("2026-10-13T00:00:00Z");
    charging.state = Some(ChargeState::Charging);
    let start = event(&manifest, &mut state, &charging);
    assert_eq!(start.kind, NormalizedEventKind::Start);
    assert!(start.location == Some(NormalizedLocation::InsideGeofence));
    assert_eq!(state.location.as_ref().unwrap().time, parked().time);
    charging = signal("2026-10-13T00:01:00Z");
    charging.odometer = Some(OdometerReading::Valid(
        DecimalValue::parse("100.00000001").unwrap(),
    ));
    assert!(event(&manifest, &mut state, &charging).location.is_none());
    state = serde_json::from_str(&encode(&state).unwrap()).unwrap();
    assert!(
        event(&manifest, &mut state, &signal("2026-10-13T00:02:00Z"))
            .location
            .is_none()
    );
    let mut fresh = signal("2026-10-13T00:03:00Z");
    fresh.location = Some(NormalizedLocation::OutsideGeofence);
    assert!(
        event(&manifest, &mut state, &fresh).location == Some(NormalizedLocation::OutsideGeofence)
    );
}

#[test]
fn conflicting_location_classifications_at_one_instant_remain_held_after_restart() {
    let manifest = config().legacy_manifest().unwrap();
    let mut state = AdapterState::default();
    let mut point = parked();
    adapt(&manifest, &mut state, &point, false).unwrap();
    point.location = Some(NormalizedLocation::OutsideGeofence);
    adapt(&manifest, &mut state, &point, false).unwrap();
    state = serde_json::from_str(&encode(&state).unwrap()).unwrap();
    point.location = Some(NormalizedLocation::InsideGeofence);
    adapt(&manifest, &mut state, &point, false).unwrap();
    assert!(state.location.is_none());
}

#[test]
fn late_preconnection_odometer_evidence_cannot_leave_a_stale_parked_point_qualified() {
    let manifest = config().legacy_manifest().unwrap();
    let mut state = AdapterState::default();
    adapt(&manifest, &mut state, &parked(), false).unwrap();
    adapt(
        &manifest,
        &mut state,
        &signal("2026-10-13T00:00:00Z"),
        false,
    )
    .unwrap();
    let mut late = signal("2026-10-12T00:00:00Z");
    late.odometer = Some(OdometerReading::Valid(DecimalValue::parse("101").unwrap()));
    adapt(&manifest, &mut state, &late, false).unwrap();
    state = serde_json::from_str(&encode(&state).unwrap()).unwrap();
    let mut start = signal("2026-10-14T00:00:00Z");
    start.state = Some(ChargeState::Charging);
    assert!(event(&manifest, &mut state, &start).location.is_none());
}

#[test]
fn preconnection_location_and_odometer_commit_and_recover_together_without_input_files() {
    let directory = std::env::temp_dir().join(format!(
        "chargeshare-inline-location-{}",
        std::process::id()
    ));
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let mut json = config_json();
    json["database"] = json!(directory.join("ingestion.sqlite3"));
    let config = crate::registry_bootstrap::RegistryBootstrap::from_json(
        &serde_json::to_vec(&json).unwrap(),
    )
    .unwrap();
    let manifest = config.legacy_manifest().unwrap();
    let source = crate::SourceBinding {
        epoch: config.source_epoch.clone(),
        topic: "unit_V".to_owned(),
        cluster_id: Some("unit-cluster".to_owned()),
        topic_id: None,
        partitions: vec![crate::PartitionStart {
            partition: 0,
            initial_offset: 0,
        }],
    };
    let headers = vec![
        ("vin".to_owned(), b"device-a".to_vec()),
        ("txid".to_owned(), b"parked".to_vec()),
        ("txtype".to_owned(), b"V".to_vec()),
    ];
    let mut store = Store::open(&config.database, &manifest, &source).unwrap();
    let payload = serde_json::to_vec(&json!({"createdAt": "2026-01-01T00:00:00Z", "data": [
        {"key": "DetailedChargeState", "value": {"detailedChargeStateValue": "DetailedChargeStateDisconnected"}},
        {"key": "Location", "value": {"locationValue": {"latitude": 0, "longitude": 0}}},
        {"key": "Odometer", "value": {"stringValue": "100"}}
    ]})).unwrap();
    store
        .ingest_bootstrap(
            &config,
            DeliveryCoordinate {
                partition: 0,
                offset: 0,
            },
            0,
            &headers,
            &payload,
        )
        .unwrap();
    assert!(Store::preserve_live_source(&config.database, &manifest, &source).is_err());
    drop(store);
    let original_bytes = std::fs::read(&config.database).unwrap();
    let (preserved, positions) =
        Store::preserve_live_source(&config.database, &manifest, &source).unwrap();
    assert_eq!(positions[0].next_offset, 1);
    assert!(Store::open(&config.database, &manifest, &source).is_err());
    drop(preserved);
    assert_eq!(std::fs::read(&config.database).unwrap(), original_bytes);
    let mut store = Store::open(&config.database, &manifest, &source).unwrap();
    let mut headers = headers;
    headers[1].1 = b"charging".to_vec();
    let payload = serde_json::to_vec(&json!({"createdAt": "2026-10-13T00:00:00Z", "data": [
        {"key": "DetailedChargeState", "value": {"detailedChargeStateValue": "DetailedChargeStateCharging"}}
    ]})).unwrap();
    store
        .ingest_bootstrap(
            &config,
            DeliveryCoordinate {
                partition: 0,
                offset: 1,
            },
            1,
            &headers,
            &payload,
        )
        .unwrap();
    let session = store
        .sessions(
            &OwnerId::new("owner-a").unwrap(),
            &VehicleId::new("vehicle-a").unwrap(),
        )
        .unwrap()
        .remove(0);
    assert_eq!(
        session.classification,
        chargeshare_core::ChargerClassification::SharedCharger
    );
    let state: String = store
        .connection
        .query_row("SELECT state_json FROM live_adapter_state", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(state.contains("odometer") && state.contains("time"));
    assert!(!state.contains("latitude") && !state.contains("longitude"));
    drop(store);
    let connection = rusqlite::Connection::open(&config.database).unwrap();
    let mut legacy = serde_json::to_value(manifest.config()).unwrap();
    legacy["normalizer_version"] = json!("chargeshare-live-adapter-v1");
    connection
        .execute(
            "UPDATE runtime_metadata SET manifest_json = ?1",
            [serde_json::to_string(&legacy).unwrap()],
        )
        .unwrap();
    drop(connection);
    let legacy_bytes = std::fs::read(&config.database).unwrap();
    assert!(Store::open(&config.database, &manifest, &source).is_err());
    let (preserved, _) = Store::preserve_live_source(&config.database, &manifest, &source).unwrap();
    drop(preserved);
    assert_eq!(std::fs::read(&config.database).unwrap(), legacy_bytes);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn future_invalid_and_missing_odometer_evidence_never_qualifies_an_earlier_point() {
    let manifest = config().legacy_manifest().unwrap();
    for variant in [
        "missing_odometer",
        "invalid_location",
        "invalid_odometer",
        "rollback",
    ] {
        let mut state = AdapterState::default();
        let mut parked = parked();
        if variant == "missing_odometer" {
            parked.odometer = None;
        }
        adapt(&manifest, &mut state, &parked, false).unwrap();
        let mut start = signal("2026-10-13T00:00:00Z");
        start.state = Some(ChargeState::Charging);
        if variant == "invalid_location" {
            start.location = Some(NormalizedLocation::Invalid);
        }
        if variant == "invalid_odometer" {
            start.odometer = Some(OdometerReading::Invalid);
        }
        if variant == "rollback" {
            start.odometer = Some(OdometerReading::Valid(DecimalValue::parse("99").unwrap()));
        }
        let earlier = event(&manifest, &mut state, &start);
        assert!(earlier.location.is_none());
        let mut future = signal("2026-10-13T00:02:00Z");
        future.location = Some(NormalizedLocation::InsideGeofence);
        future.odometer = Some(OdometerReading::Valid(DecimalValue::parse("101").unwrap()));
        event(&manifest, &mut state, &future);
        let late = event(&manifest, &mut state, &signal("2026-10-13T00:01:00Z"));
        assert!(late.location == Some(NormalizedLocation::Invalid));
        assert!(earlier.location.is_none());
    }
}

#[test]
fn same_connection_ac_resolution_requires_power_or_a_rise_and_dc_always_vetoes() {
    let manifest = config().legacy_manifest().unwrap();
    for proof in [
        "counter_rise",
        "power",
        "flat_counter",
        "fast_charger",
        "false_alone",
        "unknown_state",
        "one_reading",
    ] {
        let mut state = AdapterState::default();
        adapt(&manifest, &mut state, &parked(), false).unwrap();
        let vehicle = VehicleId::new("vehicle-a").unwrap();
        let mut ledger = Ledger::default();
        ledger
            .register(vehicle.clone(), OwnerId::new("owner-a").unwrap())
            .unwrap();
        ledger.use_observed_counter_window(&vehicle).unwrap();
        for (index, time) in [
            "2026-10-13T00:00:00Z",
            "2026-10-13T00:01:00Z",
            "2026-10-13T00:02:00Z",
            "2026-10-13T00:03:00Z",
        ]
        .into_iter()
        .enumerate()
        {
            let mut signal = signal(time);
            signal.state = Some(if index == 3 {
                ChargeState::Complete
            } else {
                ChargeState::Charging
            });
            if index == 1 || (index == 2 && proof != "one_reading") {
                signal.counter = Some(normalize_counter(
                    if index == 1 || proof != "counter_rise" {
                        "10"
                    } else {
                        "10.5"
                    },
                ));
            }
            if proof == "power" && index == 2 {
                signal.ac_power = Some(true);
            }
            if proof == "one_reading" && index == 1 {
                signal.ac_power = Some(true);
            }
            if proof == "fast_charger" && index == 2 {
                signal.ac_power = Some(true);
                signal.fast_charger = Some(true);
            }
            if proof == "false_alone" {
                signal.fast_charger = Some(false);
            }
            if proof == "unknown_state" && index == 1 {
                signal.state = Some(ChargeState::Unknown);
                signal.ac_power = Some(true);
            }
            ledger
                .ingest(event(&manifest, &mut state, &signal).to_core().unwrap())
                .unwrap();
            state = serde_json::from_str(&encode(&state).unwrap()).unwrap();
        }
        let session = ledger.sessions(&vehicle).unwrap().remove(0);
        assert_eq!(
            session.is_eligible(),
            matches!(proof, "counter_rise" | "power")
        );
        if proof == "counter_rise" {
            assert_eq!(session.observed_ac, Energy::parse_kwh("0.5").unwrap());
        }
        let result = crate::SessionResult::from(session);
        assert!(result.coverage.as_ref().unwrap().partial_start);
        assert!(result.coverage.as_ref().unwrap().partial_end);
        assert_eq!(
            result.coverage.unwrap().quantization,
            crate::LIVE_QUANTIZATION
        );
        let mut next = signal("2026-10-13T00:04:00Z");
        next.state = Some(ChargeState::Charging);
        assert_eq!(
            event(&manifest, &mut state, &next).charge_type,
            NormalizedChargeType::Unknown
        );
    }
}

#[test]
fn numeric_fields_preserve_json_decimal_text_and_optional_fast_charger_is_strict() {
    for (raw, expected) in [
        (r#"{"doubleValue":1.2345674999999999999}"#, "1.234567"),
        (r#"{"floatValue":12345675e-7}"#, "1.234568"),
    ] {
        let raw = serde_json::value::RawValue::from_string(raw.to_owned()).unwrap();
        assert!(
            normalize_counter(&numeric_text(&raw).unwrap())
                == NormalizedCounter::Valid {
                    kwh: expected.to_owned()
                }
        );
    }
    let duplicate = serde_json::value::RawValue::from_string(
        r#"{"stringValue":"1","stringValue":"2"}"#.to_owned(),
    )
    .unwrap();
    assert!(numeric_text(&duplicate).is_none());
    let config = config();
    let headers = vec![
        ("vin".to_owned(), b"device-a".to_vec()),
        ("txid".to_owned(), b"unit-record".to_vec()),
        ("txtype".to_owned(), b"V".to_vec()),
    ];
    for value in [
        json!({"booleanValue": true}),
        json!({"stringValue": "true"}),
        json!({"booleanValue": false}),
        json!({"invalid": true}),
    ] {
        let payload = serde_json::to_vec(&json!({"createdAt": "2026-10-13T00:00:00Z", "data": [{"key": "FastChargerPresent", "value": value}]})).unwrap();
        let (signal, _, _) = decode_bootstrap(&config, &headers, &payload).unwrap();
        if value.get("invalid").is_some() {
            assert!(signal.state == Some(ChargeState::Unknown));
        } else {
            assert_eq!(
                signal.fast_charger,
                Some(
                    value
                        .get("booleanValue")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(true)
                )
            );
        }
    }
}

#[test]
fn radius_defaults_validates_redacts_and_changes_the_frozen_fingerprint() {
    let config = config();
    let original = config.legacy_manifest().unwrap();
    assert_eq!(
        original.config().owner_chargers[0].geofence.radius_meters,
        100
    );
    let mut changed = config_json();
    changed["shared_charger"]["geofence"]["radius_meters"] = json!(250);
    let changed_manifest = crate::registry_bootstrap::RegistryBootstrap::from_json(
        &serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap()
    .legacy_manifest()
    .unwrap();
    assert_eq!(
        changed_manifest.config().owner_chargers[0]
            .geofence
            .radius_meters,
        250
    );
    assert_ne!(
        original.config().geofence_version,
        changed_manifest.config().geofence_version
    );
    assert!(
        !encode(changed_manifest.config())
            .unwrap()
            .contains("latitude")
    );
    changed["shared_charger"]["geofence"]["radius_meters"] = json!(0);
    assert!(
        crate::registry_bootstrap::RegistryBootstrap::from_json(
            &serde_json::to_vec(&changed).unwrap()
        )
        .is_err()
    );
    for radius in ["-1", "0", "1.5", "4294967296", "\"100\""] {
        let json = format!(
            r#"{{"center":{{"status":"valid","latitude":"0","longitude":"0"}},"radius_meters":{radius}}}"#
        );
        let geofence = serde_json::from_str::<crate::ChargerGeofenceConfig>(&json);
        assert!(geofence.is_err() || geofence.unwrap().to_core().is_err());
    }
}

#[test]
fn rebuild_replays_original_offsets_into_a_separate_versioned_dataset() {
    use rdkafka::config::ClientConfig;
    use rdkafka::message::{Header, OwnedHeaders};
    use rdkafka::mocking::MockCluster;
    use rdkafka::producer::{BaseProducer, BaseRecord, Producer};
    use std::os::unix::fs::DirBuilderExt;
    use std::time::Duration;

    let cluster = MockCluster::new(1).unwrap();
    cluster.create_topic("unit_V", 1, 1).unwrap();
    let producer: BaseProducer = ClientConfig::new()
        .set("bootstrap.servers", cluster.bootstrap_servers())
        .set("log_level", "0")
        .set("message.timeout.ms", "2000")
        .create()
        .unwrap();
    for (index, (time, fields)) in [
        ("2026-10-13T00:00:00Z", json!([
            {"key": "DetailedChargeState", "value": {"detailedChargeStateValue": "DetailedChargeStateDisconnected"}},
            {"key": "Location", "value": {"locationValue": {"latitude": 0, "longitude": 0}}},
            {"key": "Odometer", "value": {"stringValue": "100"}}
        ])),
        ("2026-10-13T00:01:00Z", json!([{"key": "DetailedChargeState", "value": {"detailedChargeStateValue": "DetailedChargeStateCharging"}}])),
        ("2026-10-13T00:02:00Z", json!([{"key": "ACChargingEnergyIn", "value": {"stringValue": "1.0000004"}}])),
        ("2026-10-13T00:03:00Z", json!([{"key": "ACChargingEnergyIn", "value": {"stringValue": "1.5000004"}}])),
        ("2026-10-13T00:04:00Z", json!([{"key": "DetailedChargeState", "value": {"detailedChargeStateValue": "DetailedChargeStateComplete"}}])),
    ].into_iter().enumerate() {
        let payload = serde_json::to_vec(&json!({"createdAt": time, "data": fields})).unwrap();
        let transaction = format!("unit-{index}");
        let headers = OwnedHeaders::new().insert(Header { key: "vin", value: Some("device-a") })
            .insert(Header { key: "txtype", value: Some("V") }).insert(Header { key: "txid", value: Some(&transaction) });
        producer.send(BaseRecord::to("unit_V").partition(0).key("device-a").payload(&payload).headers(headers)).unwrap();
    }
    producer.flush(Duration::from_secs(5)).unwrap();
    let directory = std::path::Path::new("/tmp")
        .join(format!("chargeshare-inline-rebuild-{}", std::process::id()));
    for path in [
        &directory,
        &directory.join("original"),
        &directory.join("replacement"),
    ] {
        std::fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
    let mut original = config_json();
    original["database"] = json!(directory.join("original/ingestion.sqlite3"));
    original["kafka_bootstrap"] = json!(cluster.bootstrap_servers());
    let original_bytes = serde_json::to_vec(&original).unwrap();
    let original_config =
        crate::registry_bootstrap::RegistryBootstrap::from_json(&original_bytes).unwrap();
    let original_source = crate::kafka::live_source(&original_config).unwrap();
    drop(
        Store::open(
            &original_config.database,
            &original_config.legacy_manifest().unwrap(),
            &original_source,
        )
        .unwrap(),
    );
    let mut legacy =
        serde_json::to_value(original_config.legacy_manifest().unwrap().config()).unwrap();
    legacy["normalizer_version"] = json!("chargeshare-live-adapter-v1");
    let connection = rusqlite::Connection::open(&original_config.database).unwrap();
    connection
        .execute(
            "UPDATE runtime_metadata SET manifest_json = ?1",
            [serde_json::to_string(&legacy).unwrap()],
        )
        .unwrap();
    drop(connection);
    let preserved = std::fs::read(&original_config.database).unwrap();
    let mut replacement = original.clone();
    replacement["database"] = json!(directory.join("replacement/ingestion.sqlite3"));
    replacement["source_epoch"] = json!("live-unit-rebuilt-v2");
    replacement["shared_charger"]["geofence"]["radius_meters"] = json!(250);
    let replacement_bytes = serde_json::to_vec(&replacement).unwrap();
    crate::rebuild_live(&original_bytes, &replacement_bytes).unwrap();
    assert_eq!(std::fs::read(&original_config.database).unwrap(), preserved);
    let config =
        crate::registry_bootstrap::RegistryBootstrap::from_json(&replacement_bytes).unwrap();
    let rebuilt_source =
        crate::read_source_marker(&config.kafka_config().source_epoch_path).unwrap();
    assert_ne!(rebuilt_source.epoch, original_source.epoch);
    assert_eq!(rebuilt_source.partitions, original_source.partitions);
    let store = Store::open(
        &config.database,
        &config.legacy_manifest().unwrap(),
        &rebuilt_source,
    )
    .unwrap();
    assert_eq!(store.positions().unwrap()[0].next_offset, 5);
    let results = store
        .results(
            &OwnerId::new("owner-a").unwrap(),
            &VehicleId::new("vehicle-a").unwrap(),
        )
        .unwrap();
    assert_eq!(results.eligible_shared_charger_kwh, "0.500000");
    assert_eq!(results.sessions.len(), 1);
    assert_eq!(results.sessions[0].evidence_state, "partial");
    assert_eq!(
        results.provenance.normalizer_version,
        LIVE_NORMALIZER_VERSION
    );
    assert!(crate::rebuild_live(&original_bytes, &replacement_bytes).is_err());
    drop(store);
    std::fs::remove_dir_all(directory).unwrap();
}
