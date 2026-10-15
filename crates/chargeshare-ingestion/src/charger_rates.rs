use std::path::Path;

use chargeshare_core::{RateAssignment, RateModelHistory};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::registry::{Charger, json};
use crate::schema::storage_error;
use crate::{IngestionError, RateModelConfig, Store};

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct ChargerRateAssignment {
    pub(crate) active_from: i64,
    pub(crate) rate_version_id: String,
}

pub(crate) fn load_versions(
    connection: &Connection,
) -> Result<Vec<RateModelConfig>, IngestionError> {
    let rows = connection
        .prepare("SELECT id, config_json FROM rate_model_versions ORDER BY id")
        .map_err(storage_error)?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    rows.into_iter()
        .map(|(id, contents)| {
            let config: RateModelConfig =
                serde_json::from_str(&contents).map_err(|_| IngestionError::RegistryIntegrity)?;
            if config.id != id || config.to_core().is_err() {
                return Err(IngestionError::RegistryIntegrity);
            }
            Ok(config)
        })
        .collect()
}

pub(crate) fn load_assignments(
    connection: &Connection,
    charger: &str,
) -> Result<Vec<ChargerRateAssignment>, IngestionError> {
    connection.prepare("SELECT active_from, rate_version_id FROM charger_rate_assignments WHERE charger_id = ?1 ORDER BY active_from")
        .map_err(storage_error)?.query_map([charger], |row| Ok(ChargerRateAssignment { active_from: row.get(0)?, rate_version_id: row.get(1)? }))
        .map_err(storage_error)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage_error)
}

pub(crate) fn history(
    connection: &Connection,
    charger: &Charger,
) -> Result<RateModelHistory, IngestionError> {
    if charger.active_rate_version_id.as_ref()
        != charger
            .rate_assignments
            .last()
            .map(|assignment| &assignment.rate_version_id)
    {
        return Err(IngestionError::RegistryIntegrity);
    }
    let versions = load_versions(connection)?
        .into_iter()
        .filter(|version| {
            charger
                .rate_assignments
                .iter()
                .any(|assignment| assignment.rate_version_id == version.id)
        })
        .map(|version| {
            version
                .to_core()
                .map_err(|_| IngestionError::RegistryIntegrity)
        })
        .collect::<Result<Vec<_>, _>>()?;
    RateModelHistory::assigned(
        versions,
        charger
            .rate_assignments
            .iter()
            .map(|assignment| RateAssignment {
                active_from: assignment.active_from,
                model_version: assignment.rate_version_id.clone(),
            })
            .collect(),
    )
    .map_err(|_| IngestionError::RegistryIntegrity)
}

pub(crate) fn migrate(connection: &Connection) -> Result<(), IngestionError> {
    let schema: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    if schema == 5 {
        return Ok(());
    }
    if schema != 4 {
        return Err(IngestionError::SchemaMismatch);
    }
    let versions = load_versions(connection)?;
    let chargers = connection
        .prepare("SELECT id, rate_plan FROM chargers ORDER BY id")
        .map_err(storage_error)?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    connection.execute_batch("ALTER TABLE chargers ADD COLUMN active_rate_version_id TEXT REFERENCES rate_model_versions(id);
        CREATE TABLE charger_rate_assignments(charger_id TEXT NOT NULL REFERENCES chargers(id), active_from INTEGER NOT NULL, rate_version_id TEXT NOT NULL REFERENCES rate_model_versions(id), PRIMARY KEY(charger_id, active_from)) STRICT;
        ALTER TABLE runtime_metadata RENAME TO rate_legacy_metadata;
        CREATE TABLE runtime_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton = 1), schema_version INTEGER NOT NULL CHECK(schema_version = 5), manifest_json TEXT NOT NULL, source_json TEXT NOT NULL) STRICT;
        INSERT INTO runtime_metadata SELECT singleton,5,manifest_json,source_json FROM rate_legacy_metadata;
        DROP TABLE rate_legacy_metadata;
        PRAGMA user_version = 5;").map_err(storage_error)?;
    for (charger, plan) in chargers {
        let ids: Vec<String> =
            serde_json::from_str(&plan).map_err(|_| IngestionError::RegistryIntegrity)?;
        let selected = ids
            .iter()
            .map(|id| {
                versions
                    .iter()
                    .find(|version| &version.id == id)
                    .ok_or(IngestionError::RegistryIntegrity)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let legacy = RateModelHistory::new(
            selected
                .iter()
                .map(|version| {
                    version
                        .to_core()
                        .map_err(|_| IngestionError::RegistryIntegrity)
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
        .map_err(|_| IngestionError::RegistryIntegrity)?;
        for version in legacy.versions() {
            let active_from = version
                .effective_from_time()
                .ok_or(IngestionError::RegistryIntegrity)?;
            connection
                .execute(
                    "INSERT INTO charger_rate_assignments VALUES (?1,?2,?3)",
                    params![charger, active_from, version.definition().id],
                )
                .map_err(storage_error)?;
            connection
                .execute(
                    "UPDATE chargers SET active_rate_version_id = ?1 WHERE id = ?2",
                    params![version.definition().id, charger],
                )
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

impl Store {
    pub fn assign_charger_rate(
        database: &Path,
        charger: &str,
        active_from: i64,
        definition: &RateModelConfig,
    ) -> Result<(), IngestionError> {
        definition
            .to_core()
            .map_err(|_| IngestionError::InvalidConfiguration)?;
        let mut store = Self::open_registry_database(database)?;
        let registry = store
            .registry
            .as_ref()
            .ok_or(IngestionError::MigrationInputRequired)?;
        let selected = registry
            .chargers
            .get(charger)
            .ok_or(IngestionError::RegistryConflict)?;
        let mut assignments = selected.rate_assignments.clone();
        match assignments
            .iter()
            .find(|assignment| assignment.active_from == active_from)
        {
            Some(existing) if existing.rate_version_id != definition.id => {
                return Err(IngestionError::RegistryConflict);
            }
            Some(_) => {}
            None => assignments.push(ChargerRateAssignment {
                active_from,
                rate_version_id: definition.id.clone(),
            }),
        }
        let mut versions = load_versions(&store.connection)?;
        if let Some(existing) = versions.iter().find(|version| version.id == definition.id) {
            if existing != definition {
                return Err(IngestionError::ConfigurationMismatch);
            }
        } else {
            versions.push(definition.clone());
        }
        RateModelHistory::assigned(
            versions
                .iter()
                .map(|version| {
                    version
                        .to_core()
                        .map_err(|_| IngestionError::InvalidConfiguration)
                })
                .collect::<Result<Vec<_>, _>>()?,
            assignments
                .iter()
                .map(|assignment| RateAssignment {
                    active_from: assignment.active_from,
                    model_version: assignment.rate_version_id.clone(),
                })
                .collect(),
        )
        .map_err(|_| IngestionError::RegistryConflict)?;
        let transaction = store
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let saved: Option<String> = transaction
            .query_row(
                "SELECT config_json FROM rate_model_versions WHERE id = ?1",
                [&definition.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if saved.is_none() {
            transaction
                .execute(
                    "INSERT INTO rate_model_versions VALUES (?1,?2)",
                    params![definition.id, json(definition)?],
                )
                .map_err(storage_error)?;
        }
        transaction
            .execute(
                "INSERT OR IGNORE INTO charger_rate_assignments VALUES (?1,?2,?3)",
                params![charger, active_from, definition.id],
            )
            .map_err(storage_error)?;
        transaction.execute("UPDATE chargers SET active_rate_version_id = (SELECT rate_version_id FROM charger_rate_assignments WHERE charger_id = ?1 ORDER BY active_from DESC LIMIT 1) WHERE id = ?1", [charger]).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }
}
