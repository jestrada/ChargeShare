use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rdkafka::client::ClientContext;
use rdkafka::config::{ClientConfig, RDKafkaLogLevel};
use rdkafka::consumer::{BaseConsumer, Consumer, ConsumerContext};
use rdkafka::error::{KafkaError, RDKafkaErrorCode};
use rdkafka::{Message, Offset, TopicPartitionList};

use crate::{DeliveryCoordinate, IngestionError, Manifest, PartitionRange, SourceBinding, Store};

const MAX_SOURCE_MARKER_BYTES: usize = 16_384;
const MAX_RECORDS: usize = 10_000;
const MAX_DEADLINE: Duration = Duration::from_secs(120);

pub struct KafkaConfig {
    pub bootstrap_servers: String,
    pub source_epoch_path: PathBuf,
    pub max_records: usize,
    pub deadline: Duration,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConsumeReport {
    pub committed_records: usize,
    pub duplicate_deliveries: usize,
    pub inserted_variants: usize,
}

struct SilentSyntheticContext;

impl ClientContext for SilentSyntheticContext {
    fn log(&self, _: RDKafkaLogLevel, _: &str, _: &str) {}

    fn error(&self, _: KafkaError, _: &str) {}

    fn stats_raw(&self, _: &[u8]) {}
}

impl ConsumerContext for SilentSyntheticContext {}

impl KafkaConfig {
    pub fn validate(&self) -> Result<(), IngestionError> {
        if self.max_records == 0
            || self.max_records > MAX_RECORDS
            || self.deadline.is_zero()
            || self.deadline > MAX_DEADLINE
            || self.bootstrap_servers.len() > 2_048
            || self
                .source_epoch_path
                .file_name()
                .and_then(|name| name.to_str())
                != Some("source-epoch.json")
        {
            return Err(IngestionError::InvalidConfiguration);
        }
        let endpoints: Vec<_> = self.bootstrap_servers.split(',').collect();
        if endpoints.is_empty() || endpoints.len() > 16 {
            return Err(IngestionError::InvalidConfiguration);
        }
        for endpoint in endpoints {
            if endpoint == "kafka:9092" {
                continue;
            }
            let address = endpoint
                .parse::<SocketAddr>()
                .map_err(|_| IngestionError::InvalidConfiguration)?;
            if !address.ip().is_loopback() || address.port() == 0 {
                return Err(IngestionError::InvalidConfiguration);
            }
        }
        Ok(())
    }
}

pub fn consume_bounded(
    config: &KafkaConfig,
    source: &SourceBinding,
    manifest: &Manifest,
    store: &mut Store,
) -> Result<ConsumeReport, IngestionError> {
    let deadline = Instant::now()
        .checked_add(config.deadline)
        .ok_or(IngestionError::InvalidConfiguration)?;
    config.validate()?;
    source.validate()?;
    if store.manifest.config() != manifest.config() {
        return Err(IngestionError::ConfigurationMismatch);
    }
    verify_source_marker(&config.source_epoch_path, source)?;
    if source.topic_id.is_some() {
        return Err(IngestionError::SourceMismatch);
    }
    let consumer: BaseConsumer<SilentSyntheticContext> = ClientConfig::new()
        .set("bootstrap.servers", &config.bootstrap_servers)
        .set("group.id", "chargeshare-synthetic-sqlite-progress")
        .set("client.id", "chargeshare-synthetic-ingestion")
        .set("enable.auto.commit", "false")
        .set("enable.auto.offset.store", "false")
        .set("auto.offset.reset", "error")
        .set("allow.auto.create.topics", "false")
        .set("enable.partition.eof", "false")
        .set("socket.timeout.ms", "1000")
        .set("socket.connection.setup.timeout.ms", "1000")
        .set("fetch.wait.max.ms", "100")
        .set("fetch.error.backoff.ms", "100")
        .set("reconnect.backoff.ms", "100")
        .set("reconnect.backoff.max.ms", "500")
        .set("statistics.interval.ms", "0")
        .set("log_level", "0")
        .create_with_context(SilentSyntheticContext)
        .map_err(|_| IngestionError::KafkaUnavailable)?;
    let result = consume_assigned(&consumer, config, source, manifest, store, deadline);
    let cleanup = consumer
        .unassign()
        .map_err(|_| IngestionError::KafkaUnavailable);
    match result {
        Err(error) => Err(error),
        Ok(report) => cleanup.map(|()| report),
    }
}

fn consume_assigned(
    consumer: &BaseConsumer<SilentSyntheticContext>,
    config: &KafkaConfig,
    source: &SourceBinding,
    manifest: &Manifest,
    store: &mut Store,
    deadline: Instant,
) -> Result<ConsumeReport, IngestionError> {
    let metadata = consumer
        .fetch_metadata(Some(&source.topic), remaining(deadline)?)
        .map_err(|_| source_operation_error(deadline))?;
    if metadata.brokers().is_empty()
        || metadata
            .brokers()
            .iter()
            .any(|broker| !permitted_broker(broker.host(), broker.port()))
    {
        return Err(IngestionError::SourceMismatch);
    }
    let topics: Vec<_> = metadata
        .topics()
        .iter()
        .filter(|topic| topic.name() == source.topic)
        .collect();
    if topics.len() != 1 || topics[0].error().is_some() {
        return Err(IngestionError::SourceMismatch);
    }
    let expected_partitions: BTreeSet<_> = source
        .partitions
        .iter()
        .map(|start| start.partition)
        .collect();
    let actual_partitions: BTreeSet<_> = topics[0]
        .partitions()
        .iter()
        .map(|partition| partition.id())
        .collect();
    if expected_partitions != actual_partitions
        || topics[0]
            .partitions()
            .iter()
            .any(|partition| partition.error().is_some())
    {
        return Err(IngestionError::SourceMismatch);
    }
    let mut observed = source.clone();
    observed.cluster_id = consumer.client().fetch_cluster_id(remaining(deadline)?);
    source.verify_identity(&observed)?;
    let mut bounds = Vec::new();
    for start in &source.partitions {
        let (low, high) = consumer
            .fetch_watermarks(&source.topic, start.partition, remaining(deadline)?)
            .map_err(|_| source_operation_error(deadline))?;
        bounds.push(PartitionRange {
            partition: start.partition,
            low,
            high,
        });
    }
    store.validate_source(&observed, &bounds)?;
    verify_source_marker(&config.source_epoch_path, source)?;
    let mut positions: BTreeMap<_, _> = store
        .positions()?
        .into_iter()
        .map(|position| (position.partition, position.next_offset))
        .collect();
    let mut assignment = TopicPartitionList::new();
    for (&partition, &next_offset) in &positions {
        assignment
            .add_partition_offset(&source.topic, partition, Offset::Offset(next_offset))
            .map_err(|_| IngestionError::KafkaUnavailable)?;
    }
    remaining(deadline)?;
    consumer
        .assign(&assignment)
        .map_err(|_| IngestionError::KafkaUnavailable)?;
    let mut report = ConsumeReport::default();
    while report.committed_records < config.max_records {
        let poll_timeout = remaining(deadline)?.min(Duration::from_millis(100));
        let Some(delivery) = consumer.poll(poll_timeout) else {
            verify_source_marker(&config.source_epoch_path, source)?;
            continue;
        };
        let delivery = delivery.map_err(consumer_error)?;
        if delivery.topic() != source.topic || delivery.offset() < 0 {
            return Err(IngestionError::SourceMismatch);
        }
        let partition = delivery.partition();
        let expected_resume_position = *positions
            .get(&partition)
            .ok_or(IngestionError::SourceMismatch)?;
        verify_source_marker(&config.source_epoch_path, source)?;
        remaining(deadline)?;
        let outcome = store.ingest(
            manifest,
            DeliveryCoordinate {
                partition,
                offset: delivery.offset(),
            },
            expected_resume_position,
            delivery.key(),
            delivery.payload().unwrap_or_default(),
        )?;
        positions.insert(partition, outcome.next_offset);
        report.committed_records += 1;
        report.duplicate_deliveries += usize::from(outcome.duplicate_delivery);
        report.inserted_variants += usize::from(outcome.inserted_variant);
        remaining(deadline)?;
    }
    Ok(report)
}

fn permitted_broker(host: &str, port: i32) -> bool {
    if !(1..=65_535).contains(&port) {
        return false;
    }
    (host == "kafka" && port == 9092) || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

pub fn read_source_marker(path: &Path) -> Result<SourceBinding, IngestionError> {
    if path.file_name().and_then(|name| name.to_str()) != Some("source-epoch.json") {
        return Err(IngestionError::SourceMismatch);
    }
    for ancestor in path.ancestors().skip(1) {
        if !ancestor.as_os_str().is_empty()
            && fs::symlink_metadata(ancestor)
                .map_err(|_| IngestionError::SourceMismatch)?
                .file_type()
                .is_symlink()
        {
            return Err(IngestionError::SourceMismatch);
        }
    }
    let parent = path.parent().ok_or(IngestionError::SourceMismatch)?;
    let parent_metadata =
        fs::symlink_metadata(parent).map_err(|_| IngestionError::SourceMismatch)?;
    if !parent_metadata.is_dir() {
        return Err(IngestionError::SourceMismatch);
    }
    validate_private_marker_metadata(&parent_metadata, false)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| IngestionError::SourceMismatch)?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_MARKER_BYTES as u64 {
        return Err(IngestionError::SourceMismatch);
    }
    validate_private_marker_metadata(&metadata, true)?;
    let file = open_source_marker(path)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| IngestionError::SourceMismatch)?;
    if !opened_metadata.is_file() || opened_metadata.len() > MAX_SOURCE_MARKER_BYTES as u64 {
        return Err(IngestionError::SourceMismatch);
    }
    validate_private_marker_metadata(&opened_metadata, true)?;
    verify_marker_file_identity(&metadata, &opened_metadata)?;
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_MARKER_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| IngestionError::SourceMismatch)?;
    if bytes.len() > MAX_SOURCE_MARKER_BYTES {
        return Err(IngestionError::SourceMismatch);
    }
    let final_metadata = fs::symlink_metadata(path).map_err(|_| IngestionError::SourceMismatch)?;
    validate_private_marker_metadata(&final_metadata, true)?;
    verify_marker_file_identity(&opened_metadata, &final_metadata)?;
    let source: SourceBinding =
        serde_json::from_slice(&bytes).map_err(|_| IngestionError::SourceMismatch)?;
    source.validate()?;
    Ok(source)
}

#[cfg(target_os = "linux")]
fn validate_private_marker_metadata(
    metadata: &fs::Metadata,
    regular_file: bool,
) -> Result<(), IngestionError> {
    use std::os::unix::fs::MetadataExt;

    let owner = rustix::process::geteuid().as_raw();
    if metadata.uid() != owner
        || metadata.mode() & 0o077 != 0
        || (regular_file && metadata.nlink() != 1)
    {
        return Err(IngestionError::SourceMismatch);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn validate_private_marker_metadata(_: &fs::Metadata, _: bool) -> Result<(), IngestionError> {
    Err(IngestionError::SourceMismatch)
}

#[cfg(target_os = "linux")]
fn verify_marker_file_identity(
    expected: &fs::Metadata,
    actual: &fs::Metadata,
) -> Result<(), IngestionError> {
    use std::os::unix::fs::MetadataExt;

    if expected.dev() != actual.dev() || expected.ino() != actual.ino() || !actual.is_file() {
        return Err(IngestionError::SourceMismatch);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn verify_marker_file_identity(_: &fs::Metadata, _: &fs::Metadata) -> Result<(), IngestionError> {
    Err(IngestionError::SourceMismatch)
}

#[cfg(target_os = "linux")]
fn open_source_marker(path: &Path) -> Result<fs::File, IngestionError> {
    use rustix::fs::{Mode, OFlags, open, openat};

    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let mut directory = fs::File::from(
        open(
            if path.is_absolute() { "/" } else { "." },
            directory_flags,
            Mode::empty(),
        )
        .map_err(|_| IngestionError::SourceMismatch)?,
    );
    for component in path
        .parent()
        .ok_or(IngestionError::SourceMismatch)?
        .components()
    {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                directory = fs::File::from(
                    openat(&directory, name, directory_flags, Mode::empty())
                        .map_err(|_| IngestionError::SourceMismatch)?,
                );
            }
            _ => return Err(IngestionError::SourceMismatch),
        }
    }
    validate_private_marker_metadata(
        &directory
            .metadata()
            .map_err(|_| IngestionError::SourceMismatch)?,
        false,
    )?;
    Ok(fs::File::from(
        openat(
            &directory,
            path.file_name().ok_or(IngestionError::SourceMismatch)?,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|_| IngestionError::SourceMismatch)?,
    ))
}

#[cfg(not(target_os = "linux"))]
fn open_source_marker(_: &Path) -> Result<fs::File, IngestionError> {
    Err(IngestionError::SourceMismatch)
}

fn verify_source_marker(path: &Path, source: &SourceBinding) -> Result<(), IngestionError> {
    source.verify_identity(&read_source_marker(path)?)
}

fn remaining(deadline: Instant) -> Result<Duration, IngestionError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(IngestionError::Timeout)
}

fn source_operation_error(deadline: Instant) -> IngestionError {
    if Instant::now() >= deadline {
        IngestionError::Timeout
    } else {
        IngestionError::KafkaUnavailable
    }
}

fn consumer_error(error: KafkaError) -> IngestionError {
    match error {
        KafkaError::MessageConsumption(
            RDKafkaErrorCode::OffsetOutOfRange | RDKafkaErrorCode::AutoOffsetReset,
        )
        | KafkaError::MessageConsumptionFatal(
            RDKafkaErrorCode::OffsetOutOfRange | RDKafkaErrorCode::AutoOffsetReset,
        ) => IngestionError::RecoveryGap,
        _ => IngestionError::KafkaUnavailable,
    }
}
