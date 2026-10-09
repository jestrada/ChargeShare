use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use chargeshare_core::{
    CounterProblem, Energy, ExclusionReason, Ledger, OwnerId, QualityFlag, VehicleId,
};
use chargeshare_ingestion::*;
use rusqlite::Connection;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct RuntimeDirectory(PathBuf);

impl RuntimeDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chargeshare-synthetic-recovery-{}-{}",
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
        epoch: "synthetic-recovery-epoch".to_owned(),
        topic: SYNTHETIC_TOPIC.to_owned(),
        cluster_id: Some("synthetic-cluster".to_owned()),
        topic_id: Some("synthetic-topic".to_owned()),
        partitions: vec![PartitionStart {
            partition: 0,
            initial_offset: 0,
        }],
    }
}

fn manifest(records: &[AcceptedEvidence]) -> Manifest {
    Manifest::new(ManifestConfig {
        input_contract: INPUT_CONTRACT_VERSION.to_owned(),
        mapping_version: "synthetic-mapping-v1".to_owned(),
        manifest_version: "synthetic-manifest-v1".to_owned(),
        normalizer_version: NORMALIZER_VERSION.to_owned(),
        timestamp_version: TIMESTAMP_VERSION.to_owned(),
        mappings: vec![DeviceMapping {
            device: "device-a".to_owned(),
            vehicle: "vehicle-a".to_owned(),
            owner: "owner-a".to_owned(),
        }],
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

fn record(
    position: u64,
    connection: &str,
    kind: NormalizedEventKind,
    charge_type: NormalizedChargeType,
    counter: Option<NormalizedCounter>,
) -> AcceptedEvidence {
    AcceptedEvidence {
        event: NormalizedEvent {
            vehicle: "vehicle-a".to_owned(),
            connection: connection.to_owned(),
            position: position.to_string(),
            time: position as i64,
            kind,
            charge_type,
            counter,
        },
        provenance: EvidenceProvenance {
            device: "device-a".to_owned(),
            owner: "owner-a".to_owned(),
            created_at: format!("1970-01-01T00:00:{position:02}Z"),
            input_contract: INPUT_CONTRACT_VERSION.to_owned(),
            mapping_version: "synthetic-mapping-v1".to_owned(),
            manifest_version: "synthetic-manifest-v1".to_owned(),
            normalizer_version: NORMALIZER_VERSION.to_owned(),
            timestamp_version: TIMESTAMP_VERSION.to_owned(),
            is_resend: false,
        },
    }
}

fn counter(kwh: &str) -> Option<NormalizedCounter> {
    Some(NormalizedCounter::Valid {
        kwh: kwh.to_owned(),
    })
}

#[test]
fn recovery_validates_explicit_start_source_identity_retention_and_numeric_gaps() {
    let directory = RuntimeDirectory::new();
    let manifest = manifest(&[]);
    let mut configured_source = source();
    configured_source.partitions[0].initial_offset = 5;
    let mut store = Store::open(&directory.database(), &manifest, &configured_source).unwrap();
    assert_eq!(
        store.positions().unwrap(),
        vec![PartitionPosition {
            partition: 0,
            next_offset: 5
        }]
    );
    store
        .validate_source(
            &configured_source,
            &[PartitionRange {
                partition: 0,
                low: 2,
                high: 12,
            }],
        )
        .unwrap();
    assert_eq!(
        store.validate_source(
            &configured_source,
            &[PartitionRange {
                partition: 0,
                low: 6,
                high: 12
            }]
        ),
        Err(IngestionError::RecoveryGap)
    );
    store
        .commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset: 8,
            },
            5,
            RecordDisposition::Rejected(RejectionReason::UnknownIdentity),
        )
        .unwrap();
    assert_eq!(store.positions().unwrap()[0].next_offset, 9);
    assert_eq!(
        store.commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset: 10
            },
            5,
            RecordDisposition::Rejected(RejectionReason::UnknownIdentity)
        ),
        Err(IngestionError::ProgressConflict)
    );
    assert_eq!(
        store.commit_record(
            DeliveryCoordinate {
                partition: 0,
                offset: i64::MAX
            },
            9,
            RecordDisposition::Rejected(RejectionReason::UnknownIdentity)
        ),
        Err(IngestionError::ProgressConflict)
    );
    assert_eq!(
        store.commit_record(
            DeliveryCoordinate {
                partition: 1,
                offset: 9
            },
            9,
            RecordDisposition::Rejected(RejectionReason::UnknownIdentity)
        ),
        Err(IngestionError::SourceMismatch)
    );
    assert!(
        store
            .commit_record(
                DeliveryCoordinate {
                    partition: 0,
                    offset: 8
                },
                9,
                RecordDisposition::Rejected(RejectionReason::UnknownIdentity)
            )
            .unwrap()
            .duplicate_delivery
    );
    assert_eq!(
        store.validate_source(
            &configured_source,
            &[PartitionRange {
                partition: 0,
                low: 10,
                high: 15
            }]
        ),
        Err(IngestionError::RecoveryGap)
    );
    assert_eq!(
        store.validate_source(
            &configured_source,
            &[PartitionRange {
                partition: 0,
                low: 0,
                high: 8
            }]
        ),
        Err(IngestionError::RecoveryGap)
    );
    let mut reset = configured_source.clone();
    reset.epoch = "synthetic-reset-epoch".to_owned();
    assert_eq!(
        store.validate_source(
            &reset,
            &[PartitionRange {
                partition: 0,
                low: 0,
                high: 12
            }]
        ),
        Err(IngestionError::SourceMismatch)
    );
    let mut missing_epoch = configured_source.clone();
    missing_epoch.epoch.clear();
    assert_eq!(
        store.validate_source(
            &missing_epoch,
            &[PartitionRange {
                partition: 0,
                low: 0,
                high: 12
            }]
        ),
        Err(IngestionError::InvalidConfiguration)
    );
    drop(store);
    let store = Store::open(&directory.database(), &manifest, &configured_source).unwrap();
    assert_eq!(store.positions().unwrap()[0].next_offset, 9);
    assert_eq!(store.statistics().unwrap().rejected_deliveries, 1);
}

#[test]
fn independent_partitions_have_canonical_source_binding_and_atomic_cas() {
    let directory = RuntimeDirectory::new();
    let manifest = manifest(&[]);
    let mut source = source();
    source.partitions = vec![
        PartitionStart {
            partition: 2,
            initial_offset: 20,
        },
        PartitionStart {
            partition: 0,
            initial_offset: 3,
        },
    ];
    let mut store = Store::open(&directory.database(), &manifest, &source).unwrap();
    store
        .commit_record(
            DeliveryCoordinate {
                partition: 2,
                offset: 24,
            },
            20,
            RecordDisposition::Rejected(RejectionReason::UnknownIdentity),
        )
        .unwrap();
    assert_eq!(
        store.positions().unwrap(),
        vec![
            PartitionPosition {
                partition: 0,
                next_offset: 3
            },
            PartitionPosition {
                partition: 2,
                next_offset: 25
            }
        ]
    );
    source.partitions.reverse();
    drop(store);
    let store = Store::open(&directory.database(), &manifest, &source).unwrap();
    store
        .validate_source(
            &source,
            &[
                PartitionRange {
                    partition: 2,
                    low: 20,
                    high: 25,
                },
                PartitionRange {
                    partition: 0,
                    low: 3,
                    high: 3,
                },
            ],
        )
        .unwrap();
    assert_eq!(
        store.validate_source(
            &source,
            &[
                PartitionRange {
                    partition: 2,
                    low: 20,
                    high: 25
                },
                PartitionRange {
                    partition: 2,
                    low: 20,
                    high: 25
                }
            ]
        ),
        Err(IngestionError::RecoveryGap)
    );
}

#[test]
fn all_retained_replay_orders_match_core_for_pause_omission_rollback_and_charge_types() {
    let ac = NormalizedChargeType::Ac;
    let records = vec![
        record(
            1,
            "pause-connection",
            NormalizedEventKind::Start,
            ac,
            counter("0.000000"),
        ),
        record(2, "pause-connection", NormalizedEventKind::Sample, ac, None),
        record(
            3,
            "pause-connection",
            NormalizedEventKind::Pause,
            ac,
            counter("2.000000"),
        ),
        record(
            4,
            "pause-connection",
            NormalizedEventKind::Resume,
            ac,
            counter("2.000000"),
        ),
        record(
            5,
            "pause-connection",
            NormalizedEventKind::End,
            ac,
            counter("5.000000"),
        ),
        record(
            6,
            "new-connection",
            NormalizedEventKind::Start,
            ac,
            counter("100.000000"),
        ),
        record(
            7,
            "new-connection",
            NormalizedEventKind::End,
            ac,
            counter("104.000000"),
        ),
        record(
            8,
            "missing-boundaries",
            NormalizedEventKind::Start,
            ac,
            None,
        ),
        record(9, "missing-boundaries", NormalizedEventKind::End, ac, None),
        record(
            10,
            "rollback-connection",
            NormalizedEventKind::Start,
            ac,
            counter("10.000000"),
        ),
        record(
            11,
            "rollback-connection",
            NormalizedEventKind::Sample,
            ac,
            counter("5.000000"),
        ),
        record(
            12,
            "rollback-connection",
            NormalizedEventKind::End,
            ac,
            counter("8.000000"),
        ),
        record(
            13,
            "dc-connection",
            NormalizedEventKind::Start,
            NormalizedChargeType::Dc,
            counter("0.000000"),
        ),
        record(
            14,
            "dc-connection",
            NormalizedEventKind::End,
            NormalizedChargeType::Dc,
            counter("4.000000"),
        ),
        record(
            15,
            "ambiguous-connection",
            NormalizedEventKind::Start,
            NormalizedChargeType::Ambiguous,
            counter("0.000000"),
        ),
        record(
            16,
            "ambiguous-connection",
            NormalizedEventKind::End,
            NormalizedChargeType::Ambiguous,
            counter("2.000000"),
        ),
        record(
            17,
            "invalid-connection",
            NormalizedEventKind::Start,
            ac,
            Some(NormalizedCounter::Rejected {
                reason: SafeCounterProblem::Negative,
            }),
        ),
        record(
            18,
            "invalid-connection",
            NormalizedEventKind::End,
            ac,
            counter("1.000000"),
        ),
        record(
            19,
            "missing-end",
            NormalizedEventKind::Start,
            ac,
            counter("0.000000"),
        ),
        record(
            20,
            "missing-end",
            NormalizedEventKind::Sample,
            ac,
            counter("1.000000"),
        ),
    ];
    let manifest = manifest(&records);
    let owner = OwnerId::new("owner-a").unwrap();
    let vehicle = VehicleId::new("vehicle-a").unwrap();
    let mut reference = Ledger::default();
    reference.register(vehicle.clone(), owner.clone()).unwrap();
    for evidence in &records {
        reference.ingest(evidence.event.to_core().unwrap()).unwrap();
    }
    let expected_sessions = reference.sessions(&vehicle).unwrap();
    let expected_summary = reference.summary(&vehicle).unwrap();
    for ordering in 0..3 {
        let directory = RuntimeDirectory::new();
        let mut records = records.clone();
        match ordering {
            1 => records.reverse(),
            2 => records.sort_by_key(|record| (record.event.time * 7) % 20),
            _ => {}
        }
        let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
        for (offset, evidence) in records.into_iter().enumerate() {
            store
                .commit_record(
                    DeliveryCoordinate {
                        partition: 0,
                        offset: offset as i64,
                    },
                    offset as i64,
                    RecordDisposition::Accepted(Box::new(evidence)),
                )
                .unwrap();
            if offset == 8 {
                drop(store);
                store = Store::open(&directory.database(), &manifest, &source()).unwrap();
            }
        }
        assert_eq!(store.sessions(&owner, &vehicle).unwrap(), expected_sessions);
        assert_eq!(store.summary(&owner, &vehicle).unwrap(), expected_summary);
        drop(store);
        let store = Store::open(&directory.database(), &manifest, &source()).unwrap();
        assert_eq!(store.sessions(&owner, &vehicle).unwrap(), expected_sessions);
    }
    let pause = expected_sessions
        .iter()
        .find(|session| session.id.connection.as_str() == "pause-connection")
        .unwrap();
    assert_eq!(pause.observed_ac, Energy::parse_kwh("5").unwrap());
    assert!(pause.quality_flags.is_empty());
    let missing = expected_sessions
        .iter()
        .find(|session| session.id.connection.as_str() == "missing-boundaries")
        .unwrap();
    assert!(
        missing
            .quality_flags
            .contains(&QualityFlag::MissingBaseline)
    );
    assert!(
        missing
            .quality_flags
            .contains(&QualityFlag::MissingTerminalSample)
    );
    let rollback = expected_sessions
        .iter()
        .find(|session| session.id.connection.as_str() == "rollback-connection")
        .unwrap();
    assert!(
        rollback
            .quality_flags
            .contains(&QualityFlag::CounterRollback)
    );
    let invalid = expected_sessions
        .iter()
        .find(|session| session.id.connection.as_str() == "invalid-connection")
        .unwrap();
    assert!(
        invalid
            .quality_flags
            .contains(&QualityFlag::InvalidCounter(CounterProblem::Negative))
    );
    assert!(expected_sessions.iter().any(|session| {
        session
            .exclusion_reasons
            .contains(&ExclusionReason::DcEvidence)
    }));
    assert!(expected_sessions.iter().any(|session| {
        session
            .exclusion_reasons
            .contains(&ExclusionReason::AmbiguousChargeType)
    }));
    assert_eq!(expected_summary.eligible_shared_charger, Energy::ZERO);
}

#[test]
fn child_process_crash_worker() {
    let Some(mode) = std::env::var_os("CHARGESHARE_SYNTHETIC_CRASH_MODE") else {
        return;
    };
    let database = PathBuf::from(std::env::var_os("CHARGESHARE_SYNTHETIC_CRASH_DATABASE").unwrap());
    let marker = PathBuf::from(std::env::var_os("CHARGESHARE_SYNTHETIC_CRASH_MARKER").unwrap());
    let records = crash_records();
    let mut store = Store::open(&database, &manifest(&records), &source()).unwrap();
    match mode.to_str().unwrap() {
        "before" => {
            let connection = Connection::open(&database).unwrap();
            connection
                .execute_batch("PRAGMA synchronous=FULL; BEGIN IMMEDIATE;")
                .unwrap();
            insert_crash_variant(&connection, 1, &records[0]);
            connection.execute("INSERT INTO deliveries(partition, offset, variant_id, provenance_json) VALUES (0, 0, 1, ?1)", [serde_json::to_string(&records[0].provenance).unwrap()]).unwrap();
            connection
                .execute(
                    "UPDATE partition_progress SET next_offset=1 WHERE partition=0",
                    [],
                )
                .unwrap();
            fs::write(marker, b"synthetic-before-commit").unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        "after" => {
            for (offset, record) in records.iter().enumerate() {
                store
                    .commit_record(
                        DeliveryCoordinate {
                            partition: 0,
                            offset: offset as i64,
                        },
                        offset as i64,
                        RecordDisposition::Accepted(Box::new(record.clone())),
                    )
                    .unwrap();
            }
            fs::write(marker, b"synthetic-after-commit").unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        "commit-window" => {
            let mut connection = Connection::open(&database).unwrap();
            connection
                .execute_batch("PRAGMA synchronous=FULL; PRAGMA cache_size=8;")
                .unwrap();
            let transaction = connection.transaction().unwrap();
            {
                insert_crash_variant(&transaction, 1, &records[0]);
                insert_crash_variant(&transaction, 2, &records[1]);
                let provenance = records
                    .iter()
                    .map(|record| serde_json::to_string(&record.provenance).unwrap())
                    .collect::<Vec<_>>();
                let mut statement = transaction.prepare("INSERT INTO deliveries(partition, offset, variant_id, provenance_json) VALUES (0, ?1, ?2, ?3)").unwrap();
                for offset in 0..40_000_i64 {
                    let index = (offset % 2) as usize;
                    statement
                        .execute(rusqlite::params![
                            offset,
                            index as i64 + 1,
                            provenance[index]
                        ])
                        .unwrap();
                }
            }
            transaction
                .execute(
                    "UPDATE partition_progress SET next_offset=40000 WHERE partition=0",
                    [],
                )
                .unwrap();
            fs::write(marker, b"synthetic-commit-window").unwrap();
            transaction.commit().unwrap();
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        _ => panic!("invalid synthetic crash mode"),
    }
}

fn kill_child_at_marker(database: &Path, marker: &Path, mode: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_process_crash_worker", "--nocapture"])
        .env("CHARGESHARE_SYNTHETIC_CRASH_MODE", mode)
        .env("CHARGESHARE_SYNTHETIC_CRASH_DATABASE", database)
        .env("CHARGESHARE_SYNTHETIC_CRASH_MARKER", marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !marker.exists() {
        if child.try_wait().unwrap().is_some() {
            panic!("synthetic child stopped before crash marker");
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("synthetic child crash deadline exceeded");
        }
        thread::sleep(Duration::from_millis(1));
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
}

#[test]
fn actual_child_kill_before_and_after_commit_recovers_owned_atomic_state() {
    for (mode, expected) in [("before", 0), ("after", 2)] {
        let directory = RuntimeDirectory::new();
        let records = crash_records();
        let manifest = manifest(&records);
        drop(Store::open(&directory.database(), &manifest, &source()).unwrap());
        kill_child_at_marker(
            &directory.database(),
            &directory.0.join("crash.marker"),
            mode,
        );
        let mut store = Store::open(&directory.database(), &manifest, &source()).unwrap();
        assert_eq!(store.positions().unwrap()[0].next_offset, expected);
        assert_eq!(
            store.statistics().unwrap().accepted_deliveries,
            expected as u64
        );
        assert_eq!(store.statistics().unwrap().event_variants, expected as u64);
        assert_eq!(
            store
                .summary(
                    &OwnerId::new("owner-a").unwrap(),
                    &VehicleId::new("vehicle-a").unwrap()
                )
                .unwrap()
                .observed_ac,
            Energy::parse_kwh(if expected == 0 { "0" } else { "10" }).unwrap()
        );
        for (offset, record) in records.into_iter().enumerate() {
            let resume = store.positions().unwrap()[0].next_offset;
            let reread = store
                .commit_record(
                    DeliveryCoordinate {
                        partition: 0,
                        offset: offset as i64,
                    },
                    resume,
                    RecordDisposition::Accepted(Box::new(record)),
                )
                .unwrap();
            assert_eq!(reread.duplicate_delivery, mode == "after");
        }
        assert_eq!(store.positions().unwrap()[0].next_offset, 2);
        assert_eq!(store.statistics().unwrap().accepted_deliveries, 2);
        assert_eq!(store.statistics().unwrap().event_variants, 2);
        assert_eq!(
            store
                .summary(
                    &OwnerId::new("owner-a").unwrap(),
                    &VehicleId::new("vehicle-a").unwrap()
                )
                .unwrap()
                .observed_ac,
            Energy::parse_kwh("10").unwrap()
        );
    }
}

#[test]
fn actual_commit_window_child_kill_recovers_complete_old_or_new_transaction() {
    let directory = RuntimeDirectory::new();
    let manifest = manifest(&crash_records());
    drop(Store::open(&directory.database(), &manifest, &source()).unwrap());
    kill_child_at_marker(
        &directory.database(),
        &directory.0.join("crash.marker"),
        "commit-window",
    );
    let store = Store::open(&directory.database(), &manifest, &source()).unwrap();
    let progress = store.positions().unwrap()[0].next_offset;
    assert!(progress == 0 || progress == 40_000);
    assert_eq!(
        store.statistics().unwrap().accepted_deliveries,
        progress as u64
    );
    assert_eq!(store.statistics().unwrap().rejected_deliveries, 0);
    assert_eq!(
        store.statistics().unwrap().event_variants,
        if progress == 0 { 0 } else { 2 }
    );
    assert_eq!(
        store
            .summary(
                &OwnerId::new("owner-a").unwrap(),
                &VehicleId::new("vehicle-a").unwrap()
            )
            .unwrap()
            .observed_ac,
        Energy::parse_kwh(if progress == 0 { "0" } else { "10" }).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn store_refuses_symlink_special_and_hardlinked_runtime_paths() {
    use std::os::unix::fs::symlink;
    let directory = RuntimeDirectory::new();
    let manifest = manifest(&[]);
    let original = directory.0.join("original.sqlite");
    fs::write(&original, b"synthetic-preserved-file").unwrap();
    symlink(&original, directory.database()).unwrap();
    assert!(matches!(
        Store::open(&directory.database(), &manifest, &source()),
        Err(IngestionError::StorageUnavailable)
    ));
    assert_eq!(fs::read(&original).unwrap(), b"synthetic-preserved-file");
    fs::remove_file(directory.database()).unwrap();
    fs::hard_link(&original, directory.database()).unwrap();
    assert!(matches!(
        Store::open(&directory.database(), &manifest, &source()),
        Err(IngestionError::StorageUnavailable)
    ));
    fs::remove_file(directory.database()).unwrap();
    fs::create_dir(directory.database()).unwrap();
    assert!(matches!(
        Store::open(&directory.database(), &manifest, &source()),
        Err(IngestionError::StorageUnavailable)
    ));
    fs::remove_dir(directory.database()).unwrap();
    let lock_path = directory.0.join("evidence.sqlite.lock");
    symlink(&original, &lock_path).unwrap();
    assert!(matches!(
        Store::open(&directory.database(), &manifest, &source()),
        Err(IngestionError::StorageUnavailable)
    ));
    assert!(!directory.database().exists());
    assert_eq!(fs::read(&original).unwrap(), b"synthetic-preserved-file");
}

fn crash_records() -> Vec<AcceptedEvidence> {
    vec![
        record(
            1,
            "crash-connection",
            NormalizedEventKind::Start,
            NormalizedChargeType::Ac,
            counter("0.000000"),
        ),
        record(
            2,
            "crash-connection",
            NormalizedEventKind::End,
            NormalizedChargeType::Ac,
            counter("10.000000"),
        ),
    ]
}

fn insert_crash_variant(connection: &Connection, id: i64, evidence: &AcceptedEvidence) {
    connection.execute("INSERT INTO event_variants(id, vehicle, position, connection, source_time, event_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, evidence.event.vehicle, evidence.event.position, evidence.event.connection, evidence.event.time, serde_json::to_string(&evidence.event).unwrap()]).unwrap();
}

#[cfg(unix)]
#[test]
fn runtime_parent_and_companion_privacy_fail_without_modifying_the_database() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = RuntimeDirectory::new();
    let manifest = manifest(&[]);
    let database = directory.database();
    drop(Store::open(&database, &manifest, &source()).unwrap());
    let original = fs::read(&database).unwrap();
    let alias = directory.0.join("parent-alias");
    symlink(&directory.0, &alias).unwrap();
    assert!(matches!(
        Store::open(&alias.join("evidence.sqlite"), &manifest, &source()),
        Err(IngestionError::StorageUnavailable)
    ));
    assert_eq!(fs::read(&database).unwrap(), original);
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        Store::open(&database, &manifest, &source()),
        Err(IngestionError::StorageUnavailable)
    ));
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read(&database).unwrap(), original);
    for name in ["evidence.sqlite", "evidence.sqlite.lock"] {
        let file = directory.0.join(name);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            Store::open(&database, &manifest, &source()),
            Err(IngestionError::StorageUnavailable)
        ));
        assert_eq!(fs::read(&database).unwrap(), original);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let target = directory.0.join("synthetic-companion-target");
    fs::write(&target, b"synthetic-preserved-companion").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    for suffix in ["-wal", "-shm", "-journal"] {
        let companion = directory.0.join(format!("evidence.sqlite{suffix}"));
        symlink(&target, &companion).unwrap();
        assert!(matches!(
            Store::open(&database, &manifest, &source()),
            Err(IngestionError::StorageUnavailable)
        ));
        assert_eq!(fs::read(&database).unwrap(), original);
        assert_eq!(fs::read(&target).unwrap(), b"synthetic-preserved-companion");
        fs::remove_file(&companion).unwrap();
        fs::hard_link(&target, &companion).unwrap();
        assert!(matches!(
            Store::open(&database, &manifest, &source()),
            Err(IngestionError::StorageUnavailable)
        ));
        assert_eq!(fs::read(&database).unwrap(), original);
        fs::remove_file(&companion).unwrap();
        fs::write(&companion, b"synthetic-permissive-companion").unwrap();
        fs::set_permissions(&companion, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            Store::open(&database, &manifest, &source()),
            Err(IngestionError::StorageUnavailable)
        ));
        assert_eq!(fs::read(&database).unwrap(), original);
        fs::remove_file(&companion).unwrap();
    }
    Store::open(&database, &manifest, &source()).unwrap();
}
