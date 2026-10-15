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
            "INSERT INTO chargers VALUES (?1,?2,?3,?4,'charger-owner',?5,?6)",
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
