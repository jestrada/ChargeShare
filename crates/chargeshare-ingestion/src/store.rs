use std::ffi::OsString;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, fstat, open, openat, statat};
use rustix::process::geteuid;

use crate::schema::{
    configure_durability, initialize, inspect_version, storage_error, verify_configuration,
};
use crate::{
    AcceptedEvidence, DeliveryCoordinate, IngestionError, Manifest, PartitionRange,
    RejectionReason, SourceBinding, normalize,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordDisposition {
    Accepted(Box<AcceptedEvidence>),
    Rejected(RejectionReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PartitionPosition {
    pub partition: i32,
    pub next_offset: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitOutcome {
    pub duplicate_delivery: bool,
    pub inserted_variant: bool,
    pub next_offset: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreStatistics {
    pub accepted_deliveries: u64,
    pub rejected_deliveries: u64,
    pub event_variants: u64,
}

pub struct Store {
    pub(crate) connection: Connection,
    pub(crate) manifest: Manifest,
    source: SourceBinding,
    manifest_json: String,
    _owning_lock: OwningLock,
    _runtime_directory: File,
    _database_guard: File,
}

impl Store {
    pub fn open(
        path: &Path,
        manifest: &Manifest,
        source: &SourceBinding,
    ) -> Result<Self, IngestionError> {
        source.validate()?;
        let mut source = source.clone();
        source.partitions.sort_by_key(|start| start.partition);
        let manifest_json = serde_json::to_string(manifest.config())
            .map_err(|_| IngestionError::InvalidConfiguration)?;
        let source_json =
            serde_json::to_string(&source).map_err(|_| IngestionError::InvalidConfiguration)?;
        let runtime = RuntimePath::open(path)?;
        runtime.validate_files()?;
        let owning_lock = runtime.open_private_file(&runtime.companion_name(".lock"))?;
        let owning_lock = OwningLock::acquire(owning_lock)?;
        let database_guard = runtime.open_private_file(&runtime.database_name)?;
        let preflight = Connection::open_with_flags(
            &runtime.absolute_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(storage_error)?;
        preflight
            .busy_timeout(Duration::from_millis(250))
            .map_err(storage_error)?;
        let fresh = inspect_version(&preflight)?;
        if !fresh {
            verify_configuration(&preflight, manifest, &manifest_json, &source, &source_json)?;
            verify_retained_evidence(&preflight, manifest)?;
        }
        drop(preflight);
        let mut connection = Connection::open_with_flags(
            &runtime.absolute_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(storage_error)?;
        runtime.verify_file_binding(&runtime.database_name, &database_guard)?;
        runtime.verify_file_binding(&runtime.companion_name(".lock"), &owning_lock.file)?;
        connection
            .busy_timeout(Duration::from_millis(250))
            .map_err(storage_error)?;
        let fresh = inspect_version(&connection)?;
        if !fresh {
            verify_configuration(&connection, manifest, &manifest_json, &source, &source_json)?;
        }
        configure_durability(&connection)?;
        if fresh {
            initialize(
                &mut connection,
                manifest,
                &manifest_json,
                &source,
                &source_json,
            )?;
        }
        runtime.validate_files()?;
        let store = Self {
            connection,
            manifest: manifest.clone(),
            source,
            manifest_json,
            _owning_lock: owning_lock,
            _runtime_directory: runtime.directory,
            _database_guard: database_guard,
        };
        verify_retained_evidence(&store.connection, &store.manifest)?;
        Ok(store)
    }

    pub fn ingest(
        &mut self,
        manifest: &Manifest,
        coordinate: DeliveryCoordinate,
        expected_resume_position: i64,
        key: Option<&[u8]>,
        payload: &[u8],
    ) -> Result<CommitOutcome, IngestionError> {
        let manifest_json = serde_json::to_string(manifest.config())
            .map_err(|_| IngestionError::InvalidConfiguration)?;
        if manifest_json != self.manifest_json {
            return Err(IngestionError::ConfigurationMismatch);
        }
        let disposition = match normalize(manifest, key, payload) {
            Ok(evidence) => RecordDisposition::Accepted(Box::new(evidence)),
            Err(reason) => RecordDisposition::Rejected(reason),
        };
        self.commit_record(coordinate, expected_resume_position, disposition)
    }

    pub fn commit_record(
        &mut self,
        coordinate: DeliveryCoordinate,
        expected_resume_position: i64,
        disposition: RecordDisposition,
    ) -> Result<CommitOutcome, IngestionError> {
        if coordinate.partition < 0 || coordinate.offset < 0 || expected_resume_position < 0 {
            return Err(IngestionError::ProgressConflict);
        }
        let next_offset = coordinate
            .offset
            .checked_add(1)
            .ok_or(IngestionError::ProgressConflict)?;
        let prepared = PreparedDisposition::new(&self.manifest, disposition)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let current: i64 = transaction
            .query_row(
                "SELECT next_offset FROM partition_progress WHERE partition = ?1",
                [coordinate.partition],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(IngestionError::SourceMismatch)?;
        let existing: Option<(Option<String>, Option<String>, Option<String>)> = transaction
            .query_row(
                "SELECT v.event_json, d.provenance_json, d.rejection_reason
                 FROM deliveries d LEFT JOIN event_variants v ON v.id = d.variant_id
                 WHERE d.partition = ?1 AND d.offset = ?2",
                params![coordinate.partition, coordinate.offset],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some(stored) = existing {
            if stored != prepared.identity() {
                return Err(IngestionError::DeliveryConflict);
            }
            return Ok(CommitOutcome {
                duplicate_delivery: true,
                inserted_variant: false,
                next_offset: current,
            });
        }
        if current != expected_resume_position || coordinate.offset < current {
            return Err(IngestionError::ProgressConflict);
        }
        let (variant_id, inserted_variant) = match &prepared {
            PreparedDisposition::Accepted {
                evidence,
                event_json,
                ..
            } => {
                let inserted = transaction
                    .execute(
                        "INSERT INTO event_variants(vehicle, position, connection, source_time, event_json)
                         VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(event_json) DO NOTHING",
                        params![evidence.event.vehicle, evidence.event.position, evidence.event.connection, evidence.event.time, event_json],
                    )
                    .map_err(storage_error)?;
                let id: i64 = transaction
                    .query_row(
                        "SELECT id FROM event_variants WHERE event_json = ?1",
                        [event_json],
                        |row| row.get(0),
                    )
                    .map_err(storage_error)?;
                (Some(id), inserted == 1)
            }
            PreparedDisposition::Rejected { .. } => (None, false),
        };
        let (_, provenance_json, rejection_reason) = prepared.identity();
        transaction
            .execute(
                "INSERT INTO deliveries(partition, offset, variant_id, provenance_json, rejection_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![coordinate.partition, coordinate.offset, variant_id, provenance_json, rejection_reason],
            )
            .map_err(storage_error)?;
        let changed = transaction
            .execute(
                "UPDATE partition_progress SET next_offset = ?1 WHERE partition = ?2 AND next_offset = ?3",
                params![next_offset, coordinate.partition, expected_resume_position],
            )
            .map_err(storage_error)?;
        if changed != 1 {
            return Err(IngestionError::ProgressConflict);
        }
        transaction.commit().map_err(storage_error)?;
        Ok(CommitOutcome {
            duplicate_delivery: false,
            inserted_variant,
            next_offset,
        })
    }

    pub fn positions(&self) -> Result<Vec<PartitionPosition>, IngestionError> {
        self.connection
            .prepare("SELECT partition, next_offset FROM partition_progress ORDER BY partition")
            .map_err(storage_error)?
            .query_map([], |row| {
                Ok(PartitionPosition {
                    partition: row.get(0)?,
                    next_offset: row.get(1)?,
                })
            })
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)
    }

    pub fn validate_source(
        &self,
        observed: &SourceBinding,
        bounds: &[PartitionRange],
    ) -> Result<(), IngestionError> {
        let mut observed = observed.clone();
        observed.partitions.sort_by_key(|start| start.partition);
        self.source.verify_identity(&observed)?;
        if bounds.len() != self.source.partitions.len() {
            return Err(IngestionError::RecoveryGap);
        }
        for position in self.positions()? {
            self.source
                .validate_position(position.partition, position.next_offset, bounds)?;
        }
        Ok(())
    }

    pub fn statistics(&self) -> Result<StoreStatistics, IngestionError> {
        let counts: (i64, i64, i64) = self
            .connection
            .query_row(
                "SELECT
                    (SELECT count(*) FROM deliveries WHERE variant_id IS NOT NULL),
                    (SELECT count(*) FROM deliveries WHERE rejection_reason IS NOT NULL),
                    (SELECT count(*) FROM event_variants)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(storage_error)?;
        Ok(StoreStatistics {
            accepted_deliveries: u64::try_from(counts.0)
                .map_err(|_| IngestionError::StorageCorrupt)?,
            rejected_deliveries: u64::try_from(counts.1)
                .map_err(|_| IngestionError::StorageCorrupt)?,
            event_variants: u64::try_from(counts.2).map_err(|_| IngestionError::StorageCorrupt)?,
        })
    }
}

fn verify_retained_evidence(
    connection: &Connection,
    manifest: &Manifest,
) -> Result<(), IngestionError> {
    let orphaned = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM event_variants v WHERE NOT EXISTS(
                    SELECT 1 FROM deliveries d WHERE d.variant_id = v.id))",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(storage_error)?;
    if orphaned {
        return Err(IngestionError::StorageCorrupt);
    }
    let mut statement = connection
        .prepare(
            "SELECT v.vehicle, v.position, v.connection, v.source_time, v.event_json,
                d.provenance_json, d.rejection_reason FROM deliveries d
                LEFT JOIN event_variants v ON v.id = d.variant_id ORDER BY d.partition, d.offset",
        )
        .map_err(storage_error)?;
    let mut rows = statement.query([]).map_err(storage_error)?;
    while let Some(row) = rows.next().map_err(storage_error)? {
        let event_json: Option<String> = row.get(4).map_err(storage_error)?;
        let provenance_json: Option<String> = row.get(5).map_err(storage_error)?;
        let rejection_json: Option<String> = row.get(6).map_err(storage_error)?;
        match (event_json, provenance_json, rejection_json) {
            (Some(event_json), Some(provenance_json), None) => {
                let evidence = AcceptedEvidence {
                    event: serde_json::from_str(&event_json)
                        .map_err(|_| IngestionError::StorageCorrupt)?,
                    provenance: serde_json::from_str(&provenance_json)
                        .map_err(|_| IngestionError::StorageCorrupt)?,
                };
                manifest
                    .validate_evidence(&evidence)
                    .map_err(|_| IngestionError::StorageCorrupt)?;
                let prepared = PreparedDisposition::new(
                    manifest,
                    RecordDisposition::Accepted(Box::new(evidence.clone())),
                )?;
                if prepared.identity() != (Some(event_json), Some(provenance_json), None)
                    || row.get::<_, String>(0).map_err(storage_error)? != evidence.event.vehicle
                    || row.get::<_, String>(1).map_err(storage_error)? != evidence.event.position
                    || row.get::<_, String>(2).map_err(storage_error)? != evidence.event.connection
                    || row.get::<_, i64>(3).map_err(storage_error)? != evidence.event.time
                {
                    return Err(IngestionError::StorageCorrupt);
                }
            }
            (None, None, Some(reason_json)) => {
                let reason: RejectionReason = serde_json::from_str(&reason_json)
                    .map_err(|_| IngestionError::StorageCorrupt)?;
                if serde_json::to_string(&reason).map_err(|_| IngestionError::StorageCorrupt)?
                    != reason_json
                {
                    return Err(IngestionError::StorageCorrupt);
                }
            }
            _ => return Err(IngestionError::StorageCorrupt),
        }
    }
    Ok(())
}

struct OwningLock {
    file: File,
}

impl OwningLock {
    fn acquire(file: File) -> Result<Self, IngestionError> {
        FileExt::try_lock_exclusive(&file).map_err(|_| IngestionError::OwnershipConflict)?;
        Ok(Self { file })
    }
}

impl Drop for OwningLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

struct RuntimePath {
    directory: File,
    absolute_path: PathBuf,
    database_name: OsString,
}

impl RuntimePath {
    fn open(path: &Path) -> Result<Self, IngestionError> {
        if path.as_os_str().is_empty() || path == Path::new(":memory:") {
            return Err(IngestionError::InvalidConfiguration);
        }
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| IngestionError::StorageUnavailable)?
                .join(path)
        };
        let database_name = absolute_path
            .file_name()
            .ok_or(IngestionError::InvalidConfiguration)?
            .to_os_string();
        let parent = absolute_path
            .parent()
            .ok_or(IngestionError::InvalidConfiguration)?;
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut directory =
            open("/", flags, Mode::empty()).map_err(|_| IngestionError::StorageUnavailable)?;
        validate_trusted_ancestor(
            &fstat(&directory).map_err(|_| IngestionError::StorageUnavailable)?,
        )?;
        for component in parent.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => {
                    directory = openat(&directory, name, flags, Mode::empty())
                        .map_err(|_| IngestionError::StorageUnavailable)?;
                    validate_trusted_ancestor(
                        &fstat(&directory).map_err(|_| IngestionError::StorageUnavailable)?,
                    )?;
                }
                _ => return Err(IngestionError::InvalidConfiguration),
            }
        }
        let metadata = fstat(&directory).map_err(|_| IngestionError::StorageUnavailable)?;
        if metadata.st_uid != geteuid().as_raw() || metadata.st_mode & 0o077 != 0 {
            return Err(IngestionError::StorageUnavailable);
        }
        Ok(Self {
            directory: File::from(directory),
            absolute_path,
            database_name,
        })
    }

    fn companion_name(&self, suffix: &str) -> OsString {
        let mut name = self.database_name.clone();
        name.push(suffix);
        name
    }

    fn validate_files(&self) -> Result<(), IngestionError> {
        for name in [
            self.database_name.clone(),
            self.companion_name(".lock"),
            self.companion_name("-wal"),
            self.companion_name("-shm"),
            self.companion_name("-journal"),
        ] {
            match statat(&self.directory, &name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(metadata) => validate_private_file(&metadata)?,
                Err(error) if error == rustix::io::Errno::NOENT => {}
                Err(_) => return Err(IngestionError::StorageUnavailable),
            }
        }
        Ok(())
    }

    fn open_private_file(&self, name: &OsString) -> Result<File, IngestionError> {
        let descriptor = openat(
            &self.directory,
            name,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|_| IngestionError::StorageUnavailable)?;
        validate_private_file(
            &fstat(&descriptor).map_err(|_| IngestionError::StorageUnavailable)?,
        )?;
        let file = File::from(descriptor);
        self.verify_file_binding(name, &file)?;
        Ok(file)
    }

    fn verify_file_binding(&self, name: &OsString, file: &File) -> Result<(), IngestionError> {
        let opened = fstat(file).map_err(|_| IngestionError::StorageUnavailable)?;
        let named = statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| IngestionError::StorageUnavailable)?;
        validate_private_file(&named)?;
        if opened.st_dev != named.st_dev || opened.st_ino != named.st_ino {
            return Err(IngestionError::StorageUnavailable);
        }
        Ok(())
    }
}

fn validate_trusted_ancestor(metadata: &Stat) -> Result<(), IngestionError> {
    if FileType::from_raw_mode(metadata.st_mode) != FileType::Directory
        || (metadata.st_uid != 0 && metadata.st_uid != geteuid().as_raw())
        || (metadata.st_mode & 0o022 != 0 && metadata.st_mode & 0o1000 == 0)
    {
        return Err(IngestionError::StorageUnavailable);
    }
    Ok(())
}

fn validate_private_file(metadata: &Stat) -> Result<(), IngestionError> {
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
        || metadata.st_nlink != 1
        || metadata.st_uid != geteuid().as_raw()
        || metadata.st_mode & 0o777 != 0o600
    {
        return Err(IngestionError::StorageUnavailable);
    }
    Ok(())
}

enum PreparedDisposition {
    Accepted {
        evidence: Box<AcceptedEvidence>,
        event_json: String,
        provenance_json: String,
    },
    Rejected {
        reason_json: String,
    },
}

impl PreparedDisposition {
    fn new(manifest: &Manifest, disposition: RecordDisposition) -> Result<Self, IngestionError> {
        match disposition {
            RecordDisposition::Accepted(evidence) => {
                manifest
                    .validate_evidence(&evidence)
                    .map_err(|_| IngestionError::InvalidConfiguration)?;
                let event_json = serde_json::to_string(&evidence.event)
                    .map_err(|_| IngestionError::InvalidConfiguration)?;
                let provenance_json = serde_json::to_string(&evidence.provenance)
                    .map_err(|_| IngestionError::InvalidConfiguration)?;
                Ok(Self::Accepted {
                    evidence,
                    event_json,
                    provenance_json,
                })
            }
            RecordDisposition::Rejected(reason) => {
                let reason_json = serde_json::to_string(&reason)
                    .map_err(|_| IngestionError::InvalidConfiguration)?;
                Ok(Self::Rejected { reason_json })
            }
        }
    }

    fn identity(&self) -> (Option<String>, Option<String>, Option<String>) {
        match self {
            Self::Accepted {
                event_json,
                provenance_json,
                ..
            } => (
                Some(event_json.clone()),
                Some(provenance_json.clone()),
                None,
            ),
            Self::Rejected { reason_json } => (None, None, Some(reason_json.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RecordDisposition, Store};
    use crate::{
        DeliveryCoordinate, DeviceMapping, INPUT_CONTRACT_VERSION, IngestionError, Manifest,
        ManifestConfig, NORMALIZER_VERSION, PartitionStart, RejectionReason, SYNTHETIC_TOPIC,
        SourceBinding, TIMESTAMP_VERSION,
    };
    use std::fs;

    #[test]
    fn explicit_owner_release_unlocks_an_inherited_open_file_description() {
        use super::{OwningLock, RuntimePath};
        use fs2::FileExt;
        let directory = std::env::temp_dir().join(format!(
            "chargeshare-synthetic-inherited-lock-{}",
            std::process::id()
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory).unwrap();
        let runtime = RuntimePath::open(&directory.join("evidence.sqlite")).unwrap();
        let name = runtime.companion_name(".lock");
        let owning_lock = OwningLock::acquire(runtime.open_private_file(&name).unwrap()).unwrap();
        let inherited = owning_lock.file.try_clone().unwrap();
        let contender = runtime.open_private_file(&name).unwrap();
        assert!(FileExt::try_lock_exclusive(&contender).is_err());
        drop(owning_lock);
        FileExt::try_lock_exclusive(&contender).unwrap();
        FileExt::unlock(&contender).unwrap();
        drop(contender);
        drop(inherited);
        drop(runtime);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn private_file_owner_mismatch_and_post_validation_symlink_swaps_fail_closed() {
        use super::{RuntimePath, validate_private_file};
        use rustix::fs::fstat;
        let directory = std::env::temp_dir().join(format!(
            "chargeshare-synthetic-file-guard-{}",
            std::process::id()
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory).unwrap();
        let runtime = RuntimePath::open(&directory.join("evidence.sqlite")).unwrap();
        let file = runtime.open_private_file(&runtime.database_name).unwrap();
        let mut metadata = fstat(&file).unwrap();
        metadata.st_uid = metadata.st_uid.wrapping_add(1);
        assert_eq!(
            validate_private_file(&metadata),
            Err(IngestionError::StorageUnavailable)
        );
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            for name in [
                runtime.database_name.clone(),
                runtime.companion_name(".lock"),
            ] {
                let file = runtime.open_private_file(&name).unwrap();
                drop(file);
                runtime.validate_files().unwrap();
                let path = directory.join(&name);
                let original = directory.join(format!("{}.preserved", name.to_string_lossy()));
                fs::rename(&path, &original).unwrap();
                symlink(&original, &path).unwrap();
                assert!(matches!(
                    runtime.open_private_file(&name),
                    Err(IngestionError::StorageUnavailable)
                ));
                assert!(
                    fs::symlink_metadata(&path)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
                fs::remove_file(&path).unwrap();
                fs::rename(&original, &path).unwrap();
            }
        }
        drop(runtime);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn sqlite_full_never_advances_past_the_last_atomic_disposition() {
        let directory =
            std::env::temp_dir().join(format!("chargeshare-synthetic-full-{}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory).unwrap();
        let path = directory.join("evidence.sqlite");
        let manifest = Manifest::new(ManifestConfig {
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
            annotations: Vec::new(),
        })
        .unwrap();
        let source = SourceBinding {
            epoch: "synthetic-full-test".to_owned(),
            topic: SYNTHETIC_TOPIC.to_owned(),
            cluster_id: None,
            topic_id: None,
            partitions: vec![PartitionStart {
                partition: 0,
                initial_offset: 0,
            }],
        };
        let mut store = Store::open(&path, &manifest, &source).unwrap();
        let pages: i64 = store
            .connection
            .pragma_query_value(None, "page_count", |row| row.get(0))
            .unwrap();
        store
            .connection
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        let mut committed = 0;
        loop {
            assert!(committed < 2_000);
            match store.commit_record(
                DeliveryCoordinate {
                    partition: 0,
                    offset: committed,
                },
                committed,
                RecordDisposition::Rejected(RejectionReason::UnknownIdentity),
            ) {
                Ok(_) => committed += 1,
                Err(error) => {
                    assert_eq!(error, IngestionError::StorageUnavailable);
                    break;
                }
            }
        }
        assert_eq!(store.positions().unwrap()[0].next_offset, committed);
        assert_eq!(
            store.statistics().unwrap().rejected_deliveries,
            committed as u64
        );
        drop(store);
        let store = Store::open(&path, &manifest, &source).unwrap();
        assert_eq!(store.positions().unwrap()[0].next_offset, committed);
        assert_eq!(
            store.statistics().unwrap().rejected_deliveries,
            committed as u64
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
}
