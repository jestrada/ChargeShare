use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use chargeshare_core::{Energy, OwnerId, QualityFlag, VehicleId};
use chargeshare_ingestion::*;
use rusqlite::{Connection, params};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct RuntimeDirectory(PathBuf);

impl RuntimeDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chargeshare-synthetic-persistence-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path).unwrap();
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("evidence.sqlite")
    }
}

impl Drop for RuntimeDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn source() -> SourceBinding {
    SourceBinding {
        epoch: "synthetic-test-epoch".to_owned(),
        topic: SYNTHETIC_TOPIC.to_owned(),
        cluster_id: Some("synthetic-cluster".to_owned()),
        topic_id: Some("synthetic-topic".to_owned()),
        partitions: vec![PartitionStart {
            partition: 0,
            initial_offset: 0,
        }],
    }
}

fn evidence(
    device: &str,
    connection: &str,
    position: u64,
    second: u8,
    kind: NormalizedEventKind,
    counter: Option<NormalizedCounter>,
) -> AcceptedEvidence {
    let suffix = device.strip_prefix("device-").unwrap();
    AcceptedEvidence {
        event: NormalizedEvent {
            vehicle: format!("vehicle-{suffix}"),
            connection: connection.to_owned(),
            position: position.to_string(),
            time: i64::from(second),
            kind,
            charge_type: NormalizedChargeType::Ac,
            counter,
        },
        provenance: EvidenceProvenance {
            device: device.to_owned(),
            owner: format!("owner-{suffix}"),
            created_at: format!("1970-01-01T00:00:{second:02}Z"),
            input_contract: INPUT_CONTRACT_VERSION.to_owned(),
            mapping_version: "synthetic-mapping-v1".to_owned(),
            manifest_version: "synthetic-manifest-v1".to_owned(),
            normalizer_version: NORMALIZER_VERSION.to_owned(),
            timestamp_version: TIMESTAMP_VERSION.to_owned(),
            is_resend: false,
        },
    }
}

fn valid_counter(kwh: &str) -> Option<NormalizedCounter> {
    Some(NormalizedCounter::Valid {
        kwh: kwh.to_owned(),
    })
}

fn manifest(records: &[AcceptedEvidence]) -> Manifest {
    Manifest::new(ManifestConfig {
        input_contract: INPUT_CONTRACT_VERSION.to_owned(),
        mapping_version: "synthetic-mapping-v1".to_owned(),
        manifest_version: "synthetic-manifest-v1".to_owned(),
        normalizer_version: NORMALIZER_VERSION.to_owned(),
        timestamp_version: TIMESTAMP_VERSION.to_owned(),
        mappings: vec![
            DeviceMapping {
                device: "device-a".to_owned(),
                vehicle: "vehicle-a".to_owned(),
                owner: "owner-a".to_owned(),
            },
            DeviceMapping {
                device: "device-b".to_owned(),
                vehicle: "vehicle-b".to_owned(),
                owner: "owner-b".to_owned(),
            },
        ],
        annotations: records
            .iter()
            .map(|record| Annotation {
                device: record.provenance.device.clone(),
                created_at: record.provenance.created_at.clone(),
                position: record.event.position.clone(),
                connection: record.event.connection.clone(),
                kind: record.event.kind,
                charge_type: record.event.charge_type,
            })
            .collect(),
    })
    .unwrap()
}

fn commit(store: &mut Store, offset: i64, record: AcceptedEvidence) -> CommitOutcome {
    store
        .commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset,
            },
            offset,
            RecordDisposition::Accepted(Box::new(record)),
        )
        .unwrap()
}

fn count(path: &Path, table: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn atomic_delivery_and_domain_dedup_survive_restart_with_separate_ten_and_four() {
    let directory = RuntimeDirectory::new();
    let records = vec![
        evidence(
            "device-a",
            "connection-1",
            1,
            0,
            NormalizedEventKind::Start,
            valid_counter("0.000000"),
        ),
        evidence(
            "device-b",
            "connection-1",
            1,
            0,
            NormalizedEventKind::Start,
            valid_counter("0.000000"),
        ),
        evidence(
            "device-a",
            "connection-1",
            2,
            10,
            NormalizedEventKind::End,
            valid_counter("10.000000"),
        ),
        evidence(
            "device-b",
            "connection-1",
            2,
            10,
            NormalizedEventKind::End,
            valid_counter("4.000000"),
        ),
    ];
    let manifest = manifest(&records);
    let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    for (offset, record) in records.iter().enumerate() {
        assert!(commit(&mut store, offset as i64, record.clone()).inserted_variant);
    }
    assert!(
        store
            .commit_record(
                DeliveryCoordinate {
                    partition: 0,
                    offset: 0
                },
                0,
                RecordDisposition::Accepted(Box::new(records[0].clone()))
            )
            .unwrap()
            .duplicate_delivery
    );
    let mut resend = records[0].clone();
    resend.provenance.is_resend = true;
    assert!(!commit(&mut store, 4, resend).inserted_variant);
    assert_eq!(
        store.statistics().unwrap(),
        StoreStatistics {
            accepted_deliveries: 5,
            rejected_deliveries: 0,
            event_variants: 4
        }
    );
    let owner_a = OwnerId::new("owner-a").unwrap();
    let vehicle_a = VehicleId::new("vehicle-a").unwrap();
    let owner_b = OwnerId::new("owner-b").unwrap();
    let vehicle_b = VehicleId::new("vehicle-b").unwrap();
    let before_a = store.summary(&owner_a, &vehicle_a).unwrap();
    let before_b = store.summary(&owner_b, &vehicle_b).unwrap();
    assert_eq!(before_a.observed_ac, Energy::parse_kwh("10").unwrap());
    assert_eq!(before_b.observed_ac, Energy::parse_kwh("4").unwrap());
    assert_eq!(before_a.eligible_shared_charger, Energy::ZERO);
    assert_eq!(before_b.eligible_shared_charger, Energy::ZERO);
    assert!(store.replay(&owner_a, &vehicle_b).is_err());
    assert!(
        store
            .replay(&owner_a, &VehicleId::new("unknown-vehicle").unwrap())
            .is_err()
    );
    assert!(
        store
            .replay(&owner_a, &vehicle_a)
            .unwrap()
            .summary(&vehicle_b)
            .is_err()
    );
    drop(store);
    let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    assert_eq!(
        store.positions().unwrap(),
        vec![PartitionPosition {
            partition: 0,
            next_offset: 5
        }]
    );
    assert_eq!(store.summary(&owner_a, &vehicle_a).unwrap(), before_a);
    assert_eq!(store.summary(&owner_b, &vehicle_b).unwrap(), before_b);
    assert!(
        store
            .commit_record(
                DeliveryCoordinate {
                    partition: 0,
                    offset: 2
                },
                5,
                RecordDisposition::Accepted(Box::new(records[2].clone()))
            )
            .unwrap()
            .duplicate_delivery
    );
    assert_eq!(store.positions().unwrap()[0].next_offset, 5);
}

#[test]
fn changed_delivery_and_invalid_public_evidence_never_overwrite_or_advance() {
    let directory = RuntimeDirectory::new();
    let record = evidence(
        "device-a",
        "connection-1",
        1,
        0,
        NormalizedEventKind::Start,
        valid_counter("0.000000"),
    );
    let manifest = manifest(std::slice::from_ref(&record));
    let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    commit(&mut store, 0, record.clone());
    let before = store.statistics().unwrap();
    let mut changed = record.clone();
    changed.event.counter = valid_counter("1.000000");
    assert_eq!(
        store.commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset: 0
            },
            1,
            RecordDisposition::Accepted(Box::new(changed))
        ),
        Err(IngestionError::DeliveryConflict)
    );
    let mut invalid = record;
    invalid.provenance.device = "unknown-device".to_owned();
    assert_eq!(
        store.commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset: 1
            },
            1,
            RecordDisposition::Accepted(Box::new(invalid))
        ),
        Err(IngestionError::InvalidConfiguration)
    );
    assert_eq!(
        store.commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset: 0
            },
            0,
            RecordDisposition::Rejected(RejectionReason::UnknownIdentity)
        ),
        Err(IngestionError::DeliveryConflict)
    );
    assert_eq!(store.statistics().unwrap(), before);
    assert_eq!(store.positions().unwrap()[0].next_offset, 1);
}

#[test]
fn canonical_text_retains_maximum_u64_positions_energy_and_negative_source_time() {
    let directory = RuntimeDirectory::new();
    let mut record = evidence(
        "device-a",
        "connection-1",
        u64::MAX,
        0,
        NormalizedEventKind::Start,
        valid_counter("18446744073709.551615"),
    );
    record.event.time = -1;
    record.provenance.created_at = "1969-12-31T23:59:59Z".to_owned();
    let manifest = manifest(std::slice::from_ref(&record));
    let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    commit(&mut store, 0, record.clone());
    drop(store);
    let connection = Connection::open(directory.database()).unwrap();
    let (position, storage_type, timestamp, retained): (String, String, i64, String) = connection
        .query_row(
            "SELECT position, typeof(position), source_time, event_json FROM event_variants",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(position, u64::MAX.to_string());
    assert_eq!(storage_type, "text");
    assert_eq!(timestamp, -1);
    assert_eq!(
        serde_json::from_str::<NormalizedEvent>(&retained).unwrap(),
        record.event
    );
    drop(connection);
    Store::open(&directory.database(), &manifest, &source()).unwrap();
}

#[test]
fn all_cross_connection_position_variants_remain_conflict_barriers() {
    let directory = RuntimeDirectory::new();
    let records = vec![
        evidence(
            "device-a",
            "connection-1",
            1,
            0,
            NormalizedEventKind::Start,
            valid_counter("0.000000"),
        ),
        evidence(
            "device-a",
            "connection-1",
            2,
            10,
            NormalizedEventKind::End,
            valid_counter("10.000000"),
        ),
        evidence(
            "device-a",
            "connection-2",
            2,
            20,
            NormalizedEventKind::Start,
            valid_counter("20.000000"),
        ),
        evidence(
            "device-a",
            "connection-2",
            3,
            30,
            NormalizedEventKind::End,
            valid_counter("25.000000"),
        ),
        evidence(
            "device-b",
            "connection-1",
            2,
            0,
            NormalizedEventKind::Start,
            valid_counter("0.000000"),
        ),
        evidence(
            "device-b",
            "connection-1",
            3,
            10,
            NormalizedEventKind::End,
            valid_counter("4.000000"),
        ),
    ];
    let manifest = manifest(&records);
    let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    for (offset, record) in records.iter().rev().enumerate() {
        commit(&mut store, offset as i64, record.clone());
    }
    assert_eq!(store.statistics().unwrap().event_variants, 6);
    let sessions = store
        .sessions(
            &OwnerId::new("owner-a").unwrap(),
            &VehicleId::new("vehicle-a").unwrap(),
        )
        .unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().all(|session| {
        session
            .quality_flags
            .contains(&QualityFlag::ConflictingEvidence)
    }));
    assert_eq!(
        store
            .summary(
                &OwnerId::new("owner-b").unwrap(),
                &VehicleId::new("vehicle-b").unwrap()
            )
            .unwrap()
            .observed_ac,
        Energy::parse_kwh("4").unwrap()
    );
}

#[test]
fn rejected_payload_and_invalid_counter_text_are_not_retained() {
    let directory = RuntimeDirectory::new();
    let record = evidence(
        "device-a",
        "connection-1",
        1,
        0,
        NormalizedEventKind::Start,
        None,
    );
    let manifest = manifest(std::slice::from_ref(&record));
    let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    let poison = br#"{"vin":"private-looking-device","createdAt":"1970-01-01T00:00:00Z","isResend":false,"data":[],"location":"private-looking-place","token":"private-looking-token"}"#;
    store
        .ingest(
            &manifest,
            DeliveryCoordinate {
                partition: 0,
                offset: 0,
            },
            0,
            Some(b"private-looking-device"),
            poison,
        )
        .unwrap();
    let invalid = br#"{"vin":"device-a","createdAt":"1970-01-01T00:00:00Z","isResend":false,"data":[{"key":"ACChargingEnergyIn","value":{"stringValue":"private-looking-counter"}},{"key":"VehicleName","value":{"stringValue":"private-looking-name"}}]}"#;
    store
        .ingest(
            &manifest,
            DeliveryCoordinate {
                partition: 0,
                offset: 1,
            },
            1,
            Some(b"device-a"),
            invalid,
        )
        .unwrap();
    assert_eq!(
        store.statistics().unwrap(),
        StoreStatistics {
            accepted_deliveries: 1,
            rejected_deliveries: 1,
            event_variants: 1
        }
    );
    drop(store);
    for entry in fs::read_dir(&directory.0).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("private-looking"));
    }
    let connection = Connection::open(directory.database()).unwrap();
    let rejected: (Option<i64>, Option<String>, String) = connection
        .query_row(
            "SELECT variant_id, provenance_json, rejection_reason FROM deliveries WHERE offset = 0",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(rejected, (None, None, "\"unsupported_payload\"".to_owned()));
}

#[test]
fn owning_lock_busy_transaction_and_statement_failure_preserve_atomic_state() {
    let directory = RuntimeDirectory::new();
    let record = evidence(
        "device-a",
        "connection-1",
        1,
        0,
        NormalizedEventKind::Start,
        None,
    );
    let manifest = manifest(std::slice::from_ref(&record));
    let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    assert!(matches!(
        Store::open(&directory.database(), &manifest, &source()),
        Err(IngestionError::OwnershipConflict)
    ));
    let connection = Connection::open(directory.database()).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = Instant::now();
    assert_eq!(
        store.commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset: 0
            },
            0,
            RecordDisposition::Accepted(Box::new(record.clone()))
        ),
        Err(IngestionError::StorageUnavailable)
    );
    assert!(started.elapsed().as_secs() < 3);
    assert_eq!(store.positions().unwrap()[0].next_offset, 0);
    assert_eq!(store.statistics().unwrap().event_variants, 0);
    connection.execute_batch("ROLLBACK; CREATE TRIGGER reject_delivery BEFORE INSERT ON deliveries BEGIN SELECT RAISE(ABORT, 'synthetic-write-failure'); END;").unwrap();
    assert_eq!(
        store.commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset: 0
            },
            0,
            RecordDisposition::Accepted(Box::new(record.clone()))
        ),
        Err(IngestionError::StorageUnavailable)
    );
    assert_eq!(store.positions().unwrap()[0].next_offset, 0);
    assert_eq!(store.statistics().unwrap().event_variants, 0);
    connection
        .execute_batch("DROP TRIGGER reject_delivery")
        .unwrap();
    commit(&mut store, 0, record);
    assert_eq!(count(&directory.database(), "deliveries"), 1);
}

#[test]
fn frozen_configuration_versions_schema_and_source_fail_without_deletion() {
    let directory = RuntimeDirectory::new();
    let record = evidence(
        "device-a",
        "connection-1",
        1,
        0,
        NormalizedEventKind::Start,
        None,
    );
    let original = manifest(std::slice::from_ref(&record));
    let mut store = Store::open(&directory.database(), &original, &source()).unwrap();
    commit(&mut store, 0, record);
    let mut changed_config = original.config().clone();
    changed_config.mapping_version = "synthetic-mapping-v2".to_owned();
    let changed = Manifest::new(changed_config).unwrap();
    assert_eq!(
        store.ingest(
            &changed,
            DeliveryCoordinate {
                partition: 0,
                offset: 1
            },
            1,
            None,
            b"{}"
        ),
        Err(IngestionError::ConfigurationMismatch)
    );
    drop(store);
    assert!(matches!(
        Store::open(&directory.database(), &changed, &source()),
        Err(IngestionError::ConfigurationMismatch)
    ));
    let mut reset_source = source();
    reset_source.epoch = "synthetic-reset-epoch".to_owned();
    assert!(matches!(
        Store::open(&directory.database(), &original, &reset_source),
        Err(IngestionError::SourceMismatch)
    ));
    let connection = Connection::open(directory.database()).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    assert!(matches!(
        Store::open(&directory.database(), &original, &source()),
        Err(IngestionError::SchemaMismatch)
    ));
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT count(*) FROM deliveries", [], |row| row.get(0))
            .unwrap(),
        1
    );
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection
        .execute(
            "UPDATE partition_progress SET next_offset = ?1",
            params![100],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        Store::open(&directory.database(), &original, &source()),
        Err(IngestionError::StorageCorrupt)
    ));
    assert_eq!(count(&directory.database(), "deliveries"), 1);
}

#[test]
fn incompatible_unversioned_database_and_corrupt_file_are_preserved() {
    let directory = RuntimeDirectory::new();
    let manifest = manifest(&[]);
    let database = directory.database();
    let connection = Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES ('synthetic-preserved');").unwrap();
    drop(connection);
    make_private(&database);
    let before = fs::read(&database).unwrap();
    assert!(matches!(
        Store::open(&database, &manifest, &source()),
        Err(IngestionError::SchemaMismatch)
    ));
    assert_eq!(fs::read(&database).unwrap(), before);
    assert_eq!(count(&database, "unrelated"), 1);
    let corrupt = directory.0.join("corrupt.sqlite");
    fs::write(&corrupt, b"synthetic-not-a-database").unwrap();
    make_private(&corrupt);
    assert!(matches!(
        Store::open(&corrupt, &manifest, &source()),
        Err(IngestionError::StorageCorrupt)
    ));
    assert_eq!(fs::read(&corrupt).unwrap(), b"synthetic-not-a-database");
}

#[test]
fn semantically_corrupt_retained_json_fails_read_only_preflight_without_byte_changes() {
    let directory = RuntimeDirectory::new();
    let record = evidence(
        "device-a",
        "connection-1",
        1,
        0,
        NormalizedEventKind::Start,
        None,
    );
    let manifest = manifest(std::slice::from_ref(&record));
    let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    commit(&mut store, 0, record);
    drop(store);
    let connection = Connection::open(directory.database()).unwrap();
    connection.execute_batch("PRAGMA journal_mode=DELETE; UPDATE event_variants SET event_json='synthetic-invalid-curated-json';").unwrap();
    drop(connection);
    let before = fs::read(directory.database()).unwrap();
    assert!(matches!(
        Store::open(&directory.database(), &manifest, &source()),
        Err(IngestionError::StorageCorrupt)
    ));
    assert_eq!(fs::read(directory.database()).unwrap(), before);
}

fn make_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}
