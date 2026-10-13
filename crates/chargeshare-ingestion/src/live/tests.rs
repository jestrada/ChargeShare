use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use chargeshare_core::{Energy, Ledger, OwnerId, QualityFlag, VehicleId};
use serde_json::{Value, json};

use crate::live_config::LiveConfig;
use crate::{
    CommitBoundary, DeliveryCoordinate, IngestionError, PartitionStart, SourceBinding, Store,
};

static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(0);

struct Runtime {
    directory: PathBuf,
    config: LiveConfig,
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
        let config = LiveConfig::from_json(&serde_json::to_vec(&example).unwrap()).unwrap();
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
        Store::open(
            &self.config.database,
            &self.config.manifest().unwrap(),
            &self.source,
        )
        .unwrap()
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
        super::decode(
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
        let manifest = runtime.config.manifest().unwrap();
        let vehicle = VehicleId::new("vehicle-a").unwrap();
        let mut ledger = Ledger::default();
        ledger
            .register(vehicle.clone(), OwnerId::new("owner-a").unwrap())
            .unwrap();
        let mut state = super::AdapterState::default();
        for record in runtime
            .records
            .iter()
            .take(if variant == "missing_end" { 3 } else { 4 })
            .skip(usize::from(variant == "missing_start"))
        {
            let (signal, _, resend) = super::decode(
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
            matches!(variant, "complete" | "invalid_power" | "missing_power")
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
fn missing_end_and_missing_terminal_counter_remain_held_without_silence_inference() {
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
        assert_eq!(result.state, "held");
        assert_eq!(result.observed_ac_kwh, "0.250000");
        assert_eq!(result.eligible_shared_charger_kwh, "0.000000");
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
fn coordinate_and_vin_text_never_reach_database_companions_or_http_results() {
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
    for entry in fs::read_dir(&runtime.directory).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        for private in forbidden {
            assert!(
                !bytes
                    .windows(private.len())
                    .any(|window| window == private.as_bytes())
            );
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
        assert_eq!(result.eligible_shared_charger_kwh, "0.000000");
        assert_eq!(
            result.sessions[0].classification,
            if variant == "outside" {
                "OtherCharger"
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
