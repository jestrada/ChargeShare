use rusqlite::{Connection, ErrorCode, TransactionBehavior, params};

use crate::{IngestionError, Manifest, SourceBinding};

pub(crate) const SCHEMA_VERSION: i64 = 1;
pub(crate) const APPLICATION_ID: i64 = 0x4353_494e;

pub(crate) fn storage_error(error: rusqlite::Error) -> IngestionError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase
            ) =>
        {
            IngestionError::StorageCorrupt
        }
        _ => IngestionError::StorageUnavailable,
    }
}

pub(crate) fn inspect_version(connection: &Connection) -> Result<bool, IngestionError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    let application: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(storage_error)?;
    let objects: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if version == 0 && application == 0 && objects == 0 {
        return Ok(true);
    }
    if version != SCHEMA_VERSION || application != APPLICATION_ID {
        return Err(IngestionError::SchemaMismatch);
    }
    Ok(false)
}

pub(crate) fn configure_durability(connection: &Connection) -> Result<(), IngestionError> {
    let mode: String = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(storage_error)?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(storage_error)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(storage_error)?;
    let synchronous: i64 = connection
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .map_err(storage_error)?;
    let foreign_keys: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(storage_error)?;
    let busy_timeout: i64 = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .map_err(storage_error)?;
    if mode != "wal" || synchronous != 2 || foreign_keys != 1 || busy_timeout != 250 {
        return Err(IngestionError::StorageUnavailable);
    }
    Ok(())
}

pub(crate) fn initialize(
    connection: &mut Connection,
    manifest: &Manifest,
    manifest_json: &str,
    source: &SourceBinding,
    source_json: &str,
) -> Result<(), IngestionError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    transaction
        .execute_batch(
            "CREATE TABLE runtime_metadata (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                schema_version INTEGER NOT NULL CHECK (schema_version = 1),
                manifest_json TEXT NOT NULL,
                source_json TEXT NOT NULL
            ) STRICT;
            CREATE TABLE registrations (
                device TEXT PRIMARY KEY,
                vehicle TEXT NOT NULL UNIQUE,
                owner TEXT NOT NULL
            ) STRICT;
            CREATE TABLE partition_progress (
                partition INTEGER PRIMARY KEY CHECK (partition >= 0),
                initial_offset INTEGER NOT NULL CHECK (initial_offset >= 0),
                next_offset INTEGER NOT NULL CHECK (next_offset >= initial_offset)
            ) STRICT;
            CREATE TABLE event_variants (
                id INTEGER PRIMARY KEY,
                vehicle TEXT NOT NULL REFERENCES registrations(vehicle),
                position TEXT NOT NULL CHECK (
                    length(position) BETWEEN 1 AND 20
                    AND position NOT GLOB '*[^0-9]*'
                    AND (position = '0' OR substr(position, 1, 1) != '0')
                ),
                connection TEXT NOT NULL,
                source_time INTEGER NOT NULL,
                event_json TEXT NOT NULL UNIQUE
            ) STRICT;
            CREATE INDEX variants_by_vehicle_position ON event_variants(vehicle, position);
            CREATE TABLE deliveries (
                partition INTEGER NOT NULL REFERENCES partition_progress(partition),
                offset INTEGER NOT NULL CHECK (offset >= 0),
                variant_id INTEGER REFERENCES event_variants(id),
                provenance_json TEXT,
                rejection_reason TEXT,
                PRIMARY KEY (partition, offset),
                CHECK (
                    (variant_id IS NOT NULL AND provenance_json IS NOT NULL AND rejection_reason IS NULL)
                    OR (variant_id IS NULL AND provenance_json IS NULL AND rejection_reason IS NOT NULL)
                )
            ) STRICT;
            PRAGMA user_version = 1;
            PRAGMA application_id = 1129531726;",
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO runtime_metadata VALUES (1, ?1, ?2, ?3)",
            params![SCHEMA_VERSION, manifest_json, source_json],
        )
        .map_err(storage_error)?;
    for mapping in &manifest.config().mappings {
        transaction
            .execute(
                "INSERT INTO registrations(device, vehicle, owner) VALUES (?1, ?2, ?3)",
                params![mapping.device, mapping.vehicle, mapping.owner],
            )
            .map_err(storage_error)?;
    }
    for start in &source.partitions {
        transaction
            .execute(
                "INSERT INTO partition_progress VALUES (?1, ?2, ?2)",
                params![start.partition, start.initial_offset],
            )
            .map_err(storage_error)?;
    }
    transaction.commit().map_err(storage_error)
}

pub(crate) fn verify_configuration(
    connection: &Connection,
    manifest: &Manifest,
    manifest_json: &str,
    source: &SourceBinding,
    source_json: &str,
) -> Result<(), IngestionError> {
    let integrity: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(storage_error)?;
    if integrity != "ok" {
        return Err(IngestionError::StorageCorrupt);
    }
    let foreign_key_failure = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(storage_error)?
        .exists([])
        .map_err(storage_error)?;
    if foreign_key_failure {
        return Err(IngestionError::StorageCorrupt);
    }
    let (stored_version, stored_manifest, stored_source): (i64, String, String) = connection
        .query_row(
            "SELECT schema_version, manifest_json, source_json FROM runtime_metadata WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| IngestionError::StorageCorrupt)?;
    if stored_version != SCHEMA_VERSION {
        return Err(IngestionError::SchemaMismatch);
    }
    if stored_manifest != manifest_json {
        return Err(IngestionError::ConfigurationMismatch);
    }
    if stored_source != source_json {
        return Err(IngestionError::SourceMismatch);
    }
    let mut statement = connection
        .prepare("SELECT device, vehicle, owner FROM registrations ORDER BY device")
        .map_err(storage_error)?;
    let registrations = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    let expected = manifest
        .config()
        .mappings
        .iter()
        .map(|mapping| {
            (
                mapping.device.clone(),
                mapping.vehicle.clone(),
                mapping.owner.clone(),
            )
        })
        .collect::<Vec<_>>();
    if registrations != expected {
        return Err(IngestionError::StorageCorrupt);
    }
    let mut statement = connection
        .prepare("SELECT partition, initial_offset, next_offset FROM partition_progress ORDER BY partition")
        .map_err(storage_error)?;
    let progress = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i32>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    if progress.len() != source.partitions.len() {
        return Err(IngestionError::StorageCorrupt);
    }
    for ((partition, initial, next), start) in progress.iter().zip(&source.partitions) {
        if *partition != start.partition || *initial != start.initial_offset || next < initial {
            return Err(IngestionError::StorageCorrupt);
        }
        let maximum: Option<i64> = connection
            .query_row(
                "SELECT max(offset) FROM deliveries WHERE partition = ?1",
                [partition],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let expected_next = match maximum {
            Some(offset) if offset >= *initial => offset.checked_add(1),
            Some(_) => None,
            None => Some(*initial),
        };
        if expected_next != Some(*next) {
            return Err(IngestionError::StorageCorrupt);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{initialize, inspect_version};
    use crate::{
        DeviceMapping, INPUT_CONTRACT_VERSION, IngestionError, Manifest, ManifestConfig,
        NORMALIZER_VERSION, PartitionStart, SYNTHETIC_TOPIC, SourceBinding, TIMESTAMP_VERSION,
    };
    use rusqlite::Connection;

    #[test]
    fn full_initialization_rolls_back_every_table_and_schema_version() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "max_page_count", 1).unwrap();
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
            epoch: "synthetic-migration-test".to_owned(),
            topic: SYNTHETIC_TOPIC.to_owned(),
            cluster_id: None,
            topic_id: None,
            partitions: vec![PartitionStart {
                partition: 0,
                initial_offset: 0,
            }],
        };
        let manifest_json = serde_json::to_string(manifest.config()).unwrap();
        let source_json = serde_json::to_string(&source).unwrap();
        assert_eq!(
            initialize(
                &mut connection,
                &manifest,
                &manifest_json,
                &source,
                &source_json
            ),
            Err(IngestionError::StorageUnavailable)
        );
        assert!(inspect_version(&connection).unwrap());
        let tables: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
    }
}
