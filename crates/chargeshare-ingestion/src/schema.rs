use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};

use crate::{IngestionError, Manifest, SourceBinding};

pub(crate) const SCHEMA_VERSION: i64 = 3;
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
    if ![1, 2, SCHEMA_VERSION, 4].contains(&version) || application != APPLICATION_ID {
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
                schema_version INTEGER NOT NULL CHECK (schema_version = 3),
                manifest_json TEXT NOT NULL,
                source_json TEXT NOT NULL
            ) STRICT;
            CREATE TABLE geofence_versions (
                version TEXT PRIMARY KEY,
                config_json TEXT NOT NULL
            ) STRICT;
            CREATE TABLE rate_model_versions (id TEXT PRIMARY KEY, config_json TEXT NOT NULL) STRICT;
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
            PRAGMA user_version = 3;
            PRAGMA application_id = 1129531726;",
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO runtime_metadata VALUES (1, ?1, ?2, ?3)",
            params![SCHEMA_VERSION, manifest_json, source_json],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO geofence_versions VALUES (?1, ?2)",
            params![manifest.config().geofence_version, geofence_json(manifest)?],
        )
        .map_err(storage_error)?;
    record_rate_versions(&transaction, manifest)?;
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
    let database_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    if stored_version != database_version || ![1, 2, SCHEMA_VERSION].contains(&stored_version) {
        return Err(IngestionError::SchemaMismatch);
    }
    let retained = Manifest::from_json(stored_manifest.as_bytes())
        .map_err(|_| IngestionError::StorageCorrupt)?;
    let mut frozen = retained.config().clone();
    frozen.geofence_version = manifest.config().geofence_version.clone();
    frozen.geofences = manifest.config().geofences.clone();
    frozen.owner_chargers = manifest.config().owner_chargers.clone();
    frozen.rate_models = manifest.config().rate_models.clone();
    if frozen != *manifest.config() {
        return Err(IngestionError::ConfigurationMismatch);
    }
    let explicit_version = serde_json::from_str::<serde_json::Value>(&stored_manifest)
        .map_err(|_| IngestionError::StorageCorrupt)?["geofence_version"]
        .is_string();
    if explicit_version
        && retained.config().geofence_version == manifest.config().geofence_version
        && geofence_json(&retained)? != geofence_json(manifest)?
    {
        return Err(IngestionError::ConfigurationMismatch);
    }
    if stored_version >= 2 {
        let retained_config: Option<String> = connection
            .query_row(
                "SELECT config_json FROM geofence_versions WHERE version = ?1",
                [&retained.config().geofence_version],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if retained_config.as_ref() != Some(&geofence_json(&retained)?) {
            return Err(IngestionError::StorageCorrupt);
        }
        let recorded: Option<String> = connection
            .query_row(
                "SELECT config_json FROM geofence_versions WHERE version = ?1",
                [&manifest.config().geofence_version],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if recorded.is_some_and(|recorded| geofence_json(manifest).as_ref() != Ok(&recorded)) {
            return Err(IngestionError::ConfigurationMismatch);
        }
    }
    if stored_version == SCHEMA_VERSION {
        verify_rate_versions(connection, &retained, true)?;
        verify_rate_versions(connection, manifest, false)?;
    }
    if serde_json::to_string(manifest.config()).map_err(|_| IngestionError::InvalidConfiguration)?
        != manifest_json
    {
        return Err(IngestionError::InvalidConfiguration);
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

fn geofence_json(manifest: &Manifest) -> Result<String, IngestionError> {
    let config = manifest.config();
    if config.owner_chargers.is_empty() {
        return serde_json::to_string(&config.geofences)
            .map_err(|_| IngestionError::InvalidConfiguration);
    }
    let geofences: Vec<_> = config
        .owner_chargers
        .iter()
        .map(|charger| crate::OwnerGeofence {
            owner: charger.owner_id.clone(),
            center: charger.geofence.center.clone(),
            radius_meters: charger.geofence.radius_meters,
        })
        .collect();
    serde_json::to_string(&geofences).map_err(|_| IngestionError::InvalidConfiguration)
}

pub(crate) fn load_configuration(
    connection: &mut Connection,
    manifest: &Manifest,
    manifest_json: &str,
) -> Result<(), IngestionError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    let version: i64 = transaction
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    let retained: String = transaction
        .query_row(
            "SELECT manifest_json FROM runtime_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if version == SCHEMA_VERSION && retained == manifest_json {
        return Ok(());
    }
    if version == 1 {
        transaction.execute_batch(
            "ALTER TABLE runtime_metadata RENAME TO previous_runtime_metadata;
            CREATE TABLE runtime_metadata (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                schema_version INTEGER NOT NULL CHECK (schema_version = 2),
                manifest_json TEXT NOT NULL,
                source_json TEXT NOT NULL
            ) STRICT;
            INSERT INTO runtime_metadata SELECT singleton, 2, manifest_json, source_json FROM previous_runtime_metadata;
            DROP TABLE previous_runtime_metadata;
            CREATE TABLE geofence_versions (version TEXT PRIMARY KEY, config_json TEXT NOT NULL) STRICT;
            PRAGMA user_version = 2;"
        ).map_err(storage_error)?;
        let value: serde_json::Value =
            serde_json::from_str(&retained).map_err(|_| IngestionError::StorageCorrupt)?;
        if value["geofence_version"].is_string() {
            let retained = Manifest::from_json(retained.as_bytes())
                .map_err(|_| IngestionError::StorageCorrupt)?;
            transaction
                .execute(
                    "INSERT INTO geofence_versions VALUES (?1, ?2)",
                    params![
                        retained.config().geofence_version,
                        geofence_json(&retained)?
                    ],
                )
                .map_err(storage_error)?;
        }
    }
    if version <= 2 {
        transaction.execute_batch(
            "ALTER TABLE runtime_metadata RENAME TO previous_runtime_metadata;
            CREATE TABLE runtime_metadata (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                schema_version INTEGER NOT NULL CHECK (schema_version = 3),
                manifest_json TEXT NOT NULL,
                source_json TEXT NOT NULL
            ) STRICT;
            INSERT INTO runtime_metadata SELECT singleton, 3, manifest_json, source_json FROM previous_runtime_metadata;
            DROP TABLE previous_runtime_metadata;
            CREATE TABLE rate_model_versions (id TEXT PRIMARY KEY, config_json TEXT NOT NULL) STRICT;
            PRAGMA user_version = 3;"
        ).map_err(storage_error)?;
    }
    record_rate_versions(&transaction, manifest)?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO geofence_versions VALUES (?1, ?2)",
            params![manifest.config().geofence_version, geofence_json(manifest)?],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE runtime_metadata SET manifest_json = ?1 WHERE singleton = 1",
            [manifest_json],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

pub(crate) fn verify_rate_versions(
    connection: &Connection,
    manifest: &Manifest,
    required: bool,
) -> Result<(), IngestionError> {
    let mut statement = connection
        .prepare("SELECT id, config_json FROM rate_model_versions")
        .map_err(storage_error)?;
    let retained = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<std::collections::BTreeMap<_, _>>>()
        .map_err(storage_error)?;
    for (id, json) in &retained {
        let config: crate::RateModelConfig =
            serde_json::from_str(json).map_err(|_| IngestionError::StorageCorrupt)?;
        if &config.id != id
            || config.to_core().is_err()
            || serde_json::to_string(&config).map_err(|_| IngestionError::StorageCorrupt)? != *json
        {
            return Err(IngestionError::StorageCorrupt);
        }
    }
    for config in &manifest.config().rate_models {
        let expected =
            serde_json::to_string(config).map_err(|_| IngestionError::InvalidConfiguration)?;
        match retained.get(&config.id) {
            Some(recorded) if recorded != &expected => {
                return Err(if required {
                    IngestionError::StorageCorrupt
                } else {
                    IngestionError::ConfigurationMismatch
                });
            }
            None if required => return Err(IngestionError::StorageCorrupt),
            _ => {}
        }
    }
    Ok(())
}

fn record_rate_versions(
    transaction: &rusqlite::Transaction<'_>,
    manifest: &Manifest,
) -> Result<(), IngestionError> {
    for config in &manifest.config().rate_models {
        transaction
            .execute(
                "INSERT OR IGNORE INTO rate_model_versions VALUES (?1, ?2)",
                params![
                    config.id,
                    serde_json::to_string(config)
                        .map_err(|_| IngestionError::InvalidConfiguration)?
                ],
            )
            .map_err(storage_error)?;
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
            geofence_version: "fictional-geofence-v1".to_owned(),
            geofences: Vec::new(),
            owner_chargers: Vec::new(),
            rate_models: Vec::new(),
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

    fn memory_runtime() -> (Connection, Manifest, crate::SourceBinding) {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        let manifest =
            Manifest::from_json(include_bytes!("../tests/fixtures/manifest-v1.json")).unwrap();
        let source = crate::SourceBinding {
            epoch: "synthetic-config-test".to_owned(),
            topic: crate::SYNTHETIC_TOPIC.to_owned(),
            cluster_id: None,
            topic_id: None,
            partitions: vec![crate::PartitionStart {
                partition: 0,
                initial_offset: 0,
            }],
        };
        initialize(
            &mut connection,
            &manifest,
            &serde_json::to_string(manifest.config()).unwrap(),
            &source,
            &serde_json::to_string(&source).unwrap(),
        )
        .unwrap();
        (connection, manifest, source)
    }

    #[test]
    fn geofence_version_loading_preserves_evidence_and_rejects_reused_version_content() {
        let (mut connection, original, source) = memory_runtime();
        let mut revised = original.config().clone();
        revised.geofence_version = "fictional-geofence-v2".to_owned();
        revised.geofences[0].center =
            crate::NormalizedLocation::from_coordinates(12.3556789, 45.6789123);
        let revised = Manifest::new(revised).unwrap();
        let manifest_json = serde_json::to_string(revised.config()).unwrap();
        let source_json = serde_json::to_string(&source).unwrap();
        super::verify_configuration(&connection, &revised, &manifest_json, &source, &source_json)
            .unwrap();
        super::load_configuration(&mut connection, &revised, &manifest_json).unwrap();
        super::verify_configuration(&connection, &revised, &manifest_json, &source, &source_json)
            .unwrap();
        let versions: i64 = connection
            .query_row("SELECT count(*) FROM geofence_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(versions, 2);
        let mut reused = revised.config().clone();
        reused.geofences[0].center = original.config().geofences[0].center.clone();
        let reused = Manifest::new(reused).unwrap();
        assert_eq!(
            super::verify_configuration(
                &connection,
                &reused,
                &serde_json::to_string(reused.config()).unwrap(),
                &source,
                &source_json
            ),
            Err(IngestionError::ConfigurationMismatch)
        );
        let original_json = serde_json::to_string(original.config()).unwrap();
        super::verify_configuration(
            &connection,
            &original,
            &original_json,
            &source,
            &source_json,
        )
        .unwrap();
        super::load_configuration(&mut connection, &original, &original_json).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn schema_one_upgrade_keeps_retained_missing_location_evidence_and_progress() {
        let (mut connection, manifest, source) = memory_runtime();
        let mut legacy = serde_json::to_value(manifest.config()).unwrap();
        legacy.as_object_mut().unwrap().remove("geofence_version");
        legacy.as_object_mut().unwrap().remove("geofences");
        let legacy_json = serde_json::to_string(&legacy).unwrap();
        let legacy = Manifest::from_json(legacy_json.as_bytes()).unwrap();
        let raw = br#"{"data":[{"key":"ACChargingEnergyIn","value":{"stringValue":"100.00"}}],"createdAt":"2026-09-01T12:00:00Z","vin":"device-1","isResend":false}"#;
        let evidence = crate::normalize(&legacy, Some(b"device-1"), raw).unwrap();
        let event_json = serde_json::to_string(&evidence.event).unwrap();
        let provenance_json = serde_json::to_string(&evidence.provenance).unwrap();
        connection
            .execute(
                "INSERT INTO event_variants VALUES (1, ?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    evidence.event.vehicle,
                    evidence.event.position,
                    evidence.event.connection,
                    evidence.event.time,
                    event_json
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO deliveries VALUES (0, 0, 1, ?1, NULL)",
                [&provenance_json],
            )
            .unwrap();
        connection
            .execute("UPDATE partition_progress SET next_offset = 1", [])
            .unwrap();
        connection.execute_batch("ALTER TABLE runtime_metadata RENAME TO current_metadata;
            CREATE TABLE runtime_metadata (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), schema_version INTEGER NOT NULL CHECK(schema_version = 1), manifest_json TEXT NOT NULL, source_json TEXT NOT NULL) STRICT;
            INSERT INTO runtime_metadata SELECT singleton, 1, manifest_json, source_json FROM current_metadata;
            DROP TABLE current_metadata;
            DROP TABLE geofence_versions;
            DROP TABLE rate_model_versions;
            PRAGMA user_version = 1;").unwrap();
        connection
            .execute(
                "UPDATE runtime_metadata SET manifest_json = ?1",
                [&legacy_json],
            )
            .unwrap();
        let manifest_json = serde_json::to_string(manifest.config()).unwrap();
        let source_json = serde_json::to_string(&source).unwrap();
        super::verify_configuration(
            &connection,
            &manifest,
            &manifest_json,
            &source,
            &source_json,
        )
        .unwrap();
        super::load_configuration(&mut connection, &manifest, &manifest_json).unwrap();
        super::verify_configuration(
            &connection,
            &manifest,
            &manifest_json,
            &source,
            &source_json,
        )
        .unwrap();
        let retained: String = connection
            .query_row("SELECT event_json FROM event_variants", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(retained == event_json);
        assert!(!retained.contains("location"));
        let position: i64 = connection
            .query_row("SELECT next_offset FROM partition_progress", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(position, 1);
        let recovered: crate::NormalizedEvent = serde_json::from_str(&retained).unwrap();
        assert_eq!(
            recovered.to_core().unwrap().location,
            chargeshare_core::LocationSample::Missing
        );
    }
    fn priced_memory_runtime() -> (Connection, Manifest, crate::SourceBinding) {
        let (mut connection, _, source) = memory_runtime();
        connection.execute_batch("DROP TABLE deliveries; DROP TABLE event_variants; DROP TABLE partition_progress; DROP TABLE registrations; DROP TABLE geofence_versions; DROP TABLE rate_model_versions; DROP TABLE runtime_metadata;").unwrap();
        let manifest =
            Manifest::from_json(include_bytes!("../../../dev/ingestion-manifest.json")).unwrap();
        initialize(
            &mut connection,
            &manifest,
            &serde_json::to_string(manifest.config()).unwrap(),
            &source,
            &serde_json::to_string(&source).unwrap(),
        )
        .unwrap();
        (connection, manifest, source)
    }

    #[test]
    fn immutable_rate_versions_survive_selection_changes_and_refuse_changed_reused_ids() {
        let (mut connection, original, source) = priced_memory_runtime();
        let source_json = serde_json::to_string(&source).unwrap();
        let mut revised = original.config().clone();
        revised.rate_models[0].id = "pge-ev2-a-v2".to_owned();
        revised.rate_models[0].periods[0].usd_per_kwh = "0.3".to_owned();
        for charger in &mut revised.owner_chargers {
            charger.rate_model_versions = vec!["pge-ev2-a-v2".to_owned()];
        }
        let revised = Manifest::new(revised).unwrap();
        let revised_json = serde_json::to_string(revised.config()).unwrap();
        super::verify_configuration(&connection, &revised, &revised_json, &source, &source_json)
            .unwrap();
        super::load_configuration(&mut connection, &revised, &revised_json).unwrap();
        super::verify_configuration(&connection, &revised, &revised_json, &source, &source_json)
            .unwrap();
        let versions: i64 = connection
            .query_row("SELECT count(*) FROM rate_model_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(versions, 2);
        let mut changed_original = original.config().clone();
        changed_original.rate_models[0].periods[0].usd_per_kwh = "0.4".to_owned();
        let changed_original = Manifest::new(changed_original).unwrap();
        assert_eq!(
            super::verify_configuration(
                &connection,
                &changed_original,
                &serde_json::to_string(changed_original.config()).unwrap(),
                &source,
                &source_json
            ),
            Err(IngestionError::ConfigurationMismatch)
        );
        let retained: String = connection
            .query_row("SELECT manifest_json FROM runtime_metadata", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(retained, revised_json);
        let original_json = serde_json::to_string(original.config()).unwrap();
        super::verify_configuration(
            &connection,
            &original,
            &original_json,
            &source,
            &source_json,
        )
        .unwrap();
        super::load_configuration(&mut connection, &original, &original_json).unwrap();
        super::verify_configuration(
            &connection,
            &original,
            &original_json,
            &source,
            &source_json,
        )
        .unwrap();
    }

    #[test]
    fn schema_two_upgrade_retains_evidence_and_records_the_selected_rate_history() {
        let (mut connection, original, source) = memory_runtime();
        let legacy_json = serde_json::to_string(original.config()).unwrap();
        let raw = br#"{"data":[{"key":"ACChargingEnergyIn","value":{"stringValue":"100.00"}}],"createdAt":"2026-09-01T12:00:00Z","vin":"device-1","isResend":false}"#;
        let evidence = crate::normalize(&original, Some(b"device-1"), raw).unwrap();
        let event_json = serde_json::to_string(&evidence.event).unwrap();
        let provenance_json = serde_json::to_string(&evidence.provenance).unwrap();
        connection
            .execute(
                "INSERT INTO event_variants VALUES (1, ?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    evidence.event.vehicle,
                    evidence.event.position,
                    evidence.event.connection,
                    evidence.event.time,
                    event_json
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO deliveries VALUES (0, 0, 1, ?1, NULL)",
                [&provenance_json],
            )
            .unwrap();
        connection
            .execute("UPDATE partition_progress SET next_offset = 1", [])
            .unwrap();
        connection.execute_batch("ALTER TABLE runtime_metadata RENAME TO current_metadata;
            CREATE TABLE runtime_metadata (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), schema_version INTEGER NOT NULL CHECK(schema_version = 2), manifest_json TEXT NOT NULL, source_json TEXT NOT NULL) STRICT;
            INSERT INTO runtime_metadata SELECT singleton, 2, manifest_json, source_json FROM current_metadata;
            DROP TABLE current_metadata;
            DROP TABLE rate_model_versions;
            PRAGMA user_version = 2;").unwrap();
        let mut config = original.config().clone();
        let sample =
            Manifest::from_json(include_bytes!("../../../dev/ingestion-manifest.json")).unwrap();
        config.rate_models = sample.config().rate_models.clone();
        config.owner_chargers = sample.config().owner_chargers.clone();
        config.geofences.clear();
        config.geofence_version = sample.config().geofence_version.clone();
        let revised = Manifest::new(config).unwrap();
        let revised_json = serde_json::to_string(revised.config()).unwrap();
        super::verify_configuration(
            &connection,
            &revised,
            &revised_json,
            &source,
            &serde_json::to_string(&source).unwrap(),
        )
        .unwrap();
        super::load_configuration(&mut connection, &revised, &revised_json).unwrap();
        let retained: String = connection
            .query_row("SELECT manifest_json FROM runtime_metadata", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_ne!(retained, legacy_json);
        assert_eq!(retained, revised_json);
        let retained_event: String = connection
            .query_row("SELECT event_json FROM event_variants", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(retained_event, event_json);
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT next_offset FROM partition_progress", [], |row| row
                    .get(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            3
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT count(*) FROM rate_model_versions", [], |row| row
                    .get(0))
                .unwrap(),
            1
        );
        super::verify_configuration(
            &connection,
            &revised,
            &revised_json,
            &source,
            &serde_json::to_string(&source).unwrap(),
        )
        .unwrap();
    }
}
