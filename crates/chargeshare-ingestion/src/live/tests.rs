use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use chargeshare_core::{Energy, Ledger, OwnerId, QualityFlag, VehicleId};
use serde_json::{Value, json};

use super::ChargeState::{self, Charging, Complete, Starting, Stopped};
use crate::registry_bootstrap::RegistryBootstrap;
use crate::{
    CommitBoundary, DeliveryCoordinate, IngestionError, PartitionStart, SourceBinding, Store,
};

static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(0);

struct Runtime {
    directory: PathBuf,
    config: RegistryBootstrap,
    source: SourceBinding,
    records: Vec<Value>,
}

impl Runtime {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "chargeshare-live-test-{}-{}",
            std::process::id(),
            NEXT_RUNTIME.fetch_add(1, Ordering::Relaxed)
        ));
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let mut example: Value =
            serde_json::from_slice(include_bytes!("../../../../dev/live-config.example.json"))
                .unwrap();
        example["database"] = json!(directory.join("ingestion.sqlite3"));
        let config = RegistryBootstrap::from_json(&serde_json::to_vec(&example).unwrap()).unwrap();
        let source = SourceBinding {
            epoch: config.source_epoch.clone(),
            topic: format!("{}_V", config.topic_namespace),
            cluster_id: Some("fictional-cluster".to_owned()),
            topic_id: None,
            partitions: vec![PartitionStart {
                partition: 0,
                initial_offset: 0,
            }],
        };
        let fixture: Value =
            serde_json::from_slice(include_bytes!("../../../../dev/fixtures/live.json")).unwrap();
        Self {
            directory,
            config,
            source,
            records: fixture["records"].as_array().unwrap().clone(),
        }
    }

    fn open(&self) -> Store {
        if self.config.database.exists() {
            return Store::open_runtime(&self.config.settings, &self.source).unwrap();
        }
        let mut store = Store::open(
            &self.config.database,
            &self.config.legacy_manifest().unwrap(),
            &self.source,
        )
        .unwrap();
        let setup = crate::RegistrySetup {
            car_names: self
                .config
                .mappings
                .iter()
                .map(|mapping| {
                    (
                        mapping.vehicle.clone(),
                        if mapping.vehicle == "vehicle-a" {
                            "Car A"
                        } else {
                            "Car B"
                        }
                        .to_owned(),
                    )
                })
                .collect(),
            charger_name: "Charger A".to_owned(),
            charger_owner: "owner-a".to_owned(),
        };
        crate::registry::migrate(&mut store, &self.config, &setup, &mut |_| Ok(())).unwrap();
        store
    }

    fn ingest(
        &self,
        store: &mut Store,
        offset: i64,
        record: &Value,
    ) -> Result<crate::CommitOutcome, IngestionError> {
        let next = store.positions().unwrap()[0].next_offset;
        store.ingest_live(
            &self.config,
            DeliveryCoordinate {
                partition: 0,
                offset,
            },
            next,
            &headers(record),
            &serde_json::to_vec(&record["payload"]).unwrap(),
        )
    }

    fn results(&self, store: &Store) -> crate::ScopedResults {
        store
            .results(
                &OwnerId::new("owner-a").unwrap(),
                &VehicleId::new("vehicle-a").unwrap(),
            )
            .unwrap()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn headers(record: &Value) -> Vec<(String, Vec<u8>)> {
    record["headers"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.as_str().unwrap().as_bytes().to_vec()))
        .collect()
}

fn remove_field(record: &mut Value, key: &str) {
    record["payload"]["data"]
        .as_array_mut()
        .unwrap()
        .retain(|field| field["key"] != key);
}

fn field_mut<'a>(record: &'a mut Value, key: &str) -> &'a mut Value {
    &mut record["payload"]["data"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|field| field["key"] == key)
        .unwrap()["value"]
}

fn window_record(
    runtime: &Runtime,
    id: usize,
    time: &str,
    state: ChargeState,
    counter: Option<&str>,
) -> Value {
    let mut record = runtime.records[1].clone();
    record["headers"]["txid"] = json!(format!("fictional-window-{id}"));
    record["payload"]["createdAt"] = json!(time);
    field_mut(&mut record, "DetailedChargeState")["detailedChargeStateValue"] = json!(state);
    match counter {
        Some(counter) => {
            *field_mut(&mut record, "ACChargingEnergyIn") = json!({"stringValue": counter});
        }
        None => remove_field(&mut record, "ACChargingEnergyIn"),
    }
    record
}

#[test]
fn retained_outer_counters_recover_the_whole_window_without_moving_state_boundaries() {
    let runtime = Runtime::new();
    let records = [
        window_record(&runtime, 0, "2026-09-01T11:59:00Z", Starting, Some("0.02")),
        window_record(&runtime, 1, "2026-09-01T12:00:00Z", Charging, Some("0.20")),
        window_record(&runtime, 2, "2026-09-01T12:05:00Z", Charging, Some("1.10")),
        window_record(&runtime, 3, "2026-09-01T12:10:00Z", Complete, None),
        window_record(&runtime, 4, "2026-09-01T12:10:30Z", Complete, Some("1.42")),
    ];
    let mut store = runtime.open();
    for (offset, record) in records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let result = runtime.results(&store);
    assert_eq!(result.observed_ac_kwh, "1.400000");
    assert_eq!(result.eligible_shared_charger_kwh, "1.400000");
    let session = &result.sessions[0];
    assert_eq!(session.evidence_state, "complete");
    assert!(session.quality_flags.is_empty());
    let coverage = session.coverage.as_ref().unwrap();
    assert_eq!(
        coverage.connection_start_utc_seconds.as_deref(),
        Some("1788264000")
    );
    assert_eq!(
        coverage.connection_end_utc_seconds.as_deref(),
        Some("1788264600")
    );
    assert_eq!(
        coverage.first_counter_utc_seconds.as_deref(),
        Some("1788263940")
    );
    assert_eq!(
        coverage.last_counter_utc_seconds.as_deref(),
        Some("1788264630")
    );
    assert_eq!(coverage.usable_readings, 4);
    assert!(!coverage.partial_start);
    assert!(!coverage.partial_end);
}

#[test]
fn outer_counter_window_is_bounded_to_ninety_source_seconds_on_each_side() {
    for (before, after, expected, partial) in [
        (
            "2026-09-01T11:58:30Z",
            "2026-09-01T12:11:30Z",
            "1.400000",
            false,
        ),
        (
            "2026-09-01T11:58:29.999999999Z",
            "2026-09-01T12:11:30.000000001Z",
            "0.900000",
            true,
        ),
        (
            "2026-09-01T11:58:29Z",
            "2026-09-01T12:11:31Z",
            "0.900000",
            true,
        ),
    ] {
        let runtime = Runtime::new();
        let records = [
            window_record(&runtime, 0, before, Starting, Some("0.02")),
            window_record(&runtime, 1, "2026-09-01T12:00:00Z", Charging, None),
            window_record(&runtime, 2, "2026-09-01T12:01:00Z", Charging, Some("0.20")),
            window_record(&runtime, 3, "2026-09-01T12:05:00Z", Charging, Some("1.10")),
            window_record(&runtime, 4, "2026-09-01T12:10:00Z", Complete, None),
            window_record(&runtime, 5, after, Complete, Some("1.42")),
        ];
        let mut store = runtime.open();
        for (offset, record) in records.iter().enumerate() {
            runtime.ingest(&mut store, offset as i64, record).unwrap();
        }
        let result = runtime.results(&store);
        assert_eq!(result.observed_ac_kwh, expected);
        let coverage = result.sessions[0].coverage.as_ref().unwrap();
        assert_eq!(coverage.partial_start, partial);
        assert_eq!(coverage.partial_end, partial);
    }
}

#[test]
fn outer_counter_recovery_does_not_bridge_retained_continuity_barriers() {
    for barrier in [
        "outside",
        "invalid_location",
        "movement",
        "disconnect",
        "restart",
        "dc",
        "unknown",
        "invalid_counter",
        "rollback",
        "conflict",
    ] {
        let runtime = Runtime::new();
        let mut records = vec![
            window_record(&runtime, 0, "2026-09-01T11:59:00Z", Starting, Some("0.02")),
            window_record(&runtime, 1, "2026-09-01T11:59:30Z", Starting, None),
            window_record(&runtime, 2, "2026-09-01T12:00:00Z", Charging, None),
            window_record(&runtime, 3, "2026-09-01T12:01:00Z", Charging, Some("0.20")),
            window_record(&runtime, 4, "2026-09-01T12:05:00Z", Charging, Some("1.10")),
            window_record(&runtime, 5, "2026-09-01T12:10:00Z", Complete, None),
        ];
        remove_field(&mut records[1], "DetailedChargeState");
        if barrier == "restart" {
            field_mut(&mut records[0], "DetailedChargeState")["detailedChargeStateValue"] =
                json!("DetailedChargeStateStopped");
        }
        match barrier {
            "outside" => {
                field_mut(&mut records[1], "Location")["locationValue"]["latitude"] = json!(0)
            }
            "invalid_location" => *field_mut(&mut records[1], "Location") = json!({}),
            "movement" => field_mut(&mut records[1], "Odometer")["doubleValue"] = json!(1001),
            "disconnect" | "restart" | "unknown" => records[1]["payload"]["data"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "key": "DetailedChargeState",
                    "value": {"detailedChargeStateValue": match barrier {
                        "disconnect" => "DetailedChargeStateDisconnected",
                        "restart" => "DetailedChargeStateStarting",
                        _ => "DetailedChargeStateUnknown",
                    }}
                })),
            "dc" => records[1]["payload"]["data"]
                .as_array_mut()
                .unwrap()
                .push(json!({"key": "FastChargerPresent", "value": {"booleanValue": true}})),
            "invalid_counter" | "rollback" | "conflict" => records[1]["payload"]["data"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "key": "ACChargingEnergyIn", "value": {"stringValue": match barrier {
                        "invalid_counter" => "invalid", "rollback" => "2.0", _ => "0.03"
                    }}
                })),
            _ => unreachable!(),
        }
        if barrier == "conflict" {
            let mut conflict = records[1].clone();
            conflict["headers"]["txid"] = json!("fictional-window-conflict");
            *field_mut(&mut conflict, "ACChargingEnergyIn") = json!({"stringValue": "0.04"});
            records.insert(2, conflict);
        }
        let mut store = runtime.open();
        for (offset, record) in records.iter().enumerate() {
            runtime.ingest(&mut store, offset as i64, record).unwrap();
        }
        let result = runtime.results(&store);
        assert_eq!(result.observed_ac_kwh, "0.900000", "{barrier}");
        let coverage = result.sessions[0].coverage.as_ref().unwrap();
        assert!(coverage.partial_start, "{barrier}");
        assert!(coverage.partial_end, "{barrier}");
    }
}

#[test]
fn missing_location_does_not_erase_retained_location_uncertainty_before_a_baseline() {
    let runtime = Runtime::new();
    let mut records = [
        window_record(&runtime, 0, "2026-09-01T11:58:45Z", Stopped, None),
        window_record(&runtime, 1, "2026-09-01T11:59:00Z", Stopped, Some("0.02")),
        window_record(&runtime, 2, "2026-09-01T12:00:00Z", Charging, None),
        window_record(&runtime, 3, "2026-09-01T12:01:00Z", Charging, Some("0.20")),
        window_record(&runtime, 4, "2026-09-01T12:05:00Z", Charging, Some("1.10")),
        window_record(&runtime, 5, "2026-09-01T12:10:00Z", Complete, None),
    ];
    *field_mut(&mut records[0], "Location") = json!({});
    remove_field(&mut records[1], "Location");
    let mut store = runtime.open();
    for (offset, record) in records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let result = runtime.results(&store);
    assert_eq!(result.observed_ac_kwh, "0.900000");
    assert!(result.sessions[0].coverage.as_ref().unwrap().partial_start);
}

#[test]
fn adjacent_tiny_session_and_shared_stale_counter_preserve_uncertainty_without_reusing_energy() {
    for (baseline, expected_session, expected_total, partial) in [
        ("0.05", "1.360000", "1.410000", true),
        ("0.01", "1.410000", "1.460000", false),
    ] {
        let runtime = Runtime::new();
        let records = [
            window_record(&runtime, 0, "2026-09-01T11:58:00Z", Starting, None),
            window_record(&runtime, 1, "2026-09-01T11:58:10Z", Charging, Some("0")),
            window_record(&runtime, 2, "2026-09-01T11:59:00Z", Stopped, Some("0.05")),
            window_record(&runtime, 3, "2026-09-01T11:59:20Z", Stopped, Some(baseline)),
            window_record(&runtime, 4, "2026-09-01T12:00:00Z", Charging, Some("0.06")),
            window_record(&runtime, 5, "2026-09-01T12:05:00Z", Charging, Some("1.10")),
            window_record(&runtime, 6, "2026-09-01T12:10:00Z", Complete, None),
            window_record(&runtime, 7, "2026-09-01T12:10:30Z", Complete, Some("1.42")),
        ];
        let mut store = runtime.open();
        for (offset, record) in records.iter().enumerate() {
            runtime.ingest(&mut store, offset as i64, record).unwrap();
        }
        let result = runtime.results(&store);
        assert_eq!(result.sessions.len(), 2);
        assert_eq!(result.sessions[0].observed_ac_kwh, "0.050000");
        assert_eq!(result.sessions[1].observed_ac_kwh, expected_session);
        assert_eq!(
            result.sessions[1].coverage.as_ref().unwrap().partial_start,
            partial
        );
        assert!(!result.sessions[1].coverage.as_ref().unwrap().partial_end);
        assert_eq!(result.observed_ac_kwh, expected_total);
    }
}

#[test]
fn nearby_counters_from_another_vehicle_cannot_complete_a_session_window() {
    let mut runtime = Runtime::new();
    let mut mapping = runtime.config.mappings[0].clone();
    mapping.device.push_str("-second");
    mapping.vehicle = "vehicle-b".to_owned();
    mapping.owner = "owner-b".to_owned();
    runtime.config.mappings.push(mapping.clone());
    let mut records = [
        window_record(&runtime, 0, "2026-09-01T11:59:00Z", Starting, None),
        window_record(&runtime, 1, "2026-09-01T11:59:20Z", Stopped, Some("0.02")),
        window_record(&runtime, 2, "2026-09-01T12:00:00Z", Charging, None),
        window_record(&runtime, 3, "2026-09-01T12:01:00Z", Charging, Some("0.20")),
        window_record(&runtime, 4, "2026-09-01T12:05:00Z", Charging, Some("1.10")),
        window_record(&runtime, 5, "2026-09-01T12:10:00Z", Complete, None),
        window_record(&runtime, 6, "2026-09-01T12:10:30Z", Complete, Some("1.42")),
    ];
    for index in [1, 6] {
        records[index]["headers"]["vin"] = json!(mapping.device);
        records[index]["payload"]["vin"] = json!(mapping.device);
    }
    let mut store = runtime.open();
    for (offset, record) in records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let result = runtime.results(&store);
    assert_eq!(result.observed_ac_kwh, "0.900000");
    let coverage = result.sessions[0].coverage.as_ref().unwrap();
    assert!(coverage.partial_start);
    assert!(coverage.partial_end);
    assert!(
        store
            .sessions(
                &OwnerId::new("owner-b").unwrap(),
                &VehicleId::new("vehicle-b").unwrap()
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn sqlite_only_history_replays_idempotently_and_keeps_delivery_verification() {
    let runtime = Runtime::new();
    let records = [
        window_record(&runtime, 0, "2026-09-01T11:59:00Z", Starting, Some("0.02")),
        window_record(&runtime, 1, "2026-09-01T12:00:00Z", Charging, Some("0.20")),
        window_record(&runtime, 2, "2026-09-01T12:05:00Z", Charging, Some("1.10")),
        window_record(&runtime, 3, "2026-09-01T12:10:00Z", Complete, None),
        window_record(&runtime, 4, "2026-09-01T12:10:30Z", Complete, Some("1.42")),
    ];
    let owner = OwnerId::new("owner-a").unwrap();
    let vehicle = VehicleId::new("vehicle-a").unwrap();
    let mut store = runtime.open();
    for (offset, record) in records[..4].iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let partial = runtime.results(&store);
    assert!(partial.sessions[0].coverage.as_ref().unwrap().partial_end);
    runtime.ingest(&mut store, 4, &records[4]).unwrap();
    let ongoing = runtime.results(&store);
    assert_eq!(ongoing.observed_ac_kwh, "1.400000");
    assert_ne!(ongoing.revision, partial.revision);
    let positions = store.positions().unwrap();
    drop(store);
    let unchanged_sqlite_fixture = fs::read(&runtime.config.database).unwrap();
    let mut store = runtime.open();
    let replay = store.replay(&owner, &vehicle).unwrap();
    assert_eq!(
        store.sessions(&owner, &vehicle).unwrap(),
        replay.sessions(&vehicle).unwrap()
    );
    for _ in 0..3 {
        assert_eq!(runtime.results(&store), ongoing);
    }
    assert_eq!(store.positions().unwrap(), positions);
    drop(store);
    assert_eq!(
        fs::read(&runtime.config.database).unwrap(),
        unchanged_sqlite_fixture
    );
    store = runtime.open();
    for (offset, record) in records.iter().enumerate() {
        assert!(
            runtime
                .ingest(&mut store, offset as i64, record)
                .unwrap()
                .duplicate_delivery
        );
    }
    let mut resend = records[0].clone();
    resend["headers"]["txid"] = json!("fictional-window-resend");
    resend["payload"]["isResend"] = json!(true);
    runtime.ingest(&mut store, 5, &resend).unwrap();
    assert_eq!(runtime.results(&store), ongoing);
    *field_mut(&mut resend, "ACChargingEnergyIn") = json!({"stringValue": "0.03"});
    assert_eq!(
        runtime.ingest(&mut store, 5, &resend),
        Err(IngestionError::DeliveryConflict)
    );
    assert_eq!(runtime.results(&store), ongoing);
}

#[test]
fn whole_window_uses_all_monotonic_outer_samples_instead_of_nearest_samples() {
    let runtime = Runtime::new();
    let mut records = [
        window_record(&runtime, 0, "2026-09-01T11:58:30Z", Starting, Some("0.01")),
        window_record(&runtime, 1, "2026-09-01T11:59:00Z", Starting, Some("0.02")),
        window_record(&runtime, 2, "2026-09-01T12:00:00Z", Charging, Some("0.20")),
        window_record(&runtime, 3, "2026-09-01T12:05:00Z", Charging, Some("1.10")),
        window_record(&runtime, 4, "2026-09-01T12:10:00Z", Complete, None),
        window_record(&runtime, 5, "2026-09-01T12:10:30Z", Complete, Some("1.42")),
        window_record(&runtime, 6, "2026-09-01T12:11:30Z", Complete, Some("1.61")),
    ];
    for index in [1, 5, 6] {
        remove_field(&mut records[index], "Location");
        remove_field(&mut records[index], "Odometer");
    }
    let mut store = runtime.open();
    for (offset, record) in records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let result = runtime.results(&store);
    assert_eq!(result.observed_ac_kwh, "1.600000");
    let coverage = result.sessions[0].coverage.as_ref().unwrap();
    assert_eq!(
        coverage.first_counter_utc_seconds.as_deref(),
        Some("1788263910")
    );
    assert_eq!(
        coverage.last_counter_utc_seconds.as_deref(),
        Some("1788264690")
    );
    assert_eq!(coverage.usable_readings, 6);
    assert!(!coverage.partial_start);
    assert!(!coverage.partial_end);
}

#[test]
fn recovered_counter_intervals_use_source_timestamps_across_time_of_use_boundaries() {
    let runtime = Runtime::new();
    let records = [
        window_record(&runtime, 0, "2026-09-01T21:59:30Z", Starting, Some("0.01")),
        window_record(&runtime, 1, "2026-09-01T22:00:00Z", Charging, None),
        window_record(&runtime, 2, "2026-09-01T22:00:30Z", Charging, Some("0.61")),
        window_record(&runtime, 3, "2026-09-01T22:01:00Z", Charging, Some("0.91")),
        window_record(&runtime, 4, "2026-09-01T22:01:10Z", Complete, None),
        window_record(&runtime, 5, "2026-09-01T22:01:30Z", Complete, Some("1.21")),
    ];
    let mut store = runtime.open();
    for (offset, record) in records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let result = runtime.results(&store);
    assert_eq!(result.pricing.priced_ac_kwh, "1.200000");
    let quote = result.sessions[0].pricing.quote.as_ref().unwrap();
    assert_eq!(quote.slices.len(), 4);
    assert_eq!(quote.slices[0].period, "off_peak");
    assert_eq!(quote.slices[0].allocated_ac_kwh, "0.300000");
    assert_eq!(quote.slices[0].start_utc_seconds, "1788299970");
    assert_eq!(quote.slices[0].end_utc_seconds, "1788300000");
    assert_eq!(quote.slices[1].period, "part_peak");
    assert_eq!(quote.slices[1].allocated_ac_kwh, "0.300000");
    assert_eq!(quote.slices[2].allocated_ac_kwh, "0.300000");
    assert_eq!(quote.slices[3].allocated_ac_kwh, "0.300000");
    assert_eq!(quote.exact_cost, "0.4525140000000000 USD");
    let month = result
        .monthly
        .months
        .iter()
        .find(|month| month.month == "2026-09")
        .unwrap();
    assert_eq!(month.total.as_ref().unwrap().ac_kwh, "1.200000");
    assert_eq!(month.days[0].total.as_ref().unwrap().ac_kwh, "1.200000");
}

#[test]
fn disconnected_charging_complete_and_stopped_each_yield_one_exact_ac_session() {
    for terminal in ["DetailedChargeStateComplete", "DetailedChargeStateStopped"] {
        let mut runtime = Runtime::new();
        field_mut(&mut runtime.records[3], "DetailedChargeState")["detailedChargeStateValue"] =
            json!(terminal);
        let mut store = runtime.open();
        for (offset, record) in runtime.records.iter().enumerate() {
            runtime.ingest(&mut store, offset as i64, record).unwrap();
        }
        let result = runtime.results(&store);
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.observed_ac_kwh, "0.500000");
        assert_eq!(result.eligible_shared_charger_kwh, "0.500000");
        assert_eq!(result.sessions[0].evidence_state, "complete");
        assert_eq!(result.sessions[0].classification, "SharedCharger");
        assert_eq!(result.provenance.source, "live-receiver-kafka");
        assert_eq!(
            result.evidence_boundary,
            "Vehicle-reported AC input counter energy; physical accuracy unvalidated"
        );
        assert_eq!(result.pricing.display_subtotal.as_deref(), Some("$0.11"));
        drop(store);
    }
}

#[test]
fn utc_timestamps_accept_optional_fractional_seconds_and_truncate_event_time() {
    assert_eq!(
        super::parse_utc_timestamp("1970-01-01T00:00:00Z"),
        Ok((0, 0))
    );
    for (fraction, expected_nanoseconds) in [
        ("1", 100_000_000),
        ("12", 120_000_000),
        ("123", 123_000_000),
        ("1234", 123_400_000),
        ("12345", 123_450_000),
        ("123456", 123_456_000),
        ("1234567", 123_456_700),
        ("12345678", 123_456_780),
        ("123456789", 123_456_789),
    ] {
        for suffix in ["Z", "+00:00"] {
            assert_eq!(
                super::parse_utc_timestamp(&format!("1970-01-01T00:00:00.{fraction}{suffix}")),
                Ok((0, expected_nanoseconds))
            );
        }
    }
    assert_eq!(
        super::parse_utc_timestamp("1969-12-31T23:59:59.999999999Z"),
        Ok((-1, 999_999_999))
    );
    for invalid in [
        "1970-01-01T00:00:00.Z",
        "1970-01-01T00:00:00.1234567890Z",
        "1970-01-01T00:00:00.123aZ",
        "1970-01-01T00:00:00.１２３Z",
        "1970-01-01T00:00:00.123.456Z",
        "1970-01-01T00:00:00.123",
        "1970-01-01T00:00:00.123+01:00",
        "1970-02-30T00:00:00.123Z",
        "1970-01-01T24:00:00.123Z",
    ] {
        assert_eq!(
            super::parse_utc_timestamp(invalid),
            Err(crate::RejectionReason::InvalidTimestamp)
        );
    }
}

#[test]
fn nanosecond_soc_only_resend_keeps_battery_without_a_charging_association() {
    let runtime = Runtime::new();
    let mut record = runtime.records[0].clone();
    record["payload"] = json!({
        "data": [{"key": "Soc", "value": {"doubleValue": 99.9}}],
        "createdAt": "2026-10-13T12:38:54.123456789Z",
        "vin": "FICTIONAL-VIN-A",
        "isResend": true
    });
    let (signal, _, resend) = super::decode_bootstrap(
        &runtime.config,
        &headers(&record),
        &serde_json::to_vec(&record["payload"]).unwrap(),
    )
    .unwrap();
    assert_eq!(signal.time, 1_791_895_134);
    assert!(resend);
    assert_eq!(
        super::adapt(
            &runtime.config.legacy_manifest().unwrap(),
            &mut super::AdapterState::default(),
            &signal,
            resend
        ),
        Ok(crate::RecordDisposition::Rejected(
            crate::RejectionReason::MissingAssociation
        ))
    );
    let mut store = runtime.open();
    runtime.ingest(&mut store, 0, &record).unwrap();
    let result = runtime.results(&store);
    assert!(result.sessions.is_empty());
    assert_eq!(
        result.battery_reading.unwrap().battery_percent,
        crate::BatteryPercent::try_from(99.9).unwrap()
    );
}

#[test]
fn fractional_record_ordering_survives_checkpoint_recovery() {
    let mut runtime = Runtime::new();
    runtime.records[1]["payload"]["createdAt"] = json!("2026-09-01T12:00:00.900000001Z");
    runtime.records[2]["payload"]["createdAt"] = json!("2026-09-01T12:00:00.9Z");
    field_mut(&mut runtime.records[2], "DetailedChargeState")["detailedChargeStateValue"] =
        json!("DetailedChargeStateComplete");
    runtime.records[3]["payload"]["createdAt"] = json!("2026-09-01T12:00:00.900000002Z");
    let manifest = runtime.config.legacy_manifest().unwrap();
    let mut state: super::AdapterState = serde_json::from_str(
        r#"{"previous":null,"connection":null,"next_position":0,"last_time":null,"ac_evidence":false}"#,
    )
    .unwrap();
    for (index, record) in runtime.records.iter().enumerate() {
        state = serde_json::from_str(&super::encode(&state).unwrap()).unwrap();
        let (signal, _, resend) = super::decode_bootstrap(
            &runtime.config,
            &headers(record),
            &serde_json::to_vec(&record["payload"]).unwrap(),
        )
        .unwrap();
        let disposition = super::adapt(&manifest, &mut state, &signal, resend).unwrap();
        if index == 0 {
            assert_eq!(
                disposition,
                crate::RecordDisposition::Rejected(crate::RejectionReason::MissingAssociation)
            );
            continue;
        }
        let crate::RecordDisposition::Accepted(evidence) = disposition else {
            panic!("expected charging evidence");
        };
        assert_eq!(evidence.event.position, (index - 1).to_string());
        assert_eq!(
            evidence.event.kind,
            match index {
                1 => crate::NormalizedEventKind::Start,
                2 => crate::NormalizedEventKind::Sample,
                _ => crate::NormalizedEventKind::End,
            }
        );
        assert_eq!(
            evidence.event.charge_type,
            if index == 2 {
                crate::NormalizedChargeType::Ambiguous
            } else {
                crate::NormalizedChargeType::Ac
            }
        );
        manifest.validate_evidence(&evidence).unwrap();
    }
}

#[test]
fn unknown_telemetry_fields_are_ignored_without_rejecting_or_retaining_them() {
    let mut runtime = Runtime::new();
    for record in &mut runtime.records {
        record["payload"]["data"].as_array_mut().unwrap().extend([
            json!({"key": "FutureTelemetryField", "value": {"privateValue": "fictional-discarded-text"}}),
            json!({"key": "FutureTelemetryField", "value": {"anotherValue": 42}}),
        ]);
    }
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let result = runtime.results(&store);
    assert_eq!(result.sessions.len(), 1);
    assert_eq!(result.observed_ac_kwh, "0.500000");
    assert_eq!(result.eligible_shared_charger_kwh, "0.500000");
    assert_eq!(store.positions().unwrap()[0].next_offset, 4);
    let response = serde_json::to_string(&result).unwrap();
    for discarded in ["FutureTelemetryField", "fictional-discarded-text"] {
        assert!(!response.contains(discarded));
        for entry in fs::read_dir(&runtime.directory).unwrap() {
            let bytes = fs::read(entry.unwrap().path()).unwrap();
            assert!(
                !bytes
                    .windows(discarded.len())
                    .any(|window| window == discarded.as_bytes())
            );
        }
    }
    drop(store);
}

#[test]
fn unknown_field_values_and_repeated_unknown_keys_do_not_change_decoded_signals() {
    let runtime = Runtime::new();
    let original = &runtime.records[1];
    let mut extended = original.clone();
    extended["payload"]["data"].as_array_mut().unwrap().extend([
        json!({"key": "FutureTelemetryField", "value": {"privateValue": "fictional-discarded-text"}}),
        json!({"key": "FutureTelemetryField", "value": [null, 42]}),
    ]);
    let decode_record = |record: &Value| {
        super::decode_bootstrap(
            &runtime.config,
            &headers(record),
            &serde_json::to_vec(&record["payload"]).unwrap(),
        )
        .unwrap()
    };
    let (expected, expected_id, expected_resend) = decode_record(original);
    let (observed, observed_id, observed_resend) = decode_record(&extended);
    assert!(observed == expected);
    assert_eq!(observed_id, expected_id);
    assert_eq!(observed_resend, expected_resend);
    let retained = serde_json::to_string(&observed).unwrap();
    assert!(!retained.contains("FutureTelemetryField"));
    assert!(!retained.contains("fictional-discarded-text"));
    for discarded in [
        "latitude",
        "longitude",
        "12.3456789",
        "45.6789123",
        "FICTIONAL-VIN-A",
    ] {
        assert!(!retained.contains(discarded));
    }
}

#[test]
fn live_state_transitions_replay_exact_counter_energy_and_conservative_boundary_holds() {
    for variant in [
        "complete",
        "missing_end",
        "missing_final",
        "missing_start",
        "invalid_power",
        "missing_power",
        "power_only",
    ] {
        let mut runtime = Runtime::new();
        if variant == "missing_final" {
            remove_field(&mut runtime.records[3], "ACChargingEnergyIn");
        }
        for record in &mut runtime.records {
            match variant {
                "invalid_power" => *field_mut(record, "ACChargingPower") = json!({"invalid": true}),
                "missing_power" => remove_field(record, "ACChargingPower"),
                "power_only" => remove_field(record, "ACChargingEnergyIn"),
                _ => {}
            }
        }
        let manifest = runtime.config.legacy_manifest().unwrap();
        let vehicle = VehicleId::new("vehicle-a").unwrap();
        let mut ledger = Ledger::default();
        ledger
            .register(vehicle.clone(), OwnerId::new("owner-a").unwrap())
            .unwrap();
        ledger.use_observed_counter_window(&vehicle).unwrap();
        let mut state = super::AdapterState::default();
        for record in runtime
            .records
            .iter()
            .take(if variant == "missing_end" { 3 } else { 4 })
            .skip(usize::from(variant == "missing_start"))
        {
            let (signal, _, resend) = super::decode_bootstrap(
                &runtime.config,
                &headers(record),
                &serde_json::to_vec(&record["payload"]).unwrap(),
            )
            .unwrap();
            if let crate::RecordDisposition::Accepted(evidence) =
                super::adapt(&manifest, &mut state, &signal, resend).unwrap()
            {
                ledger.ingest(evidence.event.to_core().unwrap()).unwrap();
            }
        }
        let sessions = ledger.sessions(&vehicle).unwrap();
        assert_eq!(sessions.len(), 1);
        let expected = match variant {
            "missing_end" | "missing_final" => "0.25",
            "power_only" => "0",
            _ => "0.5",
        };
        assert_eq!(
            sessions[0].observed_ac,
            Energy::parse_kwh(expected).unwrap()
        );
        assert_eq!(
            sessions[0].is_eligible(),
            matches!(
                variant,
                "complete" | "invalid_power" | "missing_power" | "missing_final"
            )
        );
        if variant == "missing_end" {
            assert!(
                sessions[0]
                    .quality_flags
                    .contains(&QualityFlag::MissingConnectionEnd)
            );
        }
        if variant == "missing_final" {
            assert!(
                sessions[0]
                    .quality_flags
                    .contains(&QualityFlag::MissingTerminalSample)
            );
        }
        if variant == "missing_start" {
            assert!(
                sessions[0]
                    .quality_flags
                    .contains(&QualityFlag::MissingConnectionStart)
            );
        }
    }
}

#[test]
fn valid_counter_or_positive_ac_power_establishes_ac_evidence() {
    for power in [
        json!({"invalid": true}),
        json!({"doubleValue": -1}),
        json!({"doubleValue": 7.2}),
    ] {
        let mut runtime = Runtime::new();
        for record in &mut runtime.records {
            *field_mut(record, "ACChargingPower") = power.clone();
        }
        let mut store = runtime.open();
        for (offset, record) in runtime.records.iter().enumerate() {
            runtime.ingest(&mut store, offset as i64, record).unwrap();
        }
        assert_eq!(
            runtime.results(&store).eligible_shared_charger_kwh,
            "0.500000"
        );
        drop(store);
    }
}

#[test]
fn missing_end_holds_and_missing_terminal_counter_prices_only_the_observed_window() {
    for omit_end in [true, false] {
        let mut runtime = Runtime::new();
        if !omit_end {
            remove_field(&mut runtime.records[3], "ACChargingEnergyIn");
        }
        let mut store = runtime.open();
        for (offset, record) in runtime
            .records
            .iter()
            .take(if omit_end { 3 } else { 4 })
            .enumerate()
        {
            runtime.ingest(&mut store, offset as i64, record).unwrap();
        }
        let result = runtime.results(&store);
        assert_eq!(result.state, if omit_end { "held" } else { "eligible" });
        assert_eq!(result.observed_ac_kwh, "0.250000");
        assert_eq!(
            result.eligible_shared_charger_kwh,
            if omit_end { "0.000000" } else { "0.250000" }
        );
        if !omit_end {
            assert_eq!(result.sessions[0].evidence_state, "partial");
            let quote = result.sessions[0].pricing.quote.as_ref().unwrap();
            assert_eq!(quote.display_cost, "$0.06");
            assert_eq!(quote.slices.last().unwrap().end_utc_seconds, "1788264300");
        }
        assert!(
            result.sessions[0].quality_flags.contains(
                &if omit_end {
                    "MissingConnectionEnd"
                } else {
                    "MissingTerminalSample"
                }
                .to_owned()
            )
        );
        drop(store);
    }
}

#[test]
fn starting_mid_charge_does_not_invent_a_start_or_baseline() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().skip(1).enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let session = &runtime.results(&store).sessions[0];
    assert!(
        session
            .quality_flags
            .contains(&"MissingConnectionStart".to_owned())
    );
    assert_eq!(session.eligible_shared_charger_kwh, "0.000000");
    drop(store);
}

#[test]
fn sparse_counter_records_use_explicit_state_and_never_close_on_zero_power() {
    let mut runtime = Runtime::new();
    remove_field(&mut runtime.records[2], "DetailedChargeState");
    field_mut(&mut runtime.records[2], "ACChargingPower")["doubleValue"] = json!(0);
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().take(3).enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    assert!(
        runtime.results(&store).sessions[0]
            .quality_flags
            .contains(&"MissingConnectionEnd".to_owned())
    );
    runtime.ingest(&mut store, 3, &runtime.records[3]).unwrap();
    assert_eq!(
        runtime.results(&store).eligible_shared_charger_kwh,
        "0.500000"
    );
    drop(store);
}

#[test]
fn redelivery_resends_and_restart_reuse_the_original_session_and_positions() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().take(3).enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    assert!(
        runtime
            .ingest(&mut store, 1, &runtime.records[1])
            .unwrap()
            .duplicate_delivery
    );
    drop(store);
    let mut store = runtime.open();
    let mut resend = runtime.records[1].clone();
    resend["payload"]["isResend"] = json!(true);
    assert!(
        !runtime
            .ingest(&mut store, 3, &resend)
            .unwrap()
            .inserted_variant
    );
    resend["headers"]["txid"] = json!("fictional-new-transaction");
    assert!(
        !runtime
            .ingest(&mut store, 4, &resend)
            .unwrap()
            .inserted_variant
    );
    runtime.ingest(&mut store, 5, &runtime.records[3]).unwrap();
    let complete = runtime.results(&store);
    for (index, record) in runtime.records.iter().enumerate() {
        assert!(
            !runtime
                .ingest(&mut store, 6 + index as i64, record)
                .unwrap()
                .inserted_variant
        );
    }
    assert_eq!(runtime.results(&store), complete);
    drop(store);
    let store = runtime.open();
    assert_eq!(runtime.results(&store), complete);
    assert_eq!(store.positions().unwrap()[0].next_offset, 10);
    drop(store);
}

#[test]
fn failed_commit_rolls_back_adapter_checkpoint_evidence_and_progress_together() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    runtime.ingest(&mut store, 0, &runtime.records[0]).unwrap();
    let record = &runtime.records[1];
    let result = store.ingest_live_observed(
        &runtime.config,
        DeliveryCoordinate {
            partition: 0,
            offset: 1,
        },
        1,
        &headers(record),
        &serde_json::to_vec(&record["payload"]).unwrap(),
        &mut |boundary| {
            if boundary == CommitBoundary::BeforeCommit {
                Err(IngestionError::StorageUnavailable)
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(result, Err(IngestionError::StorageUnavailable));
    assert_eq!(store.positions().unwrap()[0].next_offset, 1);
    assert!(runtime.results(&store).sessions.is_empty());
    drop(store);
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().enumerate().skip(1) {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    assert_eq!(
        runtime.results(&store).eligible_shared_charger_kwh,
        "0.500000"
    );
    drop(store);
}

#[test]
fn private_registry_is_separate_from_coordinate_free_telemetry_and_http_results() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let result = serde_json::to_string(&runtime.results(&store)).unwrap();
    let forbidden = [
        "latitude",
        "longitude",
        "12.3456789",
        "45.6789123",
        "FICTIONAL-VIN-A",
    ];
    for private in forbidden {
        assert!(!result.contains(private));
    }
    let device: String = store
        .connection
        .query_row("SELECT device FROM cars", [], |row| row.get(0))
        .unwrap();
    assert_eq!(device, "FICTIONAL-VIN-A");
    for table_column in [
        ("event_variants", "event_json"),
        ("deliveries", "provenance_json"),
        ("live_records", "signal_json"),
        ("live_records", "disposition_json"),
        ("live_adapter_state", "state_json"),
    ] {
        let query = format!(
            "SELECT {} FROM {} WHERE {} IS NOT NULL",
            table_column.1, table_column.0, table_column.1
        );
        let values = store
            .connection
            .prepare(&query)
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for value in values {
            for private in forbidden {
                assert!(!value.contains(private));
            }
        }
    }
    drop(store);
}

#[test]
fn missing_invalid_partial_and_mixed_location_keep_existing_classification_holds() {
    for variant in ["missing", "invalid", "partial", "mixed", "outside"] {
        let mut runtime = Runtime::new();
        for (index, record) in runtime.records.iter_mut().enumerate() {
            match variant {
                "missing" => remove_field(record, "Location"),
                "invalid" => *field_mut(record, "Location") = json!({"invalid": true}),
                "partial" if index == 2 => remove_field(record, "Location"),
                "mixed" if index == 2 => {
                    field_mut(record, "Location")["locationValue"]["latitude"] = json!(0)
                }
                "outside" => field_mut(record, "Location")["locationValue"]["latitude"] = json!(0),
                _ => {}
            }
        }
        let mut store = runtime.open();
        for (offset, record) in runtime.records.iter().enumerate() {
            runtime.ingest(&mut store, offset as i64, record).unwrap();
        }
        let result = runtime.results(&store);
        assert_eq!(result.observed_ac_kwh, "0.500000");
        assert_eq!(
            result.eligible_shared_charger_kwh,
            if variant == "partial" {
                "0.500000"
            } else {
                "0.000000"
            }
        );
        assert_eq!(
            result.sessions[0].classification,
            if variant == "outside" {
                "OtherCharger"
            } else if variant == "partial" {
                "SharedCharger"
            } else {
                "Held"
            }
        );
        drop(store);
    }
}

#[test]
fn rollback_unknown_state_and_late_records_cannot_make_energy_eligible() {
    for variant in ["rollback", "unknown", "late"] {
        let mut runtime = Runtime::new();
        match variant {
            "rollback" => {
                *field_mut(&mut runtime.records[2], "ACChargingEnergyIn") =
                    json!({"stringValue": "99"})
            }
            "unknown" => {
                *field_mut(&mut runtime.records[2], "DetailedChargeState") =
                    json!({"invalid": true})
            }
            "late" => runtime.records[2]["payload"]["createdAt"] = json!("2026-09-01T11:59:30Z"),
            _ => unreachable!(),
        }
        let mut store = runtime.open();
        for (offset, record) in runtime.records.iter().enumerate() {
            runtime.ingest(&mut store, offset as i64, record).unwrap();
        }
        assert_eq!(runtime.results(&store).state, "held");
        assert_eq!(
            runtime.results(&store).eligible_shared_charger_kwh,
            "0.000000"
        );
        drop(store);
    }
}

#[test]
fn reused_transaction_with_changed_signals_refuses_without_mutating_progress() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    runtime.ingest(&mut store, 0, &runtime.records[0]).unwrap();
    runtime.ingest(&mut store, 1, &runtime.records[1]).unwrap();
    let mut conflict = runtime.records[1].clone();
    *field_mut(&mut conflict, "ACChargingEnergyIn") = json!({"stringValue": "999"});
    assert_eq!(
        runtime.ingest(&mut store, 2, &conflict),
        Err(IngestionError::DeliveryConflict)
    );
    assert_eq!(store.positions().unwrap()[0].next_offset, 2);
    let sessions = store
        .sessions(
            &OwnerId::new("owner-a").unwrap(),
            &VehicleId::new("vehicle-a").unwrap(),
        )
        .unwrap();
    assert_eq!(sessions[0].observed_ac, Energy::ZERO);
    assert!(
        sessions[0]
            .quality_flags
            .contains(&QualityFlag::MissingConnectionEnd)
    );
    drop(store);
}

fn battery_record(runtime: &Runtime, time: &str, percent: Value, state: Option<&str>) -> Value {
    let mut record = runtime.records[0].clone();
    record["headers"]["txid"] = json!(format!("fictional-{time}-{percent}-{state:?}"));
    record["payload"]["createdAt"] = json!(time);
    record["payload"]["data"] = json!([{"key":"Soc","value":percent}]);
    if let Some(state) = state {
        record["payload"]["data"]
            .as_array_mut()
            .unwrap()
            .push(json!({"key":"DetailedChargeState","value":{"detailedChargeStateValue":state}}));
    }
    record
}

#[test]
fn current_battery_orders_fractional_source_times_outside_sessions_and_survives_replay_restart() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    assert!(runtime.results(&store).battery_reading.is_none());
    assert!(runtime.results(&store).charging.is_none());
    let idle = battery_record(
        &runtime,
        "2026-09-01T10:00:00Z",
        json!({"doubleValue":100}),
        None,
    );
    runtime.ingest(&mut store, 0, &idle).unwrap();
    assert!(runtime.results(&store).sessions.is_empty());
    assert_eq!(
        f64::from(
            runtime
                .results(&store)
                .battery_reading
                .unwrap()
                .battery_percent
        ),
        100.0
    );
    let observations = [
        (
            "2026-09-01T11:00:00.9Z",
            100.0,
            Some("DetailedChargeStateCharging"),
        ),
        ("2026-09-01T11:00:00.900000001Z", 62.5, None),
        (
            "2026-09-01T11:00:00.89Z",
            99.0,
            Some("DetailedChargeStateComplete"),
        ),
    ];
    for (offset, (time, percent, state)) in observations.into_iter().enumerate() {
        let record = battery_record(&runtime, time, json!({"doubleValue":percent}), state);
        runtime
            .ingest(&mut store, offset as i64 + 1, &record)
            .unwrap();
    }
    let result = runtime.results(&store);
    let battery = result.battery_reading.as_ref().unwrap();
    assert_eq!(f64::from(battery.battery_percent), 62.5);
    assert_eq!(battery.observed_at, "2026-09-01T11:00:00.900000001Z");
    let charging = result.charging.as_ref().unwrap();
    assert!(charging.active);
    assert_eq!(charging.observed_at, "2026-09-01T11:00:00.9Z");
    let mut duplicate = battery_record(
        &runtime,
        observations[1].0,
        json!({"doubleValue":62.5}),
        None,
    );
    runtime.ingest(&mut store, 2, &duplicate).unwrap();
    duplicate["headers"]["txid"] = json!("fictional-battery-resend");
    duplicate["payload"]["isResend"] = json!(true);
    runtime.ingest(&mut store, 4, &duplicate).unwrap();
    assert_eq!(runtime.results(&store), result);
    assert_eq!(
        store.active_scopes().unwrap()[0]["battery_reading"],
        serde_json::to_value(battery).unwrap()
    );
    let filtered = store
        .results_for_charger(
            &OwnerId::new("owner-a").unwrap(),
            &VehicleId::new("vehicle-a").unwrap(),
            "fictional-shared-charger",
        )
        .unwrap();
    assert_eq!(filtered.battery_reading, result.battery_reading);
    assert_eq!(filtered.charging, result.charging);
    drop(store);
    assert_eq!(runtime.results(&runtime.open()), result);
}

#[test]
fn missing_invalid_duplicate_soc_never_erases_or_refreshes_a_known_reading() {
    for value in [
        None,
        Some(json!({"doubleValue":-1})),
        Some(json!({"doubleValue":101})),
        Some(json!({"doubleValue":"NaN"})),
        Some(json!({"stringValue":"82"})),
        Some(json!({"doubleValue":null})),
        Some(json!({"doubleValue":82,"extra":true})),
        Some(json!({"duplicate":true})),
    ] {
        let runtime = Runtime::new();
        let mut store = runtime.open();
        let initial = battery_record(
            &runtime,
            "2026-09-01T11:00:00Z",
            json!({"doubleValue":0}),
            None,
        );
        runtime.ingest(&mut store, 0, &initial).unwrap();
        let before = runtime.results(&store).battery_reading;
        let mut invalid = battery_record(
            &runtime,
            "2026-09-10T11:00:00Z",
            json!({"doubleValue":99}),
            Some("DetailedChargeStateCharging"),
        );
        match value {
            None => remove_field(&mut invalid, "Soc"),
            Some(value) if value.get("duplicate").is_some() => invalid["payload"]["data"]
                .as_array_mut()
                .unwrap()
                .push(json!({"key":"Soc","value":{"doubleValue":20}})),
            Some(value) => *field_mut(&mut invalid, "Soc") = value,
        }
        runtime.ingest(&mut store, 1, &invalid).unwrap();
        let result = runtime.results(&store);
        assert_eq!(result.battery_reading, before);
        assert_eq!(
            f64::from(result.battery_reading.unwrap().battery_percent),
            0.0
        );
        assert_eq!(result.charging.unwrap().observed_at, "2026-09-10T11:00:00Z");
        assert!(
            result
                .sessions
                .iter()
                .all(|session| session.observed_ac_kwh == "0.000000")
        );
    }
}

#[test]
fn charging_and_idle_battery_updates_preserve_session_energy_rates_and_independent_state() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
        assert!(runtime.results(&store).battery_reading.is_some());
    }
    let before = runtime.results(&store);
    for (offset, percent) in [100.0, 0.0].into_iter().enumerate() {
        let record = battery_record(
            &runtime,
            &format!("2026-10-0{}T11:00:00Z", offset + 1),
            json!({"doubleValue":percent}),
            None,
        );
        runtime
            .ingest(&mut store, offset as i64 + 4, &record)
            .unwrap();
        let result = runtime.results(&store);
        assert_eq!(
            f64::from(result.battery_reading.unwrap().battery_percent),
            percent
        );
        assert_eq!(result.charging, before.charging);
        assert_eq!(result.sessions, before.sessions);
        assert_eq!(result.pricing, before.pricing);
    }
    assert_eq!(before.observed_ac_kwh, "0.500000");
    assert_eq!(before.pricing.display_subtotal.as_deref(), Some("$0.11"));
}

#[test]
fn legacy_records_keep_unknown_soc_on_restart_original_redelivery_and_resend() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    store.connection.execute_batch("UPDATE event_variants SET event_json = json_remove(event_json, '$.battery_percent');
        UPDATE live_records SET signal_json = json_remove(signal_json, '$.battery_percent'), disposition_json = json_remove(disposition_json, '$.Accepted.event.battery_percent');").unwrap();
    let before = runtime.results(&store);
    assert_eq!(before.battery_reading, None);
    drop(store);
    let mut store = runtime.open();
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let mut resend = runtime.records[3].clone();
    resend["headers"]["txid"] = json!("fictional-legacy-terminal-resend");
    resend["payload"]["isResend"] = json!(true);
    runtime.ingest(&mut store, 4, &resend).unwrap();
    assert_eq!(runtime.results(&store), before);
}

#[test]
fn battery_backfill_resumes_without_readapting_sessions_and_preserves_redelivery() {
    let runtime = Runtime::new();
    let mut records = runtime.records.clone();
    let fixture: Value =
        serde_json::from_slice(include_bytes!("../../../../dev/fixtures/live.json")).unwrap();
    records.push(fixture["battery_backfill_example"].clone());
    let mut store = runtime.open();
    for (offset, record) in records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let expected = runtime.results(&store);
    store.connection.execute_batch("UPDATE event_variants SET event_json = json_remove(event_json, '$.battery_percent');
        UPDATE live_records SET signal_json = json_remove(signal_json, '$.battery_percent'), disposition_json = json_remove(disposition_json, '$.Accepted.event.battery_percent');").unwrap();
    let before = runtime.results(&store);
    let state: String = store
        .connection
        .query_row("SELECT state_json FROM live_adapter_state", [], |row| {
            row.get(0)
        })
        .unwrap();
    let statistics = store.statistics().unwrap();
    let positions = store.positions().unwrap();
    assert!(store.needs_battery_backfill().unwrap());
    for record in &records[..2] {
        store
            .backfill_live_battery(
                &headers(record),
                &serde_json::to_vec(&record["payload"]).unwrap(),
            )
            .unwrap();
    }
    drop(store);
    let mut store = runtime.open();
    assert!(store.needs_battery_backfill().unwrap());
    let mut conflict = records[2].clone();
    conflict["payload"]["createdAt"] = json!("2026-09-20T12:00:00Z");
    assert_eq!(
        store.backfill_live_battery(
            &headers(&conflict),
            &serde_json::to_vec(&conflict["payload"]).unwrap()
        ),
        Err(IngestionError::DeliveryConflict)
    );
    for record in &records {
        store
            .backfill_live_battery(
                &headers(record),
                &serde_json::to_vec(&record["payload"]).unwrap(),
            )
            .unwrap();
    }
    store.finish_battery_backfill().unwrap();
    assert!(!store.needs_battery_backfill().unwrap());
    assert_eq!(store.statistics().unwrap(), statistics);
    assert_eq!(store.positions().unwrap(), positions);
    assert_eq!(
        store
            .connection
            .query_row::<String, _, _>("SELECT state_json FROM live_adapter_state", [], |row| row
                .get(0))
            .unwrap(),
        state
    );
    let after = runtime.results(&store);
    assert_eq!(after.battery_reading, expected.battery_reading);
    assert_eq!(after.sessions, before.sessions);
    assert_eq!(after.pricing, before.pricing);
    assert_eq!(after.charging, before.charging);
    for (offset, record) in records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record).unwrap();
    }
    let mut resend = records[4].clone();
    resend["headers"]["txid"] = json!("fictional-backfilled-resend");
    resend["payload"]["isResend"] = json!(true);
    runtime.ingest(&mut store, 5, &resend).unwrap();
    assert_eq!(runtime.results(&store), after);
    drop(store);
    let mut store = runtime.open();
    assert!(!store.needs_battery_backfill().unwrap());
    assert_eq!(runtime.results(&store), after);
}

#[test]
fn battery_backfill_leaves_expired_history_unknown_and_does_not_repeat_completed_scans() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    runtime.ingest(&mut store, 0, &runtime.records[0]).unwrap();
    store
        .connection
        .execute_batch(
            "UPDATE live_records SET signal_json = json_remove(signal_json, '$.battery_percent');",
        )
        .unwrap();
    assert!(store.needs_battery_backfill().unwrap());
    store.finish_battery_backfill().unwrap();
    assert!(runtime.results(&store).battery_reading.is_none());
    drop(store);
    let mut store = runtime.open();
    assert!(!store.needs_battery_backfill().unwrap());
    store.finish_battery_backfill().unwrap();
    assert!(runtime.results(&store).battery_reading.is_none());
}

#[test]
fn live_fixture_covers_never_old_fresh_and_independently_dated_charging() {
    let runtime = Runtime::new();
    let mut store = runtime.open();
    let fixture: Value =
        serde_json::from_slice(include_bytes!("../../../../dev/fixtures/live.json")).unwrap();
    let examples = fixture["battery_examples"].as_array().unwrap();
    runtime
        .ingest(&mut store, 0, &examples[3]["record"])
        .unwrap();
    assert!(runtime.results(&store).battery_reading.is_none());
    for (offset, example) in examples[..3].iter().enumerate() {
        runtime
            .ingest(&mut store, offset as i64 + 1, &example["record"])
            .unwrap();
    }
    let result = runtime.results(&store);
    assert_eq!(
        f64::from(result.battery_reading.as_ref().unwrap().battery_percent),
        62.0
    );
    assert_eq!(
        result.battery_reading.unwrap().observed_at,
        "2026-09-10T11:52:00Z"
    );
    assert!(result.charging.as_ref().unwrap().active);
    assert_eq!(result.charging.unwrap().observed_at, "2026-09-10T11:48:00Z");
}
