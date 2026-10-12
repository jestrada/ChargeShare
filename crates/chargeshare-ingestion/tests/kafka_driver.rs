#![cfg(target_os = "linux")]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use chargeshare_core::{OwnerId, VehicleId};
use chargeshare_ingestion::{
    CommitBoundary, DeliveryCoordinate, IngestionError, KafkaConfig, Manifest, PartitionStart,
    RereadRange, SYNTHETIC_TOPIC, SourceBinding, Store, consume_bounded, consume_bounded_observed,
    reread_bounded,
};
use rdkafka::client::ClientContext;
use rdkafka::config::{ClientConfig, RDKafkaLogLevel};
use rdkafka::error::KafkaError;
use rdkafka::mocking::MockCluster;
use rdkafka::producer::{BaseProducer, BaseRecord, DeliveryResult, Producer, ProducerContext};

static NEXT_RUNTIME: AtomicUsize = AtomicUsize::new(0);

struct QuietProducer;

impl ClientContext for QuietProducer {
    fn log(&self, _: RDKafkaLogLevel, _: &str, _: &str) {}

    fn error(&self, _: KafkaError, _: &str) {}
}

impl ProducerContext for QuietProducer {
    type DeliveryOpaque = ();

    fn delivery(&self, _: &DeliveryResult<'_>, _: ()) {}
}

struct Fixture {
    directory: PathBuf,
    manifest: Manifest,
    source: SourceBinding,
}

impl Fixture {
    fn new(cluster_id: Option<String>) -> Self {
        let crate_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture_parent = crate_directory
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target");
        match fs::create_dir(&fixture_parent) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => panic!("fixture parent could not be created"),
        }
        assert!(fs::symlink_metadata(&fixture_parent).unwrap().is_dir());
        let directory = fixture_parent.join(format!(
            "kafka-fixture-{}-{}",
            std::process::id(),
            NEXT_RUNTIME.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let directory = directory.canonicalize().unwrap();
        let fixture = Self {
            directory,
            manifest: Manifest::from_json(include_bytes!("fixtures/manifest-v1.json")).unwrap(),
            source: SourceBinding {
                epoch: "synthetic-mock-driver-v1".to_owned(),
                topic: SYNTHETIC_TOPIC.to_owned(),
                cluster_id,
                topic_id: None,
                partitions: vec![PartitionStart {
                    partition: 0,
                    initial_offset: 0,
                }],
            },
        };
        fixture.write_marker();
        fixture
    }

    fn marker(&self) -> PathBuf {
        self.directory.join("source-epoch.json")
    }

    fn database(&self) -> PathBuf {
        self.directory.join("ingestion.sqlite3")
    }

    fn write_marker(&self) {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(self.marker())
            .unwrap();
        file.write_all(&serde_json::to_vec(&self.source).unwrap())
            .unwrap();
        file.sync_all().unwrap();
    }

    fn config(&self, bootstrap_servers: String, max_records: usize) -> KafkaConfig {
        KafkaConfig {
            bootstrap_servers,
            source_epoch_path: self.marker(),
            max_records,
            deadline: Duration::from_secs(5),
        }
    }

    fn open(&self) -> Store {
        Store::open(&self.database(), &self.manifest, &self.source).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn producer(bootstrap_servers: &str) -> BaseProducer<QuietProducer> {
    ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("message.timeout.ms", "2000")
        .set("log_level", "0")
        .create_with_context(QuietProducer)
        .unwrap()
}

fn mock_fixture() -> (
    MockCluster<'static, rdkafka::producer::DefaultProducerContext>,
    Fixture,
) {
    let cluster = MockCluster::new(1).unwrap();
    cluster.create_topic(SYNTHETIC_TOPIC, 1, 1).unwrap();
    let producer = producer(&cluster.bootstrap_servers());
    producer
        .client()
        .fetch_metadata(Some(SYNTHETIC_TOPIC), Duration::from_secs(2))
        .unwrap();
    let cluster_id = producer.client().fetch_cluster_id(Duration::from_secs(2));
    assert!(cluster_id.is_some());
    let fixture = Fixture::new(cluster_id);
    (cluster, fixture)
}

fn records() -> Vec<(Vec<u8>, Vec<u8>)> {
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/candidate-v.json")).unwrap();
    fixture["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            (
                record["key"].as_str().unwrap().as_bytes().to_vec(),
                serde_json::to_vec(&record["payload"]).unwrap(),
            )
        })
        .collect()
}

fn publish(bootstrap_servers: &str, records: &[(Vec<u8>, Vec<u8>)]) {
    let producer = producer(bootstrap_servers);
    for (key, payload) in records {
        producer
            .send(
                BaseRecord::to(SYNTHETIC_TOPIC)
                    .partition(0)
                    .key(key)
                    .payload(payload),
            )
            .unwrap();
    }
    producer.flush(Duration::from_secs(3)).unwrap();
}

#[test]
fn focused_mock_transport_resumes_sqlite_after_forced_duplicate_and_reconnect() {
    let (cluster, fixture) = mock_fixture();
    let input = records();
    publish(&cluster.bootstrap_servers(), &input);
    let mut store = fixture.open();
    let first = consume_bounded(
        &fixture.config(cluster.bootstrap_servers(), 2),
        &fixture.source,
        &fixture.manifest,
        &mut store,
    )
    .unwrap();
    assert_eq!(first.committed_records, 2);
    assert_eq!(store.positions().unwrap()[0].next_offset, 2);
    let duplicate = store
        .ingest(
            &fixture.manifest,
            DeliveryCoordinate {
                partition: 0,
                offset: 0,
            },
            2,
            Some(&input[0].0),
            &input[0].1,
        )
        .unwrap();
    assert!(duplicate.duplicate_delivery);
    drop(store);
    let mut restarted = fixture.open();
    let remaining = consume_bounded(
        &fixture.config(cluster.bootstrap_servers(), 4),
        &fixture.source,
        &fixture.manifest,
        &mut restarted,
    )
    .unwrap();
    assert_eq!(remaining.committed_records, 4);
    assert_eq!(restarted.positions().unwrap()[0].next_offset, 6);
    assert_eq!(restarted.statistics().unwrap().accepted_deliveries, 6);
    assert_eq!(restarted.statistics().unwrap().event_variants, 6);
    let summary = restarted
        .summary(
            &OwnerId::new("owner-a").unwrap(),
            &VehicleId::new("vehicle-a").unwrap(),
        )
        .unwrap();
    assert_eq!(summary.observed_ac.to_string(), "0.500000 kWh");
    assert_eq!(summary.eligible_shared_charger.to_string(), "0.500000 kWh");
}

#[test]
fn focused_mock_transport_rejects_missing_or_mismatched_cluster_binding() {
    let (cluster, _) = mock_fixture();
    for cluster_id in [None, Some("synthetic-wrong-cluster".to_owned())] {
        let fixture = Fixture::new(cluster_id);
        let mut store = fixture.open();
        assert_eq!(
            consume_bounded(
                &fixture.config(cluster.bootstrap_servers(), 1),
                &fixture.source,
                &fixture.manifest,
                &mut store
            ),
            Err(IngestionError::SourceMismatch),
        );
        assert_eq!(store.positions().unwrap()[0].next_offset, 0);
    }
}

#[test]
fn changed_manifest_fails_before_broker_contact_or_consumption() {
    let fixture = Fixture::new(None);
    let mut store = fixture.open();
    let mut changed = fixture.manifest.config().clone();
    changed.mapping_version = "fictional-mapping-v2".to_owned();
    let manifest = Manifest::new(changed).unwrap();
    let start = Instant::now();
    assert_eq!(
        consume_bounded(
            &fixture.config("127.0.0.1:1".to_owned(), 1),
            &fixture.source,
            &manifest,
            &mut store,
        ),
        Err(IngestionError::ConfigurationMismatch),
    );
    assert!(start.elapsed() < Duration::from_millis(200));
    assert_eq!(store.positions().unwrap()[0].next_offset, 0);
}

#[test]
fn focused_mock_transport_commits_sanitized_poison_then_continues_to_valid_evidence() {
    let (cluster, fixture) = mock_fixture();
    let forbidden = "fictional-private-text-must-not-be-retained";
    let mut input = vec![(b"device-1".to_vec(), forbidden.as_bytes().to_vec())];
    input.push(records()[0].clone());
    publish(&cluster.bootstrap_servers(), &input);
    let mut store = fixture.open();
    let report = consume_bounded(
        &fixture.config(cluster.bootstrap_servers(), 2),
        &fixture.source,
        &fixture.manifest,
        &mut store,
    )
    .unwrap();
    assert_eq!(report.committed_records, 2);
    assert_eq!(store.positions().unwrap()[0].next_offset, 2);
    let statistics = store.statistics().unwrap();
    assert_eq!(statistics.accepted_deliveries, 1);
    assert_eq!(statistics.rejected_deliveries, 1);
    assert_eq!(statistics.event_variants, 1);
    drop(store);
    for entry in fs::read_dir(&fixture.directory).unwrap() {
        let path = entry.unwrap().path();
        let bytes = fs::read(path).unwrap();
        assert!(
            !bytes
                .windows(forbidden.len())
                .any(|window| window == forbidden.as_bytes())
        );
    }
}

#[test]
fn focused_mock_transport_rejects_retained_start_gap_without_reset() {
    let (cluster, mut fixture) = mock_fixture();
    fixture.source.partitions[0].initial_offset = 7;
    fixture.write_marker();
    let mut store = fixture.open();
    assert_eq!(
        consume_bounded(
            &fixture.config(cluster.bootstrap_servers(), 1),
            &fixture.source,
            &fixture.manifest,
            &mut store
        ),
        Err(IngestionError::RecoveryGap),
    );
    assert_eq!(store.positions().unwrap()[0].next_offset, 7);
    assert_eq!(store.statistics().unwrap().accepted_deliveries, 0);
}

#[test]
fn focused_mock_transport_rejects_partition_changes_and_unsupported_topic_uuid() {
    let (cluster, mut fixture) = mock_fixture();
    fixture.source.partitions.push(PartitionStart {
        partition: 1,
        initial_offset: 0,
    });
    fixture.write_marker();
    let mut store = fixture.open();
    assert_eq!(
        consume_bounded(
            &fixture.config(cluster.bootstrap_servers(), 1),
            &fixture.source,
            &fixture.manifest,
            &mut store
        ),
        Err(IngestionError::SourceMismatch),
    );
    drop(store);
    let mut topic_fixture = Fixture::new(fixture.source.cluster_id.clone());
    topic_fixture.source.topic_id = Some("synthetic-unsupported-topic-id".to_owned());
    topic_fixture.write_marker();
    let mut store = topic_fixture.open();
    assert_eq!(
        consume_bounded(
            &topic_fixture.config(cluster.bootstrap_servers(), 1),
            &topic_fixture.source,
            &topic_fixture.manifest,
            &mut store
        ),
        Err(IngestionError::SourceMismatch),
    );
    assert_eq!(store.positions().unwrap()[0].next_offset, 0);
}

#[test]
fn focused_mock_transport_observes_epoch_change_while_waiting() {
    let (cluster, fixture) = mock_fixture();
    let marker = fixture.marker();
    let mut changed = fixture.source.clone();
    changed.epoch = "synthetic-reset-v2".to_owned();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(350));
        fs::write(marker, serde_json::to_vec(&changed).unwrap()).unwrap();
    });
    let mut store = fixture.open();
    assert_eq!(
        consume_bounded(
            &fixture.config(cluster.bootstrap_servers(), 1),
            &fixture.source,
            &fixture.manifest,
            &mut store
        ),
        Err(IngestionError::SourceMismatch),
    );
    worker.join().unwrap();
    assert_eq!(store.positions().unwrap()[0].next_offset, 0);
}

#[test]
fn unavailable_broker_is_bounded_and_preserves_progress() {
    let fixture = Fixture::new(None);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let mut config = fixture.config(address, 1);
    config.deadline = Duration::from_millis(350);
    let mut store = fixture.open();
    let start = Instant::now();
    let result = consume_bounded(&config, &fixture.source, &fixture.manifest, &mut store);
    assert!(matches!(
        result,
        Err(IngestionError::Timeout | IngestionError::KafkaUnavailable)
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(store.positions().unwrap()[0].next_offset, 0);
}

#[test]
fn cli_at_committed_topic_end_fails_with_exact_deadline_and_preserves_scoped_replay() {
    let (cluster, fixture) = mock_fixture();
    publish(&cluster.bootstrap_servers(), &records());
    let runtime = fixture.directory.join(".local-runtime");
    fs::DirBuilder::new().mode(0o700).create(&runtime).unwrap();
    for (name, bytes) in [
        (
            "source-epoch.json",
            serde_json::to_vec(&fixture.source).unwrap(),
        ),
        (
            "manifest.json",
            include_bytes!("fixtures/manifest-v1.json").to_vec(),
        ),
        (
            "synthetic-runtime.json",
            br#"{"project_name":"chargeshare-local-0123456789ab","synthetic_only":true}"#.to_vec(),
        ),
    ] {
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(runtime.join(name))
            .unwrap()
            .write_all(&bytes)
            .unwrap();
    }
    let database = runtime.join("ingestion.sqlite3");
    let mut store = Store::open(&database, &fixture.manifest, &fixture.source).unwrap();
    let mut config = fixture.config(cluster.bootstrap_servers(), 6);
    config.source_epoch_path = runtime.join("source-epoch.json");
    consume_bounded(&config, &fixture.source, &fixture.manifest, &mut store).unwrap();
    let positions = store.positions().unwrap();
    let statistics = store.statistics().unwrap();
    let scopes = [
        (
            OwnerId::new("owner-a").unwrap(),
            VehicleId::new("vehicle-a").unwrap(),
        ),
        (
            OwnerId::new("owner-b").unwrap(),
            VehicleId::new("vehicle-b").unwrap(),
        ),
    ];
    let sessions: Vec<_> = scopes
        .iter()
        .map(|(owner, vehicle)| store.sessions(owner, vehicle).unwrap())
        .collect();
    assert_eq!(positions[0].next_offset, 6);
    drop(store);
    let start = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_chargeshare-ingestion"))
        .args([
            "consume",
            runtime.to_str().unwrap(),
            &cluster.bootstrap_servers(),
            "1",
            "1",
        ])
        .output()
        .unwrap();
    assert!(start.elapsed() >= Duration::from_millis(900));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"ingestion: bounded deadline exceeded\n");
    let recovered = Store::open(&database, &fixture.manifest, &fixture.source).unwrap();
    assert_eq!(recovered.positions().unwrap(), positions);
    assert_eq!(recovered.statistics().unwrap(), statistics);
    for ((owner, vehicle), before) in scopes.iter().zip(sessions) {
        assert_eq!(recovered.sessions(owner, vehicle).unwrap(), before);
    }
}

#[test]
fn missing_nonprivate_symlinked_or_hardlinked_epoch_markers_fail_before_broker_contact() {
    let fixture = Fixture::new(None);
    let mut store = fixture.open();
    let config = fixture.config("127.0.0.1:1".to_owned(), 1);
    fs::remove_file(fixture.marker()).unwrap();
    assert_eq!(
        consume_bounded(&config, &fixture.source, &fixture.manifest, &mut store),
        Err(IngestionError::SourceMismatch)
    );
    fixture.write_marker();
    fs::set_permissions(fixture.marker(), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        consume_bounded(&config, &fixture.source, &fixture.manifest, &mut store),
        Err(IngestionError::SourceMismatch)
    );
    fs::set_permissions(fixture.marker(), fs::Permissions::from_mode(0o600)).unwrap();
    let alternate = fixture.directory.join("alternate.json");
    fs::rename(fixture.marker(), &alternate).unwrap();
    symlink(&alternate, fixture.marker()).unwrap();
    assert_eq!(
        consume_bounded(&config, &fixture.source, &fixture.manifest, &mut store),
        Err(IngestionError::SourceMismatch)
    );
    fs::remove_file(fixture.marker()).unwrap();
    fs::hard_link(&alternate, fixture.marker()).unwrap();
    assert_eq!(
        consume_bounded(&config, &fixture.source, &fixture.manifest, &mut store),
        Err(IngestionError::SourceMismatch)
    );
    assert_eq!(store.positions().unwrap()[0].next_offset, 0);
}

#[test]
fn bounded_config_rejects_external_names_addresses_and_unbounded_limits() {
    let fixture = Fixture::new(None);
    for address in [
        "localhost:9092",
        "192.0.2.1:9092",
        "kafka:9093",
        "example.invalid:9092",
        "127.0.0.1:0",
        "",
        "127.0.0.1:9092,",
    ] {
        assert_eq!(
            fixture.config(address.to_owned(), 1).validate(),
            Err(IngestionError::InvalidConfiguration)
        );
    }
    for address in [
        "127.0.0.1:9092",
        "[::1]:9092",
        "kafka:9092",
        "127.0.0.1:9092,[::1]:9093",
    ] {
        fixture.config(address.to_owned(), 1).validate().unwrap();
    }
    assert_eq!(
        fixture.config("127.0.0.1:9092".to_owned(), 0).validate(),
        Err(IngestionError::InvalidConfiguration)
    );
    assert_eq!(
        fixture
            .config("127.0.0.1:9092".to_owned(), 10_001)
            .validate(),
        Err(IngestionError::InvalidConfiguration)
    );
    let mut config = fixture.config("127.0.0.1:9092".to_owned(), 1);
    config.deadline = Duration::from_secs(121);
    assert_eq!(config.validate(), Err(IngestionError::InvalidConfiguration));
}

#[test]
fn explicit_retained_reread_leaves_scoped_results_evidence_and_next_position_unchanged() {
    let (cluster, fixture) = mock_fixture();
    publish(&cluster.bootstrap_servers(), &records());
    let mut store = fixture.open();
    let config = fixture.config(cluster.bootstrap_servers(), 6);
    consume_bounded(&config, &fixture.source, &fixture.manifest, &mut store).unwrap();
    let before_positions = store.positions().unwrap();
    let before_statistics = store.statistics().unwrap();
    let owner = OwnerId::new("owner-a").unwrap();
    let vehicle = VehicleId::new("vehicle-a").unwrap();
    let before_summary = store.summary(&owner, &vehicle).unwrap();
    for _ in 0..2 {
        let report = reread_bounded(
            &config,
            &fixture.source,
            &fixture.manifest,
            &store,
            &[RereadRange {
                partition: 0,
                start_offset: 0,
                end_offset: 6,
            }],
        )
        .unwrap();
        assert_eq!(report.verified_records, 6);
        assert_eq!(store.positions().unwrap(), before_positions);
        assert_eq!(store.statistics().unwrap(), before_statistics);
        assert_eq!(store.summary(&owner, &vehicle).unwrap(), before_summary);
    }
}

#[test]
fn reread_refuses_uncommitted_invalid_duplicate_and_unbounded_ranges_before_contact() {
    let fixture = Fixture::new(None);
    let input = records();
    let mut store = fixture.open();
    store
        .ingest(
            &fixture.manifest,
            DeliveryCoordinate {
                partition: 0,
                offset: 0,
            },
            0,
            Some(&input[0].0),
            &input[0].1,
        )
        .unwrap();
    let config = fixture.config("127.0.0.1:1".to_owned(), 6);
    let valid = RereadRange {
        partition: 0,
        start_offset: 0,
        end_offset: 1,
    };
    for (ranges, reason) in [
        (vec![], IngestionError::InvalidConfiguration),
        (
            vec![RereadRange {
                end_offset: 0,
                ..valid
            }],
            IngestionError::InvalidConfiguration,
        ),
        (
            vec![RereadRange {
                start_offset: -1,
                ..valid
            }],
            IngestionError::InvalidConfiguration,
        ),
        (
            vec![RereadRange {
                end_offset: 2,
                ..valid
            }],
            IngestionError::ProgressConflict,
        ),
        (
            vec![RereadRange {
                end_offset: 7,
                ..valid
            }],
            IngestionError::InvalidConfiguration,
        ),
        (
            vec![RereadRange {
                partition: 1,
                ..valid
            }],
            IngestionError::SourceMismatch,
        ),
        (vec![valid, valid], IngestionError::InvalidConfiguration),
    ] {
        assert_eq!(
            reread_bounded(&config, &fixture.source, &fixture.manifest, &store, &ranges),
            Err(reason)
        );
        assert_eq!(store.positions().unwrap()[0].next_offset, 1);
        assert_eq!(store.statistics().unwrap().accepted_deliveries, 1);
    }
}

#[test]
fn reread_refuses_changed_committed_disposition_without_modifying_evidence() {
    let (cluster, fixture) = mock_fixture();
    let input = records();
    let mut store = fixture.open();
    store
        .ingest(
            &fixture.manifest,
            DeliveryCoordinate {
                partition: 0,
                offset: 0,
            },
            0,
            Some(&input[0].0),
            &input[0].1,
        )
        .unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(&input[0].1).unwrap();
    changed["data"][3]["value"]["stringValue"] = serde_json::json!("100.01");
    publish(
        &cluster.bootstrap_servers(),
        &[(input[0].0.clone(), serde_json::to_vec(&changed).unwrap())],
    );
    let before = store.statistics().unwrap();
    assert_eq!(
        reread_bounded(
            &fixture.config(cluster.bootstrap_servers(), 1),
            &fixture.source,
            &fixture.manifest,
            &store,
            &[RereadRange {
                partition: 0,
                start_offset: 0,
                end_offset: 1
            }]
        ),
        Err(IngestionError::DeliveryConflict)
    );
    assert_eq!(store.positions().unwrap()[0].next_offset, 1);
    assert_eq!(store.statistics().unwrap(), before);
}

#[test]
fn reread_preserves_committed_fixed_rejections_and_refuses_epoch_change() {
    let (cluster, mut fixture) = mock_fixture();
    let input = [(b"device-1".to_vec(), b"fictional-rejected-text".to_vec())];
    publish(&cluster.bootstrap_servers(), &input);
    let mut store = fixture.open();
    let config = fixture.config(cluster.bootstrap_servers(), 1);
    consume_bounded(&config, &fixture.source, &fixture.manifest, &mut store).unwrap();
    let range = [RereadRange {
        partition: 0,
        start_offset: 0,
        end_offset: 1,
    }];
    reread_bounded(&config, &fixture.source, &fixture.manifest, &store, &range).unwrap();
    assert_eq!(store.statistics().unwrap().rejected_deliveries, 1);
    fixture.source.epoch = "synthetic-changed-epoch".to_owned();
    fixture.write_marker();
    assert_eq!(
        reread_bounded(&config, &fixture.source, &fixture.manifest, &store, &range),
        Err(IngestionError::SourceMismatch)
    );
    assert_eq!(store.positions().unwrap()[0].next_offset, 1);
}

#[test]
fn observed_commit_failure_before_and_after_commit_recovers_atomic_old_or_new_state() {
    for (boundary, committed) in [
        (CommitBoundary::BeforeCommit, 0),
        (CommitBoundary::AfterCommit, 1),
    ] {
        let (cluster, fixture) = mock_fixture();
        publish(&cluster.bootstrap_servers(), &records());
        let mut store = fixture.open();
        assert_eq!(
            consume_bounded_observed(
                &fixture.config(cluster.bootstrap_servers(), 1),
                &fixture.source,
                &fixture.manifest,
                &mut store,
                &mut |observed| {
                    if observed == boundary {
                        Err(IngestionError::Timeout)
                    } else {
                        Ok(())
                    }
                }
            ),
            Err(IngestionError::Timeout)
        );
        drop(store);
        let recovered = fixture.open();
        assert_eq!(recovered.positions().unwrap()[0].next_offset, committed);
        assert_eq!(
            recovered.statistics().unwrap().accepted_deliveries,
            committed as u64
        );
        assert_eq!(
            recovered.statistics().unwrap().event_variants,
            committed as u64
        );
    }
}

struct RuntimeApi {
    child: std::process::Child,
    address: std::net::SocketAddr,
}

impl RuntimeApi {
    fn start(fixture: &Fixture, broker: &str) -> Self {
        let runtime = fixture.directory.join(".local-runtime");
        let fresh = !runtime.exists();
        if fresh {
            fs::DirBuilder::new().mode(0o700).create(&runtime).unwrap();
            for (name, bytes) in [
                (
                    "source-epoch.json",
                    serde_json::to_vec(&fixture.source).unwrap(),
                ),
                (
                    "manifest.json",
                    serde_json::to_vec(fixture.manifest.config()).unwrap(),
                ),
                (
                    "synthetic-runtime.json",
                    br#"{"project_name":"chargeshare-local-0123456789ab","synthetic_only":true}"#
                        .to_vec(),
                ),
            ] {
                OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(runtime.join(name))
                    .unwrap()
                    .write_all(&bytes)
                    .unwrap();
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let child = Command::new(env!("CARGO_BIN_EXE_chargeshare-ingestion"))
            .args([
                "serve",
                runtime.to_str().unwrap(),
                broker,
                &address.to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut api = Self { child, address };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                api.child.try_wait().unwrap().is_none(),
                "owning runtime exited before readiness"
            );
            if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(api.get("/api/poc/ready").0, 200);
        if fresh {
            api.wait_state("owner-a", "vehicle-a", "empty");
        }
        api
    }

    fn get(&self, path: &str) -> (u16, serde_json::Value) {
        http_get(self.address, path)
    }

    fn wait_state(&self, owner: &str, vehicle: &str, state: &str) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let (status, result) = self.get(&format!(
                "/api/poc/owners/{owner}/vehicles/{vehicle}/results"
            ));
            if status == 200 && result["state"] == state {
                return result;
            }
            assert!(
                Instant::now() < deadline,
                "scoped HTTP result never reached expected state"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_energy(&self, vehicle: &str, energy: &str) -> serde_json::Value {
        let owner = if vehicle == "vehicle-a" {
            "owner-a"
        } else {
            "owner-b"
        };
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let (status, result) = self.get(&format!(
                "/api/poc/owners/{owner}/vehicles/{vehicle}/results"
            ));
            if status == 200
                && matches!(result["state"].as_str(), Some("held" | "eligible"))
                && result["observed_ac_kwh"] == energy
                && matches!(
                    result["sessions"][0]["evidence_state"].as_str(),
                    Some("complete" | "complete_zero")
                )
            {
                return result;
            }
            assert!(
                Instant::now() < deadline,
                "scoped HTTP energy never reached expected result"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for RuntimeApi {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.child.wait().unwrap();
    }
}

fn http_get(address: std::net::SocketAddr, path: &str) -> (u16, serde_json::Value) {
    use std::io::Read;
    let mut stream =
        std::net::TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(headers.contains("Cache-Control: no-store"));
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

#[test]
fn owning_process_serves_committed_scopes_and_refuses_leakage_over_http() {
    let (cluster, fixture) = mock_fixture();
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    let empty = api.wait_state("owner-a", "vehicle-a", "empty");
    assert_eq!(empty["sessions"], serde_json::json!([]));
    publish(&cluster.bootstrap_servers(), &records());
    for (owner, vehicle, energy) in [
        ("owner-a", "vehicle-a", "0.500000"),
        ("owner-b", "vehicle-b", "0.250000"),
    ] {
        let result = api.wait_energy(vehicle, energy);
        assert_eq!(result["owner"], owner);
        assert_eq!(result["vehicle"], vehicle);
        assert_eq!(
            result["state"],
            if vehicle == "vehicle-a" {
                "eligible"
            } else {
                "held"
            }
        );
        assert_eq!(
            result["eligible_shared_charger_kwh"],
            if vehicle == "vehicle-a" {
                energy
            } else {
                "0.000000"
            }
        );
        assert_eq!(result["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(result["sessions"][0]["vehicle"], vehicle);
        assert_eq!(result["sessions"][0]["connection"], "connection-1");
        assert_eq!(
            result["sessions"][0]["classification"],
            if vehicle == "vehicle-a" {
                "SharedCharger"
            } else {
                "OtherCharger"
            }
        );
        assert_eq!(
            result["sessions"][0]["classification_reason"],
            if vehicle == "vehicle-a" {
                "InsideGeofence"
            } else {
                "OutsideGeofence"
            }
        );
        assert_eq!(
            result["sessions"][0]["exclusion_reasons"],
            if vehicle == "vehicle-a" {
                serde_json::json!([])
            } else {
                serde_json::json!(["OtherCharger"])
            }
        );
        assert!(
            result["evidence_boundary"]
                .as_str()
                .unwrap()
                .contains("physical accuracy unvalidated")
        );
        assert_eq!(
            result,
            api.get(&format!(
                "/api/poc/owners/{owner}/vehicles/{vehicle}/results"
            ))
            .1
        );
        let text = result.to_string();
        for forbidden in [
            "BatteryLevel",
            "vin",
            "VehicleName",
            "device-",
            "offset",
            "createdAt",
            "price",
            "owed",
            "kafka:9092",
            "sqlite3",
            "latitude",
            "longitude",
            "12.3456789",
            "45.6789123",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
    for path in [
        "/api/poc/owners/owner-a/vehicles/vehicle-b/results",
        "/api/poc/owners/unknown/vehicles/vehicle-a/results",
    ] {
        assert_eq!(
            api.get(path),
            (404, serde_json::json!({"error":"scope_refused"}))
        );
    }
    assert_eq!(
        api.get("/api/poc/owners/%00/vehicles/vehicle-a/results"),
        (400, serde_json::json!({"error":"scope_refused"}))
    );
    assert_eq!(api.get("/api/demo").0, 404);
    assert_eq!(api.get("/api/poc/results").0, 404);
}

#[test]
fn concurrent_http_reads_during_ingestion_are_coherent_and_revision_tracks_visible_change() {
    let (cluster, fixture) = mock_fixture();
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    let input = records();
    publish(&cluster.bootstrap_servers(), &input[..2]);
    let held = api.wait_state("owner-a", "vehicle-a", "held");
    assert_eq!(held["sessions"][0]["evidence_state"], "incomplete");
    let address = api.address;
    let readers: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(move || {
                for _ in 0..12 {
                    let (status, result) = http_get(
                        address,
                        "/api/poc/owners/owner-a/vehicles/vehicle-a/results",
                    );
                    assert_eq!(status, 200);
                    assert_eq!(
                        result["observed_ac_kwh"],
                        result["sessions"][0]["observed_ac_kwh"]
                    );
                    assert_eq!(
                        result["eligible_shared_charger_kwh"],
                        result["sessions"][0]["eligible_shared_charger_kwh"]
                    );
                    if result["sessions"][0]["evidence_state"] == "incomplete" {
                        assert_eq!(result["eligible_shared_charger_kwh"], "0.000000");
                    }
                    assert_eq!(result["sessions"][0]["vehicle"], "vehicle-a");
                }
            })
        })
        .collect();
    publish(&cluster.bootstrap_servers(), &input[2..]);
    for reader in readers {
        reader.join().unwrap();
    }
    let complete = api.wait_energy("vehicle-a", "0.500000");
    assert_ne!(held["revision"], complete["revision"]);
    publish(&cluster.bootstrap_servers(), &input);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(complete, api.wait_energy("vehicle-a", "0.500000"));
    let mut conflict: serde_json::Value = serde_json::from_slice(&input[2].1).unwrap();
    conflict["data"][3]["value"]["stringValue"] = "100.30".into();
    publish(
        &cluster.bootstrap_servers(),
        &[(input[2].0.clone(), serde_json::to_vec(&conflict).unwrap())],
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = api
            .get("/api/poc/owners/owner-a/vehicles/vehicle-a/results")
            .1;
        if result["sessions"][0]["quality_flags"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("ConflictingEvidence"))
        {
            assert_ne!(result["revision"], complete["revision"]);
            assert_eq!(result["eligible_shared_charger_kwh"], "0.000000");
            break;
        }
        assert!(Instant::now() < deadline);
    }
}

#[test]
fn complete_zero_evidence_is_distinct_from_empty_and_incomplete_http_results() {
    let (cluster, fixture) = mock_fixture();
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    let empty = api.wait_state("owner-a", "vehicle-a", "empty");
    let mut input = records();
    for (key, payload) in &mut input {
        let mut record: serde_json::Value = serde_json::from_slice(payload).unwrap();
        record["data"][3]["value"]["stringValue"] = if key == b"device-1" {
            "100.00"
        } else {
            "200.00"
        }
        .into();
        *payload = serde_json::to_vec(&record).unwrap();
    }
    publish(&cluster.bootstrap_servers(), &input);
    let zero = api.wait_energy("vehicle-a", "0.000000");
    assert_eq!(zero["sessions"][0]["evidence_state"], "complete_zero");
    assert_eq!(zero["state"], "eligible");
    assert_ne!(empty["revision"], zero["revision"]);
}

#[test]
fn killed_owning_process_recovers_identical_http_results_and_sqlite_lock() {
    let (cluster, fixture) = mock_fixture();
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    publish(&cluster.bootstrap_servers(), &records());
    let before = api.wait_energy("vehicle-a", "0.500000");
    drop(api);
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    assert_eq!(before, api.wait_energy("vehicle-a", "0.500000"));
    publish(&cluster.bootstrap_servers(), &records());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(before, api.wait_energy("vehicle-a", "0.500000"));
}

#[test]
fn replaced_database_becomes_safe_unavailable_and_never_cached_results() {
    let (cluster, fixture) = mock_fixture();
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    publish(&cluster.bootstrap_servers(), &records());
    api.wait_energy("vehicle-a", "0.500000");
    fs::rename(
        fixture.directory.join(".local-runtime/ingestion.sqlite3"),
        fixture.directory.join("retained.sqlite3"),
    )
    .unwrap();
    assert_eq!(
        api.get("/api/poc/owners/owner-a/vehicles/vehicle-a/results"),
        (503, serde_json::json!({"error":"results_unavailable"}))
    );
    assert_eq!(api.get("/api/poc/ready").0, 503);
}

#[test]
fn broker_interruption_turns_http_results_unavailable_without_losing_evidence() {
    let (cluster, fixture) = mock_fixture();
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    publish(&cluster.bootstrap_servers(), &records());
    let complete = api.wait_energy("vehicle-a", "0.500000");
    cluster.broker_down(1).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let (status, result) = api.get("/api/poc/owners/owner-a/vehicles/vehicle-a/results");
        if status == 503 {
            assert_eq!(result, serde_json::json!({"error":"results_unavailable"}));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(api);
    cluster.broker_up(1).unwrap();
    let store = Store::open(
        &fixture.directory.join(".local-runtime/ingestion.sqlite3"),
        &fixture.manifest,
        &fixture.source,
    )
    .unwrap();
    let recovered = store
        .results(
            &OwnerId::new("owner-a").unwrap(),
            &VehicleId::new("vehicle-a").unwrap(),
        )
        .unwrap();
    assert_eq!(serde_json::to_value(recovered).unwrap(), complete);
}

#[test]
fn http_methods_invalid_identifiers_and_malformed_requests_fail_without_echoing_input() {
    use std::io::Read;
    let (cluster, fixture) = mock_fixture();
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    for (request, status, reason) in [
        (
            "POST /api/poc/owners/owner-a/vehicles/vehicle-a/results HTTP/1.1\r\n\r\n",
            "405",
            "method_refused",
        ),
        (
            "GET /api/poc/owners/fictional-rejected-text!invalid/vehicles/vehicle-a/results HTTP/1.1\r\n\r\n",
            "400",
            "scope_refused",
        ),
        ("fictional-rejected-text\r\n\r\n", "400", "invalid_request"),
    ] {
        let mut stream = std::net::TcpStream::connect(api.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert_eq!(response.split_whitespace().nth(1).unwrap(), status);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response.split_once("\r\n\r\n").unwrap().1)
                .unwrap(),
            serde_json::json!({"error":reason})
        );
        assert!(!response.contains("fictional-rejected-text"));
    }
}

#[test]
fn live_source_refusal_and_competing_owner_preserve_the_committed_http_snapshot() {
    let (cluster, fixture) = mock_fixture();
    let api = RuntimeApi::start(&fixture, &cluster.bootstrap_servers());
    publish(&cluster.bootstrap_servers(), &records());
    let complete = api.wait_energy("vehicle-a", "0.500000");
    let runtime = fixture.directory.join(".local-runtime");
    let output = Command::new(env!("CARGO_BIN_EXE_chargeshare-ingestion"))
        .args([
            "serve",
            runtime.to_str().unwrap(),
            &cluster.bootstrap_servers(),
            "127.0.0.1:8788",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        output.stderr,
        b"storage: another writer owns this runtime\n"
    );
    assert!(output.stdout.is_empty());
    let mut changed = fixture.source.clone();
    changed.epoch = "synthetic-different-source".to_owned();
    fs::write(
        runtime.join("source-epoch.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let (status, result) = api.get("/api/poc/owners/owner-a/vehicles/vehicle-a/results");
        if status == 503 {
            assert_eq!(result, serde_json::json!({"error":"results_unavailable"}));
            break;
        }
        assert!(Instant::now() < deadline);
    }
    fs::write(
        runtime.join("source-epoch.json"),
        serde_json::to_vec(&fixture.source).unwrap(),
    )
    .unwrap();
    assert_eq!(api.get("/api/poc/ready").0, 503);
    drop(api);
    let store = Store::open(
        &runtime.join("ingestion.sqlite3"),
        &fixture.manifest,
        &fixture.source,
    )
    .unwrap();
    assert_eq!(store.positions().unwrap()[0].next_offset, 6);
    assert_eq!(
        serde_json::to_value(
            store
                .results(
                    &OwnerId::new("owner-a").unwrap(),
                    &VehicleId::new("vehicle-a").unwrap()
                )
                .unwrap()
        )
        .unwrap(),
        complete
    );
}
