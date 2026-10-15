use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use chargeshare_core::{OwnerId, VehicleId};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::schema::storage_error;
use crate::{
    ChargerGeofenceConfig, DeviceMapping, IngestionError, Manifest, RegistryBootstrap, Store,
};

#[derive(Clone, Deserialize, Serialize, Eq, PartialEq)]
pub struct CarRegistration {
    pub device: String,
    pub vehicle: String,
    pub owner: String,
    pub name: String,
    pub make: Option<String>,
    pub model: Option<String>,
    pub year: Option<u16>,
    pub chargers: Vec<String>,
}

pub struct RegistrySetup {
    pub car_names: BTreeMap<String, String>,
    pub charger_name: String,
    pub charger_owner: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryWrite {
    RecoveryPending,
    RecoveryComplete,
    Updated,
}

#[derive(Clone)]
pub(crate) struct Car {
    pub(crate) registration: CarRegistration,
    pub(crate) active: bool,
    pub(crate) identity_version: String,
    pub(crate) assignment_version: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Charger {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) owner: String,
    pub(crate) geofence: ChargerGeofenceConfig,
    pub(crate) geofence_version: String,
    pub(crate) rate_plan: Vec<String>,
    #[serde(default)]
    pub(crate) active_rate_version_id: Option<String>,
    #[serde(default)]
    pub(crate) rate_assignments: Vec<crate::charger_rates::ChargerRateAssignment>,
}

#[derive(Clone)]
pub(crate) struct RegistrySnapshot {
    pub(crate) cars: Vec<Car>,
    pub(crate) chargers: BTreeMap<String, Charger>,
    pub(crate) legacy_charger: Option<String>,
    pub(crate) assignments: BTreeMap<String, (String, Vec<String>)>,
    pub(crate) geofence_versions: BTreeMap<String, (String, ChargerGeofenceConfig)>,
}

pub(crate) fn json(value: &impl Serialize) -> Result<String, IngestionError> {
    serde_json::to_string(value).map_err(|_| IngestionError::RegistryIntegrity)
}

fn text_valid(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn validate_car(car: &CarRegistration) -> Result<(), IngestionError> {
    if car.device.trim().is_empty()
        || car.device.len() > 64
        || car.device.chars().any(char::is_control)
        || VehicleId::new(&car.vehicle).is_err()
        || OwnerId::new(&car.owner).is_err()
        || !text_valid(&car.name)
        || car.make.as_deref().is_some_and(|value| !text_valid(value))
        || car.model.as_deref().is_some_and(|value| !text_valid(value))
        || car.year.is_some_and(|year| !(1886..=9999).contains(&year))
        || car.chargers.is_empty()
        || car.chargers.len() > 256
        || car.chargers.iter().any(|id| VehicleId::new(id).is_err())
    {
        return Err(IngestionError::RegistryConflict);
    }
    Ok(())
}

pub(crate) fn is_registry(connection: &Connection) -> Result<bool, IngestionError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    Ok([4, 5].contains(&version))
}

fn version(prefix: &str, value: &impl Serialize) -> Result<String, IngestionError> {
    Ok(format!(
        "{prefix}-{:016x}",
        crate::live::fingerprint(json(value)?.as_bytes())
    ))
}

pub(crate) fn record_identity(
    connection: &Connection,
    car: &CarRegistration,
) -> Result<(String, String), IngestionError> {
    let identity_json = json(&(&car.device, &car.vehicle, &car.owner))?;
    let identity = version("car", &(&car.vehicle, &car.owner))?;
    let assignment_json = json(&(&car.vehicle, &car.chargers))?;
    let assignment = version("assignment", &(&car.vehicle, &car.chargers))?;
    connection
        .execute(
            "INSERT INTO car_identity_versions VALUES (?1, ?2, ?3)",
            params![identity, car.vehicle, identity_json],
        )
        .map_err(storage_error)?;
    connection
        .execute(
            "INSERT INTO assignment_versions VALUES (?1, ?2, ?3)",
            params![assignment, car.vehicle, assignment_json],
        )
        .map_err(storage_error)?;
    Ok((identity, assignment))
}

pub(crate) fn load(
    connection: &Connection,
    fixed: &Manifest,
) -> Result<(RegistrySnapshot, Manifest), IngestionError> {
    crate::schema::verify_rate_versions(connection, fixed, true)?;
    let metadata_version: i64 = connection
        .query_row("SELECT schema_version FROM runtime_metadata", [], |row| {
            row.get(0)
        })
        .map_err(storage_error)?;
    if metadata_version != 5
        || !fixed.config().mappings.is_empty()
        || !fixed.config().owner_chargers.is_empty()
        || !fixed.is_live()
    {
        return Err(IngestionError::RegistryIntegrity);
    }
    let foreign_failure = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(storage_error)?
        .exists([])
        .map_err(storage_error)?;
    let integrity: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(storage_error)?;
    if foreign_failure || integrity != "ok" {
        return Err(IngestionError::RegistryIntegrity);
    }
    let mut statement = connection.prepare("SELECT id, name, owner, location, radius, geofence_version, rate_plan, active_rate_version_id FROM chargers ORDER BY id").map_err(storage_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    let mut chargers = BTreeMap::new();
    for (
        id,
        name,
        owner,
        location,
        radius_meters,
        geofence_version,
        rate_plan,
        active_rate_version_id,
    ) in rows
    {
        let charger = Charger {
            id: id.clone(),
            name,
            owner,
            geofence: ChargerGeofenceConfig {
                center: serde_json::from_str(&location)
                    .map_err(|_| IngestionError::RegistryIntegrity)?,
                radius_meters,
            },
            geofence_version,
            active_rate_version_id,
            rate_assignments: crate::charger_rates::load_assignments(connection, &id)?,
            rate_plan: serde_json::from_str(&rate_plan)
                .map_err(|_| IngestionError::RegistryIntegrity)?,
        };
        if VehicleId::new(&id).is_err()
            || OwnerId::new(&charger.owner).is_err()
            || !text_valid(&charger.name)
            || charger.geofence.to_core().is_err()
        {
            return Err(IngestionError::RegistryIntegrity);
        }
        let retained: String = connection.query_row("SELECT config_json FROM charger_geofence_versions WHERE version = ?1 AND charger_id = ?2", params![charger.geofence_version, id], |row| row.get(0)).map_err(|_| IngestionError::RegistryIntegrity)?;
        if retained != json(&charger.geofence)? {
            return Err(IngestionError::RegistryIntegrity);
        }
        crate::charger_rates::history(connection, &charger)?;
        chargers.insert(id, charger);
    }
    let mut statement = connection.prepare("SELECT device, vehicle, owner, name, make, model, year, active, created_at FROM cars ORDER BY vehicle").map_err(storage_error)?;
    let car_rows = statement
        .query_map([], |row| {
            Ok((
                CarRegistration {
                    device: row.get(0)?,
                    vehicle: row.get(1)?,
                    owner: row.get(2)?,
                    name: row.get(3)?,
                    make: row.get(4)?,
                    model: row.get(5)?,
                    year: row.get(6)?,
                    chargers: Vec::new(),
                },
                row.get::<_, bool>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    let mut assignments = BTreeMap::new();
    let rows = connection
        .prepare("SELECT version,vehicle,config_json FROM assignment_versions")
        .map_err(storage_error)?
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
    for (id, vehicle, config_json) in rows {
        let config: (String, Vec<String>) =
            serde_json::from_str(&config_json).map_err(|_| IngestionError::RegistryIntegrity)?;
        if config.0 != vehicle
            || id != version("assignment", &config)?
            || config.1.iter().any(|id| !chargers.contains_key(id))
        {
            return Err(IngestionError::RegistryIntegrity);
        }
        assignments.insert(id, config);
    }
    let mut geofence_versions = BTreeMap::new();
    let rows = connection
        .prepare("SELECT version,charger_id,config_json FROM charger_geofence_versions")
        .map_err(storage_error)?
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
    for (id, charger, config_json) in rows {
        let config: ChargerGeofenceConfig =
            serde_json::from_str(&config_json).map_err(|_| IngestionError::RegistryIntegrity)?;
        if id != version("geofence", &(&charger, &config))? || config.to_core().is_err() {
            return Err(IngestionError::RegistryIntegrity);
        }
        geofence_versions.insert(id, (charger, config));
    }
    let mut cars = Vec::new();
    let mut config = fixed.config().clone();
    config.rate_models = crate::charger_rates::load_versions(connection)?;
    let mut versions = BTreeMap::<String, BTreeSet<String>>::new();
    for (mut car, active, created_at) in car_rows {
        if crate::manifest::parse_utc_seconds(&created_at).is_err() {
            return Err(IngestionError::RegistryIntegrity);
        }
        car.chargers = connection
            .prepare("SELECT charger_id FROM car_chargers WHERE car_id = ?1 ORDER BY charger_id")
            .map_err(storage_error)?
            .query_map([&car.device], |row| row.get(0))
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(storage_error)?;
        validate_car(&car).map_err(|_| IngestionError::RegistryIntegrity)?;
        let identity_version = version("car", &(&car.vehicle, &car.owner))?;
        let assignment_version = version("assignment", &(&car.vehicle, &car.chargers))?;
        let identity: String = connection
            .query_row(
                "SELECT config_json FROM car_identity_versions WHERE version = ?1 AND vehicle = ?2",
                params![identity_version, car.vehicle],
                |row| row.get(0),
            )
            .map_err(|_| IngestionError::RegistryIntegrity)?;
        let assignment: String = connection
            .query_row(
                "SELECT config_json FROM assignment_versions WHERE version = ?1 AND vehicle = ?2",
                params![assignment_version, car.vehicle],
                |row| row.get(0),
            )
            .map_err(|_| IngestionError::RegistryIntegrity)?;
        if identity != json(&(&car.device, &car.vehicle, &car.owner))?
            || assignment != json(&(&car.vehicle, &car.chargers))?
        {
            return Err(IngestionError::RegistryIntegrity);
        }
        versions
            .entry(car.vehicle.clone())
            .or_default()
            .insert(identity_version.clone());
        config.mappings.push(DeviceMapping {
            device: car.vehicle.clone(),
            vehicle: car.vehicle.clone(),
            owner: car.owner.clone(),
        });
        cars.push(Car {
            registration: car,
            active,
            identity_version,
            assignment_version,
        });
    }
    let legacy: Option<String> = connection
        .query_row(
            "SELECT manifest_json FROM legacy_registry_contract WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let mut legacy_charger = None;
    if let Some(legacy) = legacy {
        let retained = Manifest::from_json(legacy.as_bytes())
            .map_err(|_| IngestionError::RegistryIntegrity)?;
        for mapping in &retained.config().mappings {
            versions
                .entry(mapping.vehicle.clone())
                .or_default()
                .insert(retained.config().mapping_version.clone());
        }
        legacy_charger = retained
            .config()
            .owner_chargers
            .first()
            .map(|charger| charger.charger_id.clone());
    }
    let mut manifest = Manifest::new(config).map_err(|_| IngestionError::RegistryIntegrity)?;
    manifest.live_identity_versions = versions;
    Ok((
        RegistrySnapshot {
            cars,
            chargers,
            legacy_charger,
            assignments,
            geofence_versions,
        },
        manifest,
    ))
}

pub fn migrate_registry(
    database: &Path,
    bootstrap: &RegistryBootstrap,
    setup: &RegistrySetup,
) -> Result<(), IngestionError> {
    if database != bootstrap.settings.database
        || !text_valid(&setup.charger_name)
        || OwnerId::new(&setup.charger_owner).is_err()
        || setup.car_names.len() != bootstrap.mappings.len()
        || setup.car_names.values().any(|name| !text_valid(name))
    {
        return Err(IngestionError::RegistryConflict);
    }
    bootstrap.settings.validate_database_location()?;
    if database.exists() {
        let mut store = Store::open_registry_database(database)?;
        if store.registry.is_some() {
            return Ok(());
        }
        migrate(&mut store, bootstrap, setup, &mut |_| Ok(()))
    } else {
        let source = crate::kafka::live_source(&bootstrap.settings)?;
        let mut store = Store::open(database, &bootstrap.legacy_manifest()?, &source)?;
        migrate(&mut store, bootstrap, setup, &mut |_| Ok(()))
    }
}

pub(crate) fn migrate(
    store: &mut Store,
    bootstrap: &RegistryBootstrap,
    setup: &RegistrySetup,
    observer: &mut dyn FnMut(crate::CommitBoundary) -> Result<(), IngestionError>,
) -> Result<(), IngestionError> {
    let expected = bootstrap.legacy_manifest()?;
    if store.manifest.config() != expected.config()
        || store.source_epoch() != bootstrap.source_epoch
    {
        return Err(IngestionError::RegistryConflict);
    }
    if store.source_topic() != format!("{}_V", bootstrap.topic_namespace) {
        return Err(IngestionError::SourceMismatch);
    }
    let legacy_json = json(expected.config())?;
    let fixed = bootstrap.settings.manifest()?;
    let fixed_json = json(fixed.config())?;
    let transaction = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage_error)?;
    transaction.execute_batch("ALTER TABLE registrations RENAME TO cars;
        ALTER TABLE cars ADD COLUMN name TEXT NOT NULL DEFAULT 'Car' CHECK(length(trim(name)) > 0);
        ALTER TABLE cars ADD COLUMN make TEXT;
        ALTER TABLE cars ADD COLUMN model TEXT;
        ALTER TABLE cars ADD COLUMN year INTEGER CHECK(year BETWEEN 1886 AND 9999);
        ALTER TABLE cars ADD COLUMN created_at TEXT NOT NULL DEFAULT '';
        ALTER TABLE cars ADD COLUMN active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1));
        CREATE TABLE chargers(id TEXT PRIMARY KEY, name TEXT NOT NULL CHECK(length(trim(name)) > 0), location TEXT NOT NULL, radius INTEGER NOT NULL CHECK(radius > 0), owner TEXT NOT NULL, rate_plan TEXT NOT NULL, geofence_version TEXT NOT NULL) STRICT;
        CREATE TABLE car_chargers(car_id TEXT NOT NULL REFERENCES cars(device), charger_id TEXT NOT NULL REFERENCES chargers(id), PRIMARY KEY(car_id, charger_id)) STRICT;
        CREATE TABLE car_identity_versions(version TEXT PRIMARY KEY, vehicle TEXT NOT NULL REFERENCES cars(vehicle), config_json TEXT NOT NULL) STRICT;
        CREATE TABLE assignment_versions(version TEXT PRIMARY KEY, vehicle TEXT NOT NULL REFERENCES cars(vehicle), config_json TEXT NOT NULL) STRICT;
        CREATE TABLE charger_geofence_versions(version TEXT PRIMARY KEY, charger_id TEXT NOT NULL REFERENCES chargers(id), config_json TEXT NOT NULL) STRICT;
        CREATE TABLE legacy_registry_contract(singleton INTEGER PRIMARY KEY CHECK(singleton = 1), manifest_json TEXT NOT NULL) STRICT;
        CREATE TABLE registry_runtime_settings(singleton INTEGER PRIMARY KEY CHECK(singleton = 1), settings_json TEXT NOT NULL) STRICT;
        CREATE TABLE live_delivery_admissions(partition INTEGER NOT NULL, offset INTEGER NOT NULL, admitted_vehicles_json TEXT NOT NULL, PRIMARY KEY(partition,offset), FOREIGN KEY(partition,offset) REFERENCES deliveries(partition,offset)) STRICT;
        CREATE TABLE recovery_jobs(vehicle TEXT PRIMARY KEY REFERENCES cars(vehicle), source_json TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending','scanning','complete'))) STRICT;
        CREATE TABLE recovery_progress(vehicle TEXT NOT NULL REFERENCES recovery_jobs(vehicle), partition INTEGER NOT NULL REFERENCES partition_progress(partition), low INTEGER NOT NULL CHECK(low >= 0), high INTEGER NOT NULL CHECK(high >= low), next_offset INTEGER NOT NULL CHECK(next_offset BETWEEN low AND high), PRIMARY KEY(vehicle,partition)) STRICT;
        CREATE TABLE recovery_evidence(vehicle TEXT NOT NULL REFERENCES recovery_jobs(vehicle), partition INTEGER NOT NULL, offset INTEGER NOT NULL, variant_id INTEGER REFERENCES event_variants(id), disposition_json TEXT NOT NULL, PRIMARY KEY(vehicle,partition,offset), FOREIGN KEY(vehicle,partition) REFERENCES recovery_progress(vehicle,partition)) STRICT;
        ALTER TABLE runtime_metadata RENAME TO legacy_runtime_metadata;
        CREATE TABLE runtime_metadata(singleton INTEGER PRIMARY KEY CHECK(singleton = 1), schema_version INTEGER NOT NULL CHECK(schema_version = 4), manifest_json TEXT NOT NULL, source_json TEXT NOT NULL) STRICT;
        INSERT INTO runtime_metadata SELECT singleton,4,manifest_json,source_json FROM legacy_runtime_metadata;
        DROP TABLE legacy_runtime_metadata;
        PRAGMA user_version = 4;").map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO legacy_registry_contract VALUES (1, ?1)",
            [&legacy_json],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO registry_runtime_settings VALUES (1, ?1)",
            [json(&bootstrap.settings)?],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE runtime_metadata SET manifest_json = ?1",
            [&fixed_json],
        )
        .map_err(storage_error)?;
    let admissions = json(
        &expected
            .config()
            .mappings
            .iter()
            .map(|mapping| mapping.vehicle.clone())
            .collect::<Vec<_>>(),
    )?;
    transaction
        .execute(
            "INSERT INTO live_delivery_admissions SELECT partition,offset,?1 FROM deliveries",
            [&admissions],
        )
        .map_err(storage_error)?;
    let charger = &bootstrap.shared_charger;
    let geofence_version = version("geofence", &(&charger.charger_id, &charger.geofence))?;
    transaction
        .execute(
            "INSERT INTO chargers VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                charger.charger_id,
                setup.charger_name,
                json(&charger.geofence.center)?,
                charger.geofence.radius_meters,
                setup.charger_owner,
                json(&charger.rate_model_versions)?,
                geofence_version
            ],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO charger_geofence_versions VALUES (?1,?2,?3)",
            params![
                geofence_version,
                charger.charger_id,
                json(&charger.geofence)?
            ],
        )
        .map_err(storage_error)?;
    for mapping in &bootstrap.mappings {
        let name = setup
            .car_names
            .get(&mapping.vehicle)
            .filter(|name| text_valid(name))
            .ok_or(IngestionError::RegistryConflict)?;
        let changed = transaction.execute("UPDATE cars SET device = ?1, name = ?2, created_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE device = ?3 AND vehicle = ?3 AND owner = ?4", params![mapping.device, name, mapping.vehicle, mapping.owner]).map_err(|_| IngestionError::RegistryConflict)?;
        if changed != 1 {
            return Err(IngestionError::RegistryConflict);
        }
        transaction
            .execute(
                "INSERT INTO car_chargers VALUES (?1,?2)",
                params![mapping.device, charger.charger_id],
            )
            .map_err(storage_error)?;
        record_identity(
            &transaction,
            &CarRegistration {
                device: mapping.device.clone(),
                vehicle: mapping.vehicle.clone(),
                owner: mapping.owner.clone(),
                name: name.clone(),
                make: None,
                model: None,
                year: None,
                chargers: vec![charger.charger_id.clone()],
            },
        )?;
    }
    crate::charger_rates::migrate(&transaction)?;
    load(&transaction, &fixed)?;
    observer(crate::CommitBoundary::BeforeCommit)?;
    transaction.commit().map_err(storage_error)?;
    observer(crate::CommitBoundary::AfterCommit)?;
    store.refresh_registry(&fixed)?;
    Ok(())
}

impl Store {
    pub(crate) fn refresh_registry(&mut self, fixed: &Manifest) -> Result<(), IngestionError> {
        let (registry, manifest) = load(&self.connection, fixed)?;
        self.registry = Some(registry);
        self.manifest = manifest;
        self.manifest_json = json(fixed.config())?;
        Ok(())
    }

    pub fn add_car(
        database: &Path,
        mut registration: CarRegistration,
    ) -> Result<RegistryWrite, IngestionError> {
        validate_car(&registration)?;
        registration.chargers.sort();
        registration.chargers.dedup();
        let mut store = Self::open_registry_database(database)?;
        let registry = store
            .registry
            .as_ref()
            .ok_or(IngestionError::MigrationInputRequired)?;
        if registration
            .chargers
            .iter()
            .any(|id| !registry.chargers.contains_key(id))
        {
            return Err(IngestionError::RegistryConflict);
        }
        if let Some(car) = registry.cars.iter().find(|car| {
            car.registration.device == registration.device
                || car.registration.vehicle == registration.vehicle
        }) {
            if car.registration != registration || !car.active {
                return Err(IngestionError::RegistryConflict);
            }
            let complete: bool = store
                .connection
                .query_row(
                    "SELECT state = 'complete' FROM recovery_jobs WHERE vehicle = ?1",
                    [&registration.vehicle],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?
                .unwrap_or(true);
            return Ok(if complete {
                RegistryWrite::RecoveryComplete
            } else {
                RegistryWrite::RecoveryPending
            });
        }
        if registry.cars.len() >= crate::MAX_MAPPINGS {
            return Err(IngestionError::RegistryConflict);
        }
        let transaction = store
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        transaction.execute("INSERT INTO cars(device,vehicle,owner,name,make,model,year,created_at,active) VALUES (?1,?2,?3,?4,?5,?6,?7,strftime('%Y-%m-%dT%H:%M:%SZ','now'),1)", params![registration.device, registration.vehicle, registration.owner, registration.name, registration.make, registration.model, registration.year]).map_err(|_| IngestionError::RegistryConflict)?;
        for charger in &registration.chargers {
            transaction
                .execute(
                    "INSERT INTO car_chargers VALUES (?1,?2)",
                    params![registration.device, charger],
                )
                .map_err(storage_error)?;
        }
        record_identity(&transaction, &registration)?;
        transaction
            .execute(
                "INSERT INTO recovery_jobs SELECT ?1,source_json,'pending' FROM runtime_metadata",
                [&registration.vehicle],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(RegistryWrite::RecoveryPending)
    }

    pub fn rename_car(
        database: &Path,
        vehicle: &str,
        name: &str,
    ) -> Result<RegistryWrite, IngestionError> {
        if VehicleId::new(vehicle).is_err() || !text_valid(name) {
            return Err(IngestionError::RegistryConflict);
        }
        let store = Self::open_registry_database(database)?;
        if store.registry.is_none() {
            return Err(IngestionError::MigrationInputRequired);
        }
        if store
            .connection
            .execute(
                "UPDATE cars SET name = ?1 WHERE vehicle = ?2",
                params![name, vehicle],
            )
            .map_err(storage_error)?
            != 1
        {
            return Err(IngestionError::RegistryConflict);
        }
        Ok(RegistryWrite::Updated)
    }

    pub fn remove_car(database: &Path, vehicle: &str) -> Result<RegistryWrite, IngestionError> {
        if VehicleId::new(vehicle).is_err() {
            return Err(IngestionError::RegistryConflict);
        }
        let store = Self::open_registry_database(database)?;
        if store.registry.is_none() {
            return Err(IngestionError::MigrationInputRequired);
        }
        if store
            .connection
            .execute("UPDATE cars SET active = 0 WHERE vehicle = ?1", [vehicle])
            .map_err(storage_error)?
            != 1
        {
            return Err(IngestionError::RegistryConflict);
        }
        Ok(RegistryWrite::Updated)
    }
}

impl RegistrySnapshot {
    pub(crate) fn car(&self, vehicle: &str) -> Result<&Car, IngestionError> {
        self.cars
            .iter()
            .find(|car| car.registration.vehicle == vehicle)
            .ok_or(IngestionError::RegistryIntegrity)
    }

    pub(crate) fn validate_location(
        &self,
        vehicle: &str,
        location: &crate::NormalizedLocation,
    ) -> Result<(), IngestionError> {
        if let crate::NormalizedLocation::MatchedCharger {
            charger_id,
            assignment_version,
            geofence_version,
        } = location
        {
            let assignment = self
                .assignments
                .get(assignment_version)
                .ok_or(IngestionError::RegistryIntegrity)?;
            let geofence = self
                .geofence_versions
                .get(geofence_version)
                .ok_or(IngestionError::RegistryIntegrity)?;
            if assignment.0 != vehicle
                || !assignment.1.contains(charger_id)
                || &geofence.0 != charger_id
            {
                return Err(IngestionError::RegistryIntegrity);
            }
        }
        if let crate::NormalizedLocation::OutsideChargers {
            assignment_version,
            geofence_versions,
        }
        | crate::NormalizedLocation::AmbiguousChargers {
            assignment_version,
            geofence_versions,
        } = location
        {
            let assignment = self
                .assignments
                .get(assignment_version)
                .ok_or(IngestionError::RegistryIntegrity)?;
            if assignment.0 != vehicle
                || geofence_versions.is_empty()
                || geofence_versions.iter().any(|version| {
                    self.geofence_versions
                        .get(version)
                        .is_none_or(|(charger, _)| !assignment.1.contains(charger))
                })
            {
                return Err(IngestionError::RegistryIntegrity);
            }
        }
        Ok(())
    }
}

impl Store {
    pub(crate) fn connection_chargers(
        &self,
        vehicle: &str,
    ) -> Result<BTreeMap<String, BTreeSet<String>>, IngestionError> {
        let mut matches = BTreeMap::<String, BTreeSet<String>>::new();
        let Some(registry) = &self.registry else {
            return Ok(matches);
        };
        let mut statement = self
            .connection
            .prepare("SELECT event_json FROM event_variants WHERE vehicle = ?1")
            .map_err(storage_error)?;
        let events = statement
            .query_map([vehicle], |row| row.get::<_, String>(0))
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)?;
        for json in events {
            let event: crate::NormalizedEvent =
                serde_json::from_str(&json).map_err(|_| IngestionError::RegistryIntegrity)?;
            if let Some(location) = &event.location {
                registry.validate_location(vehicle, location)?;
            }
            match &event.location {
                Some(crate::NormalizedLocation::MatchedCharger { charger_id, .. }) => {
                    registry.validate_location(vehicle, event.location.as_ref().unwrap())?;
                    matches
                        .entry(event.connection)
                        .or_default()
                        .insert(charger_id.clone());
                }
                Some(crate::NormalizedLocation::AmbiguousChargers {
                    geofence_versions, ..
                }) => {
                    for version in geofence_versions {
                        let (id, _) = registry
                            .geofence_versions
                            .get(version)
                            .ok_or(IngestionError::RegistryIntegrity)?;
                        matches
                            .entry(event.connection.clone())
                            .or_default()
                            .insert(id.clone());
                    }
                }
                Some(crate::NormalizedLocation::InsideGeofence) => {
                    let id = registry
                        .legacy_charger
                        .as_ref()
                        .ok_or(IngestionError::RegistryIntegrity)?;
                    matches
                        .entry(event.connection)
                        .or_default()
                        .insert(id.clone());
                }
                _ => {}
            }
        }
        Ok(matches)
    }

    pub(crate) fn active_scopes(&self) -> Result<Vec<serde_json::Value>, IngestionError> {
        self.validate_live_storage()?;
        if let Some(registry) = &self.registry {
            let pending = self.pending_recoveries()?;
            Ok(registry.cars.iter().filter(|car| car.active && !pending.contains(&car.registration.vehicle)).map(|car| {
                let chargers: Vec<_> = car.registration.chargers.iter().filter_map(|id| registry.chargers.get(id)).map(|charger| serde_json::json!({"id":charger.id,"name":charger.name})).collect();
                serde_json::json!({"owner":car.registration.owner,"vehicle":car.registration.vehicle,"name":car.registration.name,"chargers":chargers})
            }).collect())
        } else {
            Ok(self.manifest.config().mappings.iter().enumerate().map(|(index,mapping)| {
                let chargers: Vec<_> = self.manifest.config().owner_chargers.iter().filter(|charger| charger.owner_id == mapping.owner).map(|charger| serde_json::json!({"id":charger.charger_id,"name":"Sample charger"})).collect();
                serde_json::json!({"owner":mapping.owner,"vehicle":mapping.vehicle,"name":format!("Sample car {}",index + 1),"chargers":chargers})
            }).collect())
        }
    }
}

#[cfg(test)]
mod tests;
