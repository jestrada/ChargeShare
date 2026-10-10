#![cfg(target_os = "linux")]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use chargeshare_core::{OwnerId, VehicleId};
use chargeshare_ingestion::{
    DeliveryCoordinate, IngestionError, KafkaConfig, Manifest, PartitionStart, SYNTHETIC_TOPIC,
    SourceBinding, Store, consume_bounded,
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
    assert_eq!(summary.eligible_shared_charger.to_string(), "0.000000 kWh");
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
