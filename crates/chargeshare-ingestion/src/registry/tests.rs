use super::*;
use crate::{CommitBoundary, DeliveryCoordinate, PartitionRange, PartitionStart, SourceBinding};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(0);

struct Runtime {
    directory: std::path::PathBuf,
    bootstrap: RegistryBootstrap,
    source: SourceBinding,
    records: Vec<Value>,
}

impl Runtime {
    fn new() -> Self {
        let directory = std::path::Path::new("/tmp").join(format!(
            "chargeshare-registry-test-{}-{}",
            std::process::id(),
            NEXT_RUNTIME.fetch_add(1, Ordering::Relaxed)
        ));
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let mut input: Value =
            serde_json::from_slice(include_bytes!("../../../../dev/live-config.example.json"))
                .unwrap();
        input["database"] = json!(directory.join("ingestion.sqlite3"));
        let bootstrap = RegistryBootstrap::from_json(&serde_json::to_vec(&input).unwrap()).unwrap();
        let source = SourceBinding {
            epoch: bootstrap.source_epoch.clone(),
            topic: format!("{}_V", bootstrap.topic_namespace),
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
            bootstrap,
            source,
            records: fixture["records"].as_array().unwrap().clone(),
        }
    }

    fn legacy(&self) -> Store {
        Store::open(
            &self.bootstrap.database,
            &self.bootstrap.legacy_manifest().unwrap(),
            &self.source,
        )
        .unwrap()
    }
    fn open(&self) -> Store {
        Store::open_runtime(&self.bootstrap.settings, &self.source).unwrap()
    }
    fn setup(&self) -> RegistrySetup {
        RegistrySetup {
            car_names: [("vehicle-a".to_owned(), "Car A".to_owned())].into(),
            charger_name: "Charger A".to_owned(),
            charger_owner: "charger-owner".to_owned(),
        }
    }
    fn ingest_legacy_records(&self, store: &mut Store) {
        let registry = self.bootstrap.decoder_registry().unwrap();
        for (offset, record) in self.records.iter().enumerate() {
            let (disposition, checkpoint) = store
                .prepare_live(
                    &registry,
                    &Self::headers(record),
                    &serde_json::to_vec(&record["payload"]).unwrap(),
                )
                .unwrap();
            store
                .commit_with_checkpoint(
                    DeliveryCoordinate {
                        partition: 0,
                        offset: offset as i64,
                    },
                    offset as i64,
                    disposition,
                    checkpoint.as_ref(),
                    &mut |_| Ok(()),
                )
                .unwrap();
        }
    }
    fn migrate(&self, store: &mut Store) {
        migrate(store, &self.bootstrap, &self.setup(), &mut |_| Ok(())).unwrap();
    }
    fn second(&self) -> CarRegistration {
        CarRegistration {
            device: "FICTIONAL-VIN-B".to_owned(),
            vehicle: "vehicle-b".to_owned(),
            owner: "owner-b".to_owned(),
            name: "Model Y sample".to_owned(),
            make: Some("Fictional make".to_owned()),
            model: Some("Model Y".to_owned()),
            year: Some(2026),
            chargers: vec![self.bootstrap.shared_charger.charger_id.clone()],
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
    fn ingest(&self, store: &mut Store, offset: i64, record: &Value) {
        store
            .ingest_live(
                &self.bootstrap.settings,
                DeliveryCoordinate {
                    partition: 0,
                    offset,
                },
                store.positions().unwrap()[0].next_offset,
                &Self::headers(record),
                &serde_json::to_vec(&record["payload"]).unwrap(),
            )
            .unwrap();
    }
    fn second_records(&self) -> Vec<Value> {
        self.records
            .iter()
            .map(|record| {
                let mut record = record.clone();
                record["headers"]["vin"] = json!("FICTIONAL-VIN-B");
                record["payload"]["vin"] = json!("FICTIONAL-VIN-B");
                for field in record["payload"]["data"].as_array_mut().unwrap() {
                    if field["key"] == "Soc" {
                        field["value"]["doubleValue"] = json!(67);
                    }
                }
                record
            })
            .collect()
    }
    fn result(store: &Store, owner: &str, vehicle: &str) -> crate::ScopedResults {
        store
            .results(
                &OwnerId::new(owner).unwrap(),
                &VehicleId::new(vehicle).unwrap(),
            )
            .unwrap()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn retained_rows(store: &Store) -> Value {
    let mut tables = serde_json::Map::new();
    for table in [
        "event_variants",
        "deliveries",
        "live_records",
        "live_adapter_state",
        "partition_progress",
        "geofence_versions",
        "rate_model_versions",
    ] {
        let mut statement = store
            .connection
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))
            .unwrap();
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| {
                        let value = row.get_ref(index)?;
                        Ok(match value {
                            rusqlite::types::ValueRef::Null => Value::Null,
                            rusqlite::types::ValueRef::Integer(value) => json!(value),
                            rusqlite::types::ValueRef::Text(value) => {
                                json!(std::str::from_utf8(value).unwrap())
                            }
                            _ => panic!("unexpected storage type"),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        tables.insert(table.to_owned(), json!(rows));
    }
    Value::Object(tables)
}

#[test]
fn migration_preserves_retained_rows_and_foreign_keys_and_rolls_back_before_commit() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    let registry = runtime.bootstrap.decoder_registry().unwrap();
    for (offset, record) in runtime.records.iter().enumerate() {
        let (disposition, checkpoint) = store
            .prepare_live(
                &registry,
                &Runtime::headers(record),
                &serde_json::to_vec(&record["payload"]).unwrap(),
            )
            .unwrap();
        store
            .commit_with_checkpoint(
                DeliveryCoordinate {
                    partition: 0,
                    offset: offset as i64,
                },
                offset as i64,
                disposition,
                checkpoint.as_ref(),
                &mut |_| Ok(()),
            )
            .unwrap();
    }
    let before = retained_rows(&store);
    assert_eq!(
        migrate(
            &mut store,
            &runtime.bootstrap,
            &runtime.setup(),
            &mut |boundary| if boundary == CommitBoundary::BeforeCommit {
                Err(IngestionError::Timeout)
            } else {
                Ok(())
            }
        ),
        Err(IngestionError::Timeout)
    );
    assert!(!is_registry(&store.connection).unwrap());
    assert_eq!(retained_rows(&store), before);
    runtime.migrate(&mut store);
    assert_eq!(retained_rows(&store), before);
    for table in ["event_variants", "live_records", "live_adapter_state"] {
        let target: String = store
            .connection
            .query_row(
                &format!("SELECT \"table\" FROM pragma_foreign_key_list('{table}')"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(target, "cars");
    }
    assert_eq!(
        store
            .registry
            .as_ref()
            .unwrap()
            .chargers
            .values()
            .next()
            .unwrap()
            .owner,
        "charger-owner"
    );
    let result = Runtime::result(&store, "owner-a", "vehicle-a");
    assert_eq!(result.observed_ac_kwh, "0.500000");
    assert_eq!(result.pricing.display_subtotal.as_deref(), Some("$0.11"));
    drop(store);
    migrate_registry(
        &runtime.bootstrap.database,
        &runtime.bootstrap,
        &runtime.setup(),
    )
    .unwrap();
    let store = runtime.open();
    assert_eq!(retained_rows(&store), before);
    assert_eq!(Runtime::result(&store, "owner-a", "vehicle-a"), result);
}

#[test]
fn migration_conflicting_input_and_missing_bootstrap_preserve_the_database() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    let before = retained_rows(&store);
    let mut changed = runtime.bootstrap.clone();
    changed.mappings[0].device = "FICTIONAL-DIFFERENT-IDENTITY".to_owned();
    assert_eq!(
        migrate(&mut store, &changed, &runtime.setup(), &mut |_| Ok(())),
        Err(IngestionError::RegistryConflict)
    );
    assert_eq!(retained_rows(&store), before);
    drop(store);
    assert!(matches!(
        Store::open_runtime(&runtime.bootstrap.settings, &runtime.source),
        Err(IngestionError::MigrationInputRequired)
    ));
}

#[test]
fn registry_commands_are_atomic_idempotent_private_and_preserve_inactive_ingestion() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    assert_eq!(
        Store::add_car(&runtime.bootstrap.database, runtime.second()),
        Err(IngestionError::OwnershipConflict)
    );
    drop(store);
    for change in ["name", "charger", "year", "owner"] {
        let mut second = runtime.second();
        match change {
            "name" => second.name = "  ".to_owned(),
            "charger" => second.chargers.push("missing-charger".to_owned()),
            "year" => second.year = Some(100),
            "owner" => second.owner = "bad owner".to_owned(),
            _ => unreachable!(),
        }
        assert_eq!(
            Store::add_car(&runtime.bootstrap.database, second),
            Err(IngestionError::RegistryConflict)
        );
        assert_eq!(runtime.open().registry.as_ref().unwrap().cars.len(), 1);
    }
    assert_eq!(
        Store::add_car(&runtime.bootstrap.database, runtime.second()),
        Ok(RegistryWrite::RecoveryPending)
    );
    assert_eq!(
        Store::add_car(&runtime.bootstrap.database, runtime.second()),
        Ok(RegistryWrite::RecoveryPending)
    );
    let mut conflict = runtime.second();
    conflict.name = "Unexpected rename".to_owned();
    assert_eq!(
        Store::add_car(&runtime.bootstrap.database, conflict),
        Err(IngestionError::RegistryConflict)
    );
    let mut store = runtime.open();
    assert_eq!(store.active_scopes().unwrap().len(), 1);
    store
        .capture_recovery(
            "vehicle-b",
            &runtime.source,
            &[PartitionRange {
                partition: 0,
                low: 0,
                high: 0,
            }],
        )
        .unwrap();
    store
        .complete_recovery("vehicle-b", &mut |_| Ok(()))
        .unwrap();
    drop(store);
    assert_eq!(
        Store::add_car(&runtime.bootstrap.database, runtime.second()),
        Ok(RegistryWrite::RecoveryComplete)
    );
    Store::rename_car(&runtime.bootstrap.database, "vehicle-b", "Neighbor sample").unwrap();
    Store::rename_car(&runtime.bootstrap.database, "vehicle-b", "Neighbor sample").unwrap();
    Store::remove_car(&runtime.bootstrap.database, "vehicle-b").unwrap();
    Store::remove_car(&runtime.bootstrap.database, "vehicle-b").unwrap();
    assert_eq!(
        Store::add_car(&runtime.bootstrap.database, runtime.second()),
        Err(IngestionError::RegistryConflict)
    );
    let mut store = runtime.open();
    let second = store.registry.as_ref().unwrap().car("vehicle-b").unwrap();
    assert_eq!(second.registration.name, "Neighbor sample");
    assert!(!second.active);
    assert_eq!(second.registration.chargers, runtime.second().chargers);
    for (offset, record) in runtime.second_records().iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    let result = Runtime::result(&store, "owner-b", "vehicle-b");
    assert_eq!(result.observed_ac_kwh, "0.500000");
    assert_eq!(result.pricing.display_subtotal.as_deref(), Some("$0.11"));
    let safe = serde_json::to_string(&result).unwrap();
    assert!(!safe.contains("FICTIONAL-VIN") && !safe.contains("latitude"));
    assert_eq!(store.active_scopes().unwrap().len(), 1);
    drop(store);
    Store::remove_car(&runtime.bootstrap.database, "vehicle-a").unwrap();
    assert!(runtime.open().active_scopes().unwrap().is_empty());
}

#[test]
fn recovery_preserves_first_car_and_original_rejections_resumes_and_hands_off_once() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    let first = Runtime::result(&store, "owner-a", "vehicle-a");
    let first_state: String = store
        .connection
        .query_row(
            "SELECT state_json FROM live_adapter_state WHERE vehicle='vehicle-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let second = runtime.second_records();
    runtime.ingest(&mut store, 4, &second[0]);
    runtime.ingest(&mut store, 5, &second[1]);
    let before = retained_rows(&store);
    drop(store);
    Store::add_car(&runtime.bootstrap.database, runtime.second()).unwrap();
    let mut store = runtime.open();
    assert_eq!(Runtime::result(&store, "owner-a", "vehicle-a"), first);
    let bounds = [PartitionRange {
        partition: 0,
        low: 0,
        high: 8,
    }];
    store
        .capture_recovery("vehicle-b", &runtime.source, &bounds)
        .unwrap();
    for offset in 0..4 {
        store
            .recover_live_observed(
                "vehicle-b",
                DeliveryCoordinate {
                    partition: 0,
                    offset,
                },
                &Runtime::headers(&runtime.records[offset as usize]),
                &serde_json::to_vec(&runtime.records[offset as usize]["payload"]).unwrap(),
                &mut |_| Ok(()),
            )
            .unwrap();
    }
    assert_eq!(
        store.complete_recovery("vehicle-b", &mut |_| Ok(())),
        Err(IngestionError::RecoveryPending)
    );
    assert_eq!(
        store.recover_live_observed(
            "vehicle-b",
            DeliveryCoordinate {
                partition: 0,
                offset: 4
            },
            &Runtime::headers(&second[0]),
            &serde_json::to_vec(&second[0]["payload"]).unwrap(),
            &mut |boundary| if boundary == CommitBoundary::BeforeCommit {
                Err(IngestionError::Timeout)
            } else {
                Ok(())
            }
        ),
        Err(IngestionError::Timeout)
    );
    assert_eq!(
        store.recovery_positions("vehicle-b").unwrap()[0].next_offset,
        4
    );
    store
        .recover_live_observed(
            "vehicle-b",
            DeliveryCoordinate {
                partition: 0,
                offset: 4,
            },
            &Runtime::headers(&second[0]),
            &serde_json::to_vec(&second[0]["payload"]).unwrap(),
            &mut |boundary| {
                if boundary == CommitBoundary::AfterCommit {
                    Err(IngestionError::Timeout)
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    drop(store);
    let mut store = runtime.open();
    assert_eq!(
        store
            .capture_recovery("vehicle-b", &runtime.source, &bounds)
            .unwrap()[0]
            .next_offset,
        5
    );
    for offset in 5..8 {
        let record = &second[(offset - 4) as usize];
        store
            .recover_live_observed(
                "vehicle-b",
                DeliveryCoordinate {
                    partition: 0,
                    offset,
                },
                &Runtime::headers(record),
                &serde_json::to_vec(&record["payload"]).unwrap(),
                &mut |_| Ok(()),
            )
            .unwrap();
    }
    assert_eq!(store.positions().unwrap()[0].next_offset, 6);
    assert_eq!(
        store.complete_recovery("vehicle-b", &mut |boundary| if boundary
            == CommitBoundary::BeforeCommit
        {
            Err(IngestionError::Timeout)
        } else {
            Ok(())
        }),
        Err(IngestionError::Timeout)
    );
    store
        .complete_recovery("vehicle-b", &mut |_| Ok(()))
        .unwrap();
    let recovered = Runtime::result(&store, "owner-b", "vehicle-b");
    assert_eq!(recovered.observed_ac_kwh, "0.500000");
    assert_eq!(recovered.sessions.len(), 1);
    for offset in 4..8 {
        runtime.ingest(&mut store, offset, &second[(offset - 4) as usize]);
    }
    assert_eq!(Runtime::result(&store, "owner-b", "vehicle-b"), recovered);
    assert_eq!(Runtime::result(&store, "owner-a", "vehicle-a"), first);
    assert_eq!(
        store
            .connection
            .query_row::<String, _, _>(
                "SELECT state_json FROM live_adapter_state WHERE vehicle='vehicle-a'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        first_state
    );
    let after = retained_rows(&store);
    assert_eq!(
        &after["deliveries"].as_array().unwrap()[..6],
        before["deliveries"].as_array().unwrap()
    );
    assert!(store.pending_recoveries().unwrap().is_empty());
}

#[test]
fn captured_recovery_refuses_expiry_truncation_source_changes_and_offset_gaps() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    drop(store);
    Store::add_car(&runtime.bootstrap.database, runtime.second()).unwrap();
    let mut store = runtime.open();
    let bounds = [PartitionRange {
        partition: 0,
        low: 2,
        high: 8,
    }];
    store
        .capture_recovery("vehicle-b", &runtime.source, &bounds)
        .unwrap();
    for bounds in [
        [PartitionRange {
            partition: 0,
            low: 3,
            high: 8,
        }],
        [PartitionRange {
            partition: 0,
            low: 2,
            high: 7,
        }],
    ] {
        assert_eq!(
            store.capture_recovery("vehicle-b", &runtime.source, &bounds),
            Err(IngestionError::RecoveryGap)
        );
    }
    let mut changed = runtime.source.clone();
    changed.epoch = "live-changed".to_owned();
    assert_eq!(
        store.capture_recovery("vehicle-b", &changed, &bounds),
        Err(IngestionError::SourceMismatch)
    );
    assert_eq!(
        store.recover_live_observed(
            "vehicle-b",
            DeliveryCoordinate {
                partition: 0,
                offset: 3
            },
            &Runtime::headers(&runtime.second_records()[0]),
            &serde_json::to_vec(&runtime.second_records()[0]["payload"]).unwrap(),
            &mut |_| Ok(())
        ),
        Err(IngestionError::RecoveryGap)
    );
    assert_eq!(
        store.recovery_positions("vehicle-b").unwrap()[0].next_offset,
        2
    );
}

fn seed_charger(
    store: &mut Store,
    id: &str,
    center: crate::NormalizedLocation,
    rates: Vec<String>,
) {
    let geofence = ChargerGeofenceConfig {
        center,
        radius_meters: 100,
    };
    let geofence_version = version("geofence", &(&id, &geofence)).unwrap();
    store
        .connection
        .execute(
            "INSERT INTO chargers VALUES (?1,?2,?3,?4,'charger-owner',?5,?6,NULL)",
            params![
                id,
                "Garage charger",
                json(&geofence.center).unwrap(),
                100,
                json(&rates).unwrap(),
                geofence_version
            ],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO charger_geofence_versions VALUES (?1,?2,?3)",
            params![geofence_version, id, json(&geofence).unwrap()],
        )
        .unwrap();
    for rate in &rates {
        let definition: crate::RateModelConfig = serde_json::from_str(
            &store
                .connection
                .query_row::<String, _, _>(
                    "SELECT config_json FROM rate_model_versions WHERE id=?1",
                    [rate],
                    |row| row.get(0),
                )
                .unwrap(),
        )
        .unwrap();
        let start = definition.to_core().unwrap().effective_from_time().unwrap();
        store
            .connection
            .execute(
                "INSERT INTO charger_rate_assignments VALUES (?1,?2,?3)",
                params![id, start, rate],
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE chargers SET active_rate_version_id=?1 WHERE id=?2",
                params![rate, id],
            )
            .unwrap();
    }
    let fixed = crate::RuntimeSettings::from_json(
        store
            .connection
            .query_row::<String, _, _>(
                "SELECT settings_json FROM registry_runtime_settings",
                [],
                |row| row.get(0),
            )
            .unwrap()
            .as_bytes(),
    )
    .unwrap()
    .manifest()
    .unwrap();
    store.refresh_registry(&fixed).unwrap();
}

#[test]
fn explicit_multi_charger_links_select_distinct_histories_and_hold_conflicting_matches() {
    let mut runtime = Runtime::new();
    let mut alternative = runtime.bootstrap.settings.rate_models[0].clone();
    alternative.id = "fictional-garage-rate-v1".to_owned();
    for period in &mut alternative.periods {
        period.usd_per_kwh = "0.4".to_owned();
    }
    runtime
        .bootstrap
        .settings
        .rate_models
        .push(alternative.clone());
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    seed_charger(
        &mut store,
        "garage-charger",
        crate::NormalizedLocation::from_coordinates(12.3656789, 45.6789123),
        vec![alternative.id],
    );
    drop(store);
    let mut second = runtime.second();
    second.chargers.push("garage-charger".to_owned());
    Store::add_car(&runtime.bootstrap.database, second).unwrap();
    let mut store = runtime.open();
    store
        .capture_recovery(
            "vehicle-b",
            &runtime.source,
            &[PartitionRange {
                partition: 0,
                low: 0,
                high: 0,
            }],
        )
        .unwrap();
    store
        .complete_recovery("vehicle-b", &mut |_| Ok(()))
        .unwrap();
    let second = runtime.second_records();
    for (offset, record) in second.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    let mut garage = second.clone();
    for (index, record) in garage.iter_mut().enumerate() {
        record["headers"]["txid"] = json!(format!("garage-{index}"));
        record["payload"]["createdAt"] = json!(format!("2026-09-02T12:{index:02}:00Z"));
        for field in record["payload"]["data"].as_array_mut().unwrap() {
            if field["key"] == "Location" {
                field["value"]["locationValue"]["latitude"] = json!(12.3656789);
            }
        }
        runtime.ingest(&mut store, index as i64 + 4, record);
    }
    let result = Runtime::result(&store, "owner-b", "vehicle-b");
    assert_eq!(result.sessions.len(), 2);
    assert_eq!(result.observed_ac_kwh, "1.000000");
    assert_eq!(result.pricing.display_subtotal.as_deref(), Some("$0.31"));
    let garage = store
        .results_for_charger(
            &OwnerId::new("owner-b").unwrap(),
            &VehicleId::new("vehicle-b").unwrap(),
            "garage-charger",
        )
        .unwrap();
    assert_eq!(garage.sessions.len(), 1);
    assert_eq!(garage.observed_ac_kwh, "0.500000");
    assert_eq!(garage.pricing.display_subtotal.as_deref(), Some("$0.20"));
    assert_eq!(
        garage.sessions[0].charger.as_ref().unwrap().name,
        "Garage charger"
    );
    assert_eq!(
        Runtime::result(&store, "owner-a", "vehicle-a")
            .sessions
            .len(),
        0
    );
    let mut conflicting = second.clone();
    for (index, record) in conflicting.iter_mut().enumerate() {
        record["headers"]["txid"] = json!(format!("conflict-{index}"));
        record["payload"]["createdAt"] = json!(format!("2026-09-03T12:{index:02}:00Z"));
        if index == 2 {
            for field in record["payload"]["data"].as_array_mut().unwrap() {
                if field["key"] == "Location" {
                    field["value"]["locationValue"]["latitude"] = json!(12.3656789);
                }
            }
        }
        runtime.ingest(&mut store, index as i64 + 8, record);
    }
    let result = Runtime::result(&store, "owner-b", "vehicle-b");
    assert_eq!(result.sessions[2].classification, "Held");
    assert!(result.sessions[2].pricing.quote.is_none());
    drop(store);
    assert_eq!(
        Runtime::result(&runtime.open(), "owner-b", "vehicle-b"),
        result
    );
}

#[test]
fn overlapping_linked_chargers_hold_and_unlinked_chargers_do_not_affect_matching() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    seed_charger(
        &mut store,
        "overlapping-charger",
        runtime.bootstrap.shared_charger.geofence.center.clone(),
        Vec::new(),
    );
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    assert_eq!(
        Runtime::result(&store, "owner-a", "vehicle-a").sessions[0].classification,
        "SharedCharger"
    );
    drop(store);
    let mut second = runtime.second();
    second.chargers.push("overlapping-charger".to_owned());
    Store::add_car(&runtime.bootstrap.database, second).unwrap();
    let mut store = runtime.open();
    store
        .capture_recovery(
            "vehicle-b",
            &runtime.source,
            &[PartitionRange {
                partition: 0,
                low: 0,
                high: 0,
            }],
        )
        .unwrap();
    store
        .complete_recovery("vehicle-b", &mut |_| Ok(()))
        .unwrap();
    for (offset, record) in runtime.second_records().iter().enumerate() {
        runtime.ingest(&mut store, offset as i64 + 4, record);
    }
    let result = Runtime::result(&store, "owner-b", "vehicle-b");
    assert_eq!(result.sessions[0].classification, "Held");
    assert_eq!(result.eligible_shared_charger_kwh, "0.000000");
}

#[test]
fn mocked_kafka_startup_recovers_rejected_and_unconsumed_backlog_before_ordinary_handoff() {
    use rdkafka::config::ClientConfig;
    use rdkafka::message::{Header, OwnedHeaders};
    use rdkafka::mocking::MockCluster;
    use rdkafka::producer::{BaseProducer, BaseRecord, Producer};
    use std::time::{Duration, Instant};
    let cluster = MockCluster::new(1).unwrap();
    let mut runtime = Runtime::new();
    cluster.create_topic(&runtime.source.topic, 1, 1).unwrap();
    runtime.bootstrap.settings.kafka_bootstrap = cluster.bootstrap_servers();
    runtime.source = crate::kafka::live_source(&runtime.bootstrap.settings).unwrap();
    let producer: BaseProducer = ClientConfig::new()
        .set("bootstrap.servers", cluster.bootstrap_servers())
        .set("log_level", "0")
        .create()
        .unwrap();
    let mut records = runtime.records.clone();
    records.extend(runtime.second_records());
    for record in &records {
        let payload = serde_json::to_vec(&record["payload"]).unwrap();
        let mut headers = OwnedHeaders::new();
        for (key, value) in Runtime::headers(record) {
            headers = headers.insert(Header {
                key: &key,
                value: Some(&value),
            });
        }
        producer
            .send(
                BaseRecord::<(), _>::to(&runtime.source.topic)
                    .partition(0)
                    .payload(&payload)
                    .headers(headers),
            )
            .unwrap();
    }
    producer.flush(Duration::from_secs(5)).unwrap();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    let mut driver = crate::kafka::LiveIngestion::connect_live(
        runtime.bootstrap.settings.clone(),
        runtime.source.clone(),
        &mut store,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while store.positions().unwrap()[0].next_offset < 6 {
        assert!(Instant::now() < deadline);
        driver.poll(&mut store).unwrap();
    }
    let first = Runtime::result(&store, "owner-a", "vehicle-a");
    assert_eq!(store.statistics().unwrap().rejected_deliveries, 3);
    drop(driver);
    drop(store);
    Store::add_car(&runtime.bootstrap.database, runtime.second()).unwrap();
    let mut store = runtime.open();
    let mut driver = crate::kafka::LiveIngestion::connect_live(
        runtime.bootstrap.settings.clone(),
        runtime.source.clone(),
        &mut store,
    )
    .unwrap();
    assert_eq!(store.positions().unwrap()[0].next_offset, 6);
    assert!(store.pending_recoveries().unwrap().is_empty());
    let second = Runtime::result(&store, "owner-b", "vehicle-b");
    assert_eq!(second.observed_ac_kwh, "0.500000");
    assert_eq!(store.active_scopes().unwrap().len(), 2);
    let deadline = Instant::now() + Duration::from_secs(10);
    while store.positions().unwrap()[0].next_offset < 8 {
        assert!(Instant::now() < deadline);
        driver.poll(&mut store).unwrap();
    }
    assert_eq!(Runtime::result(&store, "owner-b", "vehicle-b"), second);
    assert_eq!(Runtime::result(&store, "owner-a", "vehicle-a"), first);
    let mut changed = runtime.source.clone();
    changed.epoch = "live-changed-marker".to_owned();
    std::fs::write(
        runtime.bootstrap.kafka_config().source_epoch_path,
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    assert_eq!(driver.poll(&mut store), Err(IngestionError::SourceMismatch));
}

#[test]
fn migrating_old_checkpoint_keeps_legacy_redelivery_and_resend_dispositions() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    let registry = runtime.bootstrap.decoder_registry().unwrap();
    for (offset, record) in runtime.records.iter().enumerate() {
        let (disposition, checkpoint) = store
            .prepare_live(
                &registry,
                &Runtime::headers(record),
                &serde_json::to_vec(&record["payload"]).unwrap(),
            )
            .unwrap();
        store
            .commit_with_checkpoint(
                DeliveryCoordinate {
                    partition: 0,
                    offset: offset as i64,
                },
                offset as i64,
                disposition,
                checkpoint.as_ref(),
                &mut |_| Ok(()),
            )
            .unwrap();
    }
    runtime.migrate(&mut store);
    let before = Runtime::result(&store, "owner-a", "vehicle-a");
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    let mut resend = runtime.records[2].clone();
    resend["headers"]["txid"] = json!("fictional-resend");
    resend["payload"]["isResend"] = json!(true);
    runtime.ingest(&mut store, 4, &resend);
    assert_eq!(Runtime::result(&store, "owner-a", "vehicle-a"), before);
    drop(store);
    assert_eq!(
        Runtime::result(&runtime.open(), "owner-a", "vehicle-a"),
        before
    );
}

#[test]
fn runtime_settings_ignore_legacy_membership_and_fixed_contract_changes_still_refuse() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    drop(store);
    let mut input = serde_json::to_value(&runtime.bootstrap.settings).unwrap();
    input["mappings"] =
        json!([{"device":"FICTIONAL-UNADMITTED","vehicle":"unadmitted","owner":"owner-b"}]);
    input["shared_charger"] = json!({"invalid":"ignored legacy section"});
    let settings = crate::RuntimeSettings::from_json(&serde_json::to_vec(&input).unwrap()).unwrap();
    let store = Store::open_runtime(&settings, &runtime.source).unwrap();
    assert_eq!(store.registry.as_ref().unwrap().cars.len(), 1);
    drop(store);
    input["rate_models"][0]["periods"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let reordered =
        crate::RuntimeSettings::from_json(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert!(Store::open_runtime(&reordered, &runtime.source).is_ok());
    input["rate_models"][0]["periods"][0]["usd_per_kwh"] = json!("0.8");
    let changed = crate::RuntimeSettings::from_json(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert!(matches!(
        Store::open_runtime(&changed, &runtime.source),
        Err(IngestionError::ConfigurationMismatch)
    ));
}

#[test]
fn registry_api_serves_names_charger_filter_empty_active_scopes_and_private_historical_reads() {
    use rdkafka::mocking::MockCluster;
    use std::io::{Read, Write};
    use std::sync::{Arc, atomic::AtomicBool};
    let cluster = MockCluster::new(1).unwrap();
    let mut runtime = Runtime::new();
    cluster.create_topic(&runtime.source.topic, 1, 1).unwrap();
    runtime.bootstrap.settings.kafka_bootstrap = cluster.bootstrap_servers();
    runtime.source = crate::kafka::live_source(&runtime.bootstrap.settings).unwrap();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    drop(store);
    Store::remove_car(&runtime.bootstrap.database, "vehicle-a").unwrap();
    let mut store = runtime.open();
    let driver = crate::kafka::LiveIngestion::connect_live(
        runtime.bootstrap.settings.clone(),
        runtime.source.clone(),
        &mut store,
    )
    .unwrap();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        crate::viewing::serve_with_driver(listener, store, driver, &server_stop)
    });
    let request = |path: &str| {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: synthetic.invalid\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    };
    let scopes = request("/api/poc/scopes");
    assert!(scopes.starts_with("HTTP/1.1 200"));
    assert!(scopes.contains("\"scopes\":[]"));
    let path = format!(
        "/api/poc/owners/owner-a/vehicles/vehicle-a/results?charger={}",
        runtime.bootstrap.shared_charger.charger_id
    );
    let results = request(&path);
    assert!(results.starts_with("HTTP/1.1 200"));
    assert!(results.contains("Car A") && results.contains("Charger A"));
    assert!(!results.contains("FICTIONAL-VIN") && !results.contains("latitude"));
    let refused = request("/api/poc/owners/owner-a/vehicles/vehicle-a/results?charger=unknown");
    assert!(refused.starts_with("HTTP/1.1 404"));
    assert!(refused.ends_with("{\"error\":\"charger_refused\"}"));
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap().unwrap();
}

fn cpsf_definition() -> crate::RateModelConfig {
    serde_json::from_slice(include_bytes!("../../../../dev/cpsf-green-rate.json")).unwrap()
}

#[test]
fn cpsf_assignment_replays_winter_acceptance_cost_monthly_totals_and_provenance() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    let fixture: Value =
        serde_json::from_slice(include_bytes!("../../../../dev/fixtures/cpsf-green.json")).unwrap();
    for (offset, reading) in fixture["readings"].as_array().unwrap().iter().enumerate() {
        let mut record = runtime.records[1].clone();
        record["headers"]["txid"] = json!(format!("fictional-cpsf-{offset}"));
        record["payload"]["createdAt"] = reading["created_at"].clone();
        record["payload"]["data"][0]["value"]["detailedChargeStateValue"] =
            reading["state"].clone();
        record["payload"]["data"][1]["value"] = json!({"stringValue": reading["ac_kwh"]});
        runtime.ingest(&mut store, offset as i64, &record);
    }
    let sample = Runtime::result(&store, "owner-a", "vehicle-a");
    assert_eq!(sample.observed_ac_kwh, "24.410000");
    assert_eq!(
        sample.pricing.exact_subtotal.as_deref(),
        Some("7.0972256000000000 USD")
    );
    assert_eq!(sample.pricing.display_subtotal.as_deref(), Some("$7.10"));
    let evidence = retained_rows(&store);
    drop(store);
    let definition = cpsf_definition();
    let active_from = 1772438400;
    for _ in 0..2 {
        Store::assign_charger_rate(
            &runtime.bootstrap.database,
            "fictional-shared-charger",
            active_from,
            &definition,
        )
        .unwrap();
    }
    let store = runtime.open();
    assert_eq!(
        store
            .connection
            .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        6
    );
    let result = Runtime::result(&store, "owner-a", "vehicle-a");
    assert_ne!(result.revision, sample.revision);
    assert_eq!(result.observed_ac_kwh, "24.410000");
    assert_eq!(
        result.pricing.exact_subtotal.as_deref(),
        Some("8.3027020000000000 USD")
    );
    assert_eq!(result.pricing.display_subtotal.as_deref(), Some("$8.30"));
    let quote = result.sessions[0].pricing.quote.as_ref().unwrap();
    assert_eq!(
        quote
            .slices
            .iter()
            .map(|slice| slice.allocated_ac_kwh.as_str())
            .collect::<Vec<_>>(),
        ["15.830000", "8.580000"]
    );
    assert!(
        quote
            .slices
            .iter()
            .all(|slice| slice.model_version == definition.id)
    );
    let month = result
        .monthly
        .months
        .iter()
        .find(|month| month.month == "2026-03")
        .unwrap();
    assert_eq!(month.total.as_ref().unwrap().display_cost, "$8.30");
    assert_eq!(
        month.total.as_ref().unwrap().exact_cost,
        "8.3027020000000000 USD"
    );
    assert!(result.pricing.estimate_boundary.contains("UNVERIFIED"));
    let saved: String = store
        .connection
        .query_row(
            "SELECT config_json FROM rate_model_versions WHERE id = ?1",
            [&definition.id],
            |row| row.get(0),
        )
        .unwrap();
    let loaded: crate::RateModelConfig = serde_json::from_str(&saved).unwrap();
    assert_eq!(loaded, definition);
    assert_eq!(loaded.periods.len(), 8);
    for (season, off, part, peak) in [
        ("Winter", "0.27578", "0.44298", "0.45887"),
        ("Summer", "0.27533", "0.47472", "0.58234"),
    ] {
        let periods = loaded
            .periods
            .iter()
            .filter(|period| period.season == season)
            .map(|period| {
                (
                    period.period.as_str(),
                    period.start_second,
                    period.end_second,
                    period.usd_per_kwh.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            periods,
            [
                ("off_peak", 0, 54000, off),
                ("part_peak", 54000, 57600, part),
                ("peak", 57600, 75600, peak),
                ("part_peak", 75600, 86400, part)
            ]
        );
    }
    let off_peak = loaded
        .to_core()
        .unwrap()
        .definition()
        .periods
        .iter()
        .find(|period| {
            period.season == chargeshare_core::Season::Winter && period.period == "off_peak"
        })
        .unwrap()
        .rate
        .cost_for(chargeshare_core::Energy::parse_kwh("24.41").unwrap());
    assert_eq!(off_peak.to_string(), "6.7317898000000000 USD");
    assert_eq!(off_peak.round_half_up_to_cents().as_cents(), 673);
    let rows = retained_rows(&store);
    for table in [
        "event_variants",
        "deliveries",
        "live_records",
        "partition_progress",
    ] {
        assert_eq!(rows[table], evidence[table]);
    }
    assert_eq!(
        store
            .connection
            .query_row::<i64, _, _>("SELECT count(*) FROM rate_model_versions", [], |row| row
                .get(0))
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .connection
            .query_row::<i64, _, _>("SELECT count(*) FROM charger_rate_assignments", [], |row| {
                row.get(0)
            })
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .connection
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'rate_periods'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    drop(store);
    assert_eq!(
        Runtime::result(&runtime.open(), "owner-a", "vehicle-a"),
        result
    );
}

#[test]
fn rate_setup_refuses_reused_definitions_duplicate_instants_and_invalid_references_atomically() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    let before = retained_rows(&store);
    drop(store);
    let mut definition = cpsf_definition();
    assert!(
        Store::assign_charger_rate(
            &runtime.bootstrap.database,
            "missing-charger",
            1772438400,
            &definition
        )
        .is_err()
    );
    assert_eq!(retained_rows(&runtime.open()), before);
    definition.periods[0].usd_per_kwh = "invalid".to_owned();
    assert!(
        Store::assign_charger_rate(
            &runtime.bootstrap.database,
            "fictional-shared-charger",
            1772438400,
            &definition
        )
        .is_err()
    );
    assert_eq!(retained_rows(&runtime.open()), before);
    let definition = cpsf_definition();
    assert!(
        Store::assign_charger_rate(
            &runtime.bootstrap.database,
            "fictional-shared-charger",
            i64::MAX,
            &definition
        )
        .is_err()
    );
    assert_eq!(retained_rows(&runtime.open()), before);
    Store::assign_charger_rate(
        &runtime.bootstrap.database,
        "fictional-shared-charger",
        1772438400,
        &definition,
    )
    .unwrap();
    let before = retained_rows(&runtime.open());
    let mut changed = definition.clone();
    changed.periods[0].usd_per_kwh = "0.3".to_owned();
    assert_eq!(
        Store::assign_charger_rate(
            &runtime.bootstrap.database,
            "fictional-shared-charger",
            1772438500,
            &changed
        ),
        Err(IngestionError::ConfigurationMismatch)
    );
    changed.id = "fictional-other-version".to_owned();
    assert_eq!(
        Store::assign_charger_rate(
            &runtime.bootstrap.database,
            "fictional-shared-charger",
            1772438400,
            &changed
        ),
        Err(IngestionError::RegistryConflict)
    );
    let store = runtime.open();
    assert_eq!(retained_rows(&store), before);
    assert!(store.connection.execute("INSERT INTO charger_rate_assignments VALUES ('fictional-shared-charger',1772438400,?1)", [&definition.id]).is_err());
    assert!(store.connection.execute("INSERT INTO charger_rate_assignments VALUES ('fictional-shared-charger',1772438500,'missing-version')", []).is_err());
}

#[test]
fn rate_setup_changes_only_the_explicit_charger_independent_of_the_car_owner() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    store.connection.execute("INSERT INTO chargers SELECT 'fictional-other-charger','Other sample',location,radius,'owner-b',rate_plan,geofence_version,active_rate_version_id FROM chargers", []).unwrap();
    let geofence = runtime.bootstrap.shared_charger.geofence.clone();
    let geofence_id = version("geofence", &("fictional-other-charger", &geofence)).unwrap();
    store
        .connection
        .execute(
            "UPDATE chargers SET geofence_version=?1 WHERE id='fictional-other-charger'",
            [&geofence_id],
        )
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO charger_geofence_versions VALUES (?1,'fictional-other-charger',?2)",
            params![geofence_id, json(&geofence).unwrap()],
        )
        .unwrap();
    store.connection.execute("INSERT INTO charger_rate_assignments SELECT 'fictional-other-charger',active_from,rate_version_id FROM charger_rate_assignments", []).unwrap();
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    let before = Runtime::result(&store, "owner-a", "vehicle-a");
    drop(store);
    Store::assign_charger_rate(
        &runtime.bootstrap.database,
        "fictional-shared-charger",
        1772438400,
        &cpsf_definition(),
    )
    .unwrap();
    let store = runtime.open();
    let other: String = store
        .connection
        .query_row(
            "SELECT active_rate_version_id FROM chargers WHERE id='fictional-other-charger'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(other, "pge-ev2-a-2026-03-v1");
    let result = Runtime::result(&store, "owner-a", "vehicle-a");
    assert_eq!(before.observed_ac_kwh, result.observed_ac_kwh);
    assert_eq!(result.pricing.display_subtotal.as_deref(), Some("$0.14"));
    assert_eq!(
        store.registry.as_ref().unwrap().chargers["fictional-shared-charger"].owner,
        "charger-owner"
    );
}

#[test]
fn schema_four_database_migrates_in_place_preserving_existing_prices_evidence_and_json() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    let before = Runtime::result(&store, "owner-a", "vehicle-a");
    let rows = retained_rows(&store);
    store.connection.execute_batch("ALTER TABLE cars DROP COLUMN color;
        DROP TABLE charger_rate_assignments;
        ALTER TABLE chargers DROP COLUMN active_rate_version_id;
        ALTER TABLE runtime_metadata RENAME TO current_metadata;
        CREATE TABLE runtime_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton = 1), schema_version INTEGER NOT NULL CHECK(schema_version = 4), manifest_json TEXT NOT NULL, source_json TEXT NOT NULL) STRICT;
        INSERT INTO runtime_metadata SELECT singleton,4,manifest_json,source_json FROM current_metadata;
        DROP TABLE current_metadata;
        PRAGMA user_version = 4;").unwrap();
    drop(store);
    assert_eq!(schema_version(&runtime.bootstrap.database), 4);
    startup(&private_config(&runtime, |input| {
        input.as_object_mut().unwrap().remove("mappings");
        input.as_object_mut().unwrap().remove("shared_charger");
    }))
    .unwrap();
    assert_eq!(schema_version(&runtime.bootstrap.database), 6);
    let store = runtime.open();
    assert_eq!(
        store
            .connection
            .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        6
    );
    assert_eq!(
        store.active_scopes().unwrap()[0]["color"],
        DEFAULT_CAR_COLOR
    );
    assert_eq!(retained_rows(&store), rows);
    assert_eq!(Runtime::result(&store, "owner-a", "vehicle-a"), before);
    assert!(
        !store
            .connection
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}

#[test]
fn typed_loading_rejects_malformed_and_invalid_saved_rate_definitions() {
    for contents in ["{", "{}", "invalid-price", "unknown-field", "mismatched-id"] {
        let runtime = Runtime::new();
        let mut store = runtime.legacy();
        runtime.migrate(&mut store);
        let mut config = serde_json::to_value(cpsf_definition()).unwrap();
        let saved = match contents {
            "invalid-price" => {
                config["periods"][0]["usd_per_kwh"] = json!("NaN");
                config.to_string()
            }
            "unknown-field" => {
                config["unexpected"] = json!(true);
                config.to_string()
            }
            "mismatched-id" => {
                config["id"] = json!("other-id");
                config.to_string()
            }
            value => value.to_owned(),
        };
        store
            .connection
            .execute(
                "INSERT INTO rate_model_versions VALUES (?1,?2)",
                params![cpsf_definition().id, saved],
            )
            .unwrap();
        drop(store);
        assert!(runtime.open_result().is_err());
    }
}

impl Runtime {
    fn open_result(&self) -> Result<Store, IngestionError> {
        Store::open_runtime(&self.bootstrap.settings, &self.source)
    }
}

#[test]
fn intraperiod_charger_change_splits_replayed_counter_segment_and_conserves_energy() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    drop(store);
    Store::assign_charger_rate(
        &runtime.bootstrap.database,
        "fictional-shared-charger",
        1788264150,
        &cpsf_definition(),
    )
    .unwrap();
    let store = runtime.open();
    let result = Runtime::result(&store, "owner-a", "vehicle-a");
    let slices = &result.sessions[0].pricing.quote.as_ref().unwrap().slices;
    assert_eq!(
        slices
            .iter()
            .map(|slice| slice.allocated_ac_kwh.as_str())
            .collect::<Vec<_>>(),
        ["0.125000", "0.125000", "0.250000"]
    );
    assert_eq!(slices[0].end_utc_seconds, "1788264150");
    assert_eq!(slices[1].start_utc_seconds, "1788264150");
    assert_eq!(slices[0].model_version, "pge-ev2-a-2026-03-v1");
    assert_eq!(slices[1].model_version, cpsf_definition().id);
    assert_eq!(result.pricing.priced_ac_kwh, "0.500000");
    assert_eq!(
        result.pricing.exact_subtotal.as_deref(),
        Some("0.1314462500000000 USD")
    );
    assert_eq!(result.pricing.display_subtotal.as_deref(), Some("$0.13"));
    drop(store);
    assert_eq!(
        Runtime::result(&runtime.open(), "owner-a", "vehicle-a"),
        result
    );
}

#[test]
fn failed_rate_assignment_transaction_rolls_back_the_new_definition_and_active_reference() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    let before = retained_rows(&store);
    store.connection.execute_batch("CREATE TRIGGER reject_rate_setup BEFORE INSERT ON charger_rate_assignments BEGIN SELECT RAISE(ABORT, 'synthetic assignment failure'); END;").unwrap();
    drop(store);
    assert!(
        Store::assign_charger_rate(
            &runtime.bootstrap.database,
            "fictional-shared-charger",
            1772438400,
            &cpsf_definition()
        )
        .is_err()
    );
    let store = runtime.open();
    assert_eq!(retained_rows(&store), before);
    let active: String = store
        .connection
        .query_row("SELECT active_rate_version_id FROM chargers", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(active, "pge-ev2-a-2026-03-v1");
    assert_eq!(
        store
            .connection
            .query_row::<i64, _, _>("SELECT count(*) FROM charger_rate_assignments", [], |row| {
                row.get(0)
            })
            .unwrap(),
        1
    );
}

#[test]
fn invalid_legacy_rate_reference_rolls_back_schema_five_migration_in_place() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    store.connection.execute_batch("ALTER TABLE cars DROP COLUMN color;
        DROP TABLE charger_rate_assignments;
        ALTER TABLE chargers DROP COLUMN active_rate_version_id;
        UPDATE chargers SET rate_plan = '[\"missing-version\"]';
        ALTER TABLE runtime_metadata RENAME TO current_metadata;
        CREATE TABLE runtime_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton = 1), schema_version INTEGER NOT NULL CHECK(schema_version = 4), manifest_json TEXT NOT NULL, source_json TEXT NOT NULL) STRICT;
        INSERT INTO runtime_metadata SELECT singleton,4,manifest_json,source_json FROM current_metadata;
        DROP TABLE current_metadata;
        PRAGMA user_version = 4;").unwrap();
    let before = retained_rows(&store);
    drop(store);
    assert!(runtime.open_result().is_err());
    let connection = Connection::open(&runtime.bootstrap.database).unwrap();
    assert_eq!(
        connection
            .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        4
    );
    assert_eq!(connection.query_row::<i64, _, _>("SELECT count(*) FROM pragma_table_info('chargers') WHERE name='active_rate_version_id'", [], |row| row.get(0)).unwrap(), 0);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_schema WHERE name='charger_rate_assignments'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    let rate: String = connection
        .query_row("SELECT config_json FROM rate_model_versions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        json!([["pge-ev2-a-2026-03-v1", rate]]),
        before["rate_model_versions"]
    );
}

fn downgrade_car_colors(store: &Store) {
    store.connection.execute_batch("ALTER TABLE cars DROP COLUMN color;
        ALTER TABLE runtime_metadata RENAME TO color_current_metadata;
        CREATE TABLE runtime_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton = 1), schema_version INTEGER NOT NULL CHECK(schema_version = 5), manifest_json TEXT NOT NULL, source_json TEXT NOT NULL) STRICT;
        INSERT INTO runtime_metadata SELECT singleton,5,manifest_json,source_json FROM color_current_metadata;
        DROP TABLE color_current_metadata;
        PRAGMA user_version = 5;").unwrap();
}

#[test]
fn startup_migrates_schema_five_colors_preserving_rates_evidence_and_neutral_defaults() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    for (offset, record) in runtime.records.iter().enumerate() {
        runtime.ingest(&mut store, offset as i64, record);
    }
    drop(store);
    Store::assign_charger_rate(
        &runtime.bootstrap.database,
        "fictional-shared-charger",
        1772438400,
        &cpsf_definition(),
    )
    .unwrap();
    let store = runtime.open();
    let evidence = retained_rows(&store);
    let result = Runtime::result(&store, "owner-a", "vehicle-a");
    let scopes = store.active_scopes().unwrap();
    let rates = json(
        &crate::charger_rates::load_assignments(&store.connection, "fictional-shared-charger")
            .unwrap(),
    )
    .unwrap();
    downgrade_car_colors(&store);
    drop(store);
    assert_eq!(schema_version(&runtime.bootstrap.database), 5);
    let config = private_config(&runtime, |input| {
        input.as_object_mut().unwrap().remove("mappings");
        input.as_object_mut().unwrap().remove("shared_charger");
        input.as_object_mut().unwrap().remove("registry_setup");
    });
    startup(&config).unwrap();
    assert_eq!(schema_version(&runtime.bootstrap.database), 6);
    let migrated = database_dump(&runtime.bootstrap.database);
    startup(&config).unwrap();
    assert_eq!(database_dump(&runtime.bootstrap.database), migrated);
    let store = runtime.open();
    assert_eq!(store.active_scopes().unwrap(), scopes);
    assert_eq!(Runtime::result(&store, "owner-a", "vehicle-a"), result);
    assert_eq!(retained_rows(&store), evidence);
    assert_eq!(
        json(
            &crate::charger_rates::load_assignments(&store.connection, "fictional-shared-charger")
                .unwrap()
        )
        .unwrap(),
        rates
    );
    assert_eq!(store.active_scopes().unwrap()[0]["color"], "#808080");
    assert_eq!(
        store
            .connection
            .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        6
    );
    drop(store);
    Store::add_car(&runtime.bootstrap.database, runtime.second()).unwrap();
    let store = runtime.open();
    assert_eq!(
        store
            .registry
            .as_ref()
            .unwrap()
            .car("vehicle-b")
            .unwrap()
            .color,
        DEFAULT_CAR_COLOR
    );
    assert!(matches!(
        Store::set_car_color(&runtime.bootstrap.database, "vehicle-a", "#B8BDC3"),
        Err(IngestionError::OwnershipConflict)
    ));
    drop(store);
    for invalid in [
        "",
        "silver",
        "#fff",
        "#1234567",
        "#12GG56",
        " #123456",
        "#123456\n",
    ] {
        assert!(Store::set_car_color(&runtime.bootstrap.database, "vehicle-a", invalid).is_err());
    }
    assert!(Store::set_car_color(&runtime.bootstrap.database, "missing-car", "#123456").is_err());
    Store::set_car_color(&runtime.bootstrap.database, "vehicle-a", "#b8BdC3").unwrap();
    Store::set_car_color(&runtime.bootstrap.database, "vehicle-b", "#16181C").unwrap();
    let store = runtime.open();
    assert_eq!(store.active_scopes().unwrap()[0]["color"], "#b8BdC3");
    assert_eq!(
        store
            .registry
            .as_ref()
            .unwrap()
            .car("vehicle-b")
            .unwrap()
            .color,
        "#16181C"
    );
    assert!(
        store
            .connection
            .execute("UPDATE cars SET color = '#zzzzzz'", [])
            .is_err()
    );
    assert_eq!(
        retained_rows(&store)["event_variants"],
        evidence["event_variants"]
    );
}

#[test]
fn failed_startup_color_migration_rolls_back_schema_registry_evidence_and_rates() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    downgrade_car_colors(&store);
    let before = retained_rows(&store);
    let rates = json(
        &crate::charger_rates::load_assignments(&store.connection, "fictional-shared-charger")
            .unwrap(),
    )
    .unwrap();
    store
        .connection
        .execute_batch("CREATE TABLE color_legacy_metadata(synthetic_failure INTEGER);")
        .unwrap();
    drop(store);
    assert!(startup(&private_config(&runtime, |_| {})).is_err());
    let connection = Connection::open(&runtime.bootstrap.database).unwrap();
    assert_eq!(
        connection
            .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        5
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM pragma_table_info('cars') WHERE name='color'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        json(
            &crate::charger_rates::load_assignments(&connection, "fictional-shared-charger")
                .unwrap()
        )
        .unwrap(),
        rates
    );
    connection
        .execute_batch("DROP TABLE color_legacy_metadata")
        .unwrap();
    drop(connection);
    assert_eq!(retained_rows(&runtime.open()), before);
}

fn private_config(runtime: &Runtime, edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut input: Value =
        serde_json::from_slice(include_bytes!("../../../../dev/live-config.example.json")).unwrap();
    input["database"] = json!(runtime.bootstrap.database);
    edit(&mut input);
    serde_json::to_vec(&input).unwrap()
}

fn startup(config: &[u8]) -> Result<(), IngestionError> {
    let settings = crate::RuntimeSettings::from_json(config).unwrap();
    migrate_on_startup(config, &settings, &mut |_| Ok(()))
}

fn database_dump(path: &std::path::Path) -> Value {
    let connection = Connection::open(path).unwrap();
    let mut dump = serde_json::Map::new();
    dump.insert(
        "user_version".to_owned(),
        json!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap()
        ),
    );
    let tables: Vec<(String, String)> = connection
        .prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for (table, sql) in tables {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY 1"))
            .unwrap();
        let columns = statement.column_count();
        let rows: Vec<Vec<String>> = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| Ok(format!("{:?}", row.get_ref(index)?)))
                    .collect()
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        dump.insert(table, json!({"sql": sql, "rows": rows}));
    }
    Value::Object(dump)
}

fn schema_version(path: &std::path::Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn startup_bootstraps_legacy_registry_through_schema_six_and_repeats_as_a_no_op() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.ingest_legacy_records(&mut store);
    let before = retained_rows(&store);
    drop(store);
    let config = private_config(&runtime, |input| {
        input.as_object_mut().unwrap().remove("registry_setup");
    });
    startup(&config).unwrap();
    assert_eq!(schema_version(&runtime.bootstrap.database), 6);
    let migrated = database_dump(&runtime.bootstrap.database);
    startup(&config).unwrap();
    assert_eq!(database_dump(&runtime.bootstrap.database), migrated);
    let store = runtime.open();
    assert_eq!(retained_rows(&store), before);
    let registry = store.registry.as_ref().unwrap();
    let charger = registry.chargers.values().next().unwrap();
    assert_eq!(charger.name, "Home charger");
    assert_eq!(charger.owner, "owner-a");
    assert!(charger.active_rate_version_id.is_some());
    assert_eq!(registry.cars[0].registration.name, "vehicle-a");
    assert_eq!(registry.cars[0].color, DEFAULT_CAR_COLOR);
    assert_eq!(
        Runtime::result(&store, "owner-a", "vehicle-a").observed_ac_kwh,
        "0.500000"
    );
}

#[test]
fn startup_uses_declared_private_setup_and_runtime_settings_ignore_it() {
    let runtime = Runtime::new();
    drop(runtime.legacy());
    let config = private_config(&runtime, |_| {});
    startup(&config).unwrap();
    let store = runtime.open();
    let registry = store.registry.as_ref().unwrap();
    let charger = registry.chargers.values().next().unwrap();
    assert_eq!(
        (charger.name.as_str(), charger.owner.as_str()),
        ("Charger A", "charger-owner")
    );
    assert_eq!(registry.cars[0].registration.name, "Car A");
    drop(store);
    assert!(RegistryBootstrap::from_json(&config).is_ok());
    let unknown_car = private_config(&runtime, |input| {
        input["registry_setup"] = json!({"car_names": {"vehicle-z": "Car Z"}});
    });
    assert_eq!(
        RegistryBootstrap::startup_input(&unknown_car).err(),
        Some(IngestionError::RegistryConflict)
    );
}

#[test]
fn startup_failure_before_commit_leaves_the_legacy_database_untouched() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.ingest_legacy_records(&mut store);
    drop(store);
    let before = database_dump(&runtime.bootstrap.database);
    let config = private_config(&runtime, |_| {});
    let settings = crate::RuntimeSettings::from_json(&config).unwrap();
    assert_eq!(
        migrate_on_startup(&config, &settings, &mut |boundary| {
            if boundary == CommitBoundary::BeforeCommit {
                Err(IngestionError::Timeout)
            } else {
                Ok(())
            }
        }),
        Err(IngestionError::Timeout)
    );
    assert_eq!(database_dump(&runtime.bootstrap.database), before);
    assert_eq!(schema_version(&runtime.bootstrap.database), 3);
}

#[test]
fn startup_leaves_an_already_migrated_registry_unchanged() {
    let runtime = Runtime::new();
    let mut store = runtime.legacy();
    runtime.migrate(&mut store);
    drop(store);
    let before = database_dump(&runtime.bootstrap.database);
    startup(&private_config(&runtime, |_| {})).unwrap();
    let without_legacy = private_config(&runtime, |input| {
        input.as_object_mut().unwrap().remove("mappings");
        input.as_object_mut().unwrap().remove("shared_charger");
    });
    startup(&without_legacy).unwrap();
    assert_eq!(database_dump(&runtime.bootstrap.database), before);
    assert_eq!(
        runtime
            .open()
            .registry
            .as_ref()
            .unwrap()
            .chargers
            .values()
            .next()
            .unwrap()
            .owner,
        "charger-owner"
    );
}

#[test]
fn startup_refuses_without_legacy_input_or_with_ambiguous_charger_owner() {
    let runtime = Runtime::new();
    let without_legacy = private_config(&runtime, |input| {
        input.as_object_mut().unwrap().remove("mappings");
        input.as_object_mut().unwrap().remove("shared_charger");
    });
    assert_eq!(
        startup(&without_legacy),
        Err(IngestionError::MigrationInputRequired)
    );
    assert!(!runtime.bootstrap.database.exists());
    drop(runtime.legacy());
    let before = database_dump(&runtime.bootstrap.database);
    assert_eq!(
        startup(&without_legacy),
        Err(IngestionError::MigrationInputRequired)
    );
    let two_owners = private_config(&runtime, |input| {
        input.as_object_mut().unwrap().remove("registry_setup");
        input["mappings"].as_array_mut().unwrap().push(json!({
            "device": "FICTIONAL-VIN-B", "owner": "owner-b", "vehicle": "vehicle-b"
        }));
    });
    assert_eq!(
        startup(&two_owners),
        Err(IngestionError::MigrationInputRequired)
    );
    assert_eq!(database_dump(&runtime.bootstrap.database), before);
}
