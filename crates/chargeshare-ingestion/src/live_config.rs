use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use chargeshare_core::{ChargerGeofence, VehicleId};
use serde::{Deserialize, Serialize};

use crate::live::{LIVE_INPUT_CONTRACT, LIVE_NORMALIZER_VERSION, fingerprint};
use crate::{
    ChargerGeofenceConfig, DeviceMapping, IngestionError, KafkaConfig, Manifest, ManifestConfig,
    NormalizedLocation, OwnerChargerConfig, RateModelConfig, Store, TIMESTAMP_VERSION,
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LiveConfig {
    pub(crate) kafka_bootstrap: String,
    pub(crate) topic_namespace: String,
    pub(crate) source_epoch: String,
    pub(crate) database: PathBuf,
    pub(crate) mappings: Vec<DeviceMapping>,
    shared_charger: SharedCharger,
    rate_models: Vec<RateModelConfig>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SharedCharger {
    charger_id: String,
    geofence: ChargerGeofenceConfig,
    rate_model_versions: Vec<String>,
}

impl LiveConfig {
    pub(crate) fn from_json(bytes: &[u8]) -> Result<Self, IngestionError> {
        if bytes.len() > crate::MAX_MANIFEST_BYTES {
            return Err(IngestionError::InvalidConfiguration);
        }
        let config: Self =
            serde_json::from_slice(bytes).map_err(|_| IngestionError::InvalidConfiguration)?;
        if config.kafka_bootstrap.is_empty()
            || config.kafka_bootstrap.len() > 2048
            || config
                .kafka_bootstrap
                .bytes()
                .any(|byte| byte.is_ascii_control())
            || !config.source_epoch.starts_with("live-")
            || config.topic_namespace.len() > 60
            || VehicleId::new(&config.topic_namespace).is_err()
            || VehicleId::new(&config.source_epoch).is_err()
            || !config.database.is_absolute()
            || config.database.file_name().is_none()
            || config
                .database
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(IngestionError::InvalidConfiguration);
        }
        let mut devices = BTreeSet::new();
        for mapping in &config.mappings {
            if mapping.device.is_empty()
                || mapping.device.len() > 64
                || !devices.insert(&mapping.device)
            {
                return Err(IngestionError::InvalidConfiguration);
            }
        }
        config.geofence()?;
        config.manifest()?;
        Ok(config)
    }

    pub(crate) fn geofence(&self) -> Result<ChargerGeofence, IngestionError> {
        self.shared_charger
            .geofence
            .to_core()
            .map_err(|_| IngestionError::InvalidConfiguration)
    }

    pub(crate) fn manifest(&self) -> Result<Manifest, IngestionError> {
        let mut mappings = self.mappings.clone();
        mappings.sort_by(|left, right| left.device.cmp(&right.device));
        let contract = serde_json::to_vec(&(&mappings, &self.shared_charger.geofence))
            .map_err(|_| IngestionError::InvalidConfiguration)?;
        let version = format!("live-config-{:016x}", fingerprint(&contract));
        let owners: BTreeSet<_> = mappings
            .iter()
            .map(|mapping| mapping.owner.clone())
            .collect();
        for mapping in &mut mappings {
            mapping.device = mapping.vehicle.clone();
        }
        Manifest::new(ManifestConfig {
            input_contract: LIVE_INPUT_CONTRACT.to_owned(),
            normalizer_version: LIVE_NORMALIZER_VERSION.to_owned(),
            timestamp_version: TIMESTAMP_VERSION.to_owned(),
            mapping_version: version.clone(),
            manifest_version: "live-manifest-v1".to_owned(),
            geofence_version: version,
            mappings,
            annotations: Vec::new(),
            geofences: Vec::new(),
            owner_chargers: owners
                .into_iter()
                .map(|owner_id| OwnerChargerConfig {
                    owner_id,
                    charger_id: self.shared_charger.charger_id.clone(),
                    geofence: ChargerGeofenceConfig {
                        center: NormalizedLocation::RedactedGeofence,
                        radius_meters: self.shared_charger.geofence.radius_meters,
                    },
                    rate_model_versions: self.shared_charger.rate_model_versions.clone(),
                })
                .collect(),
            rate_models: self.rate_models.clone(),
        })
        .map_err(|_| IngestionError::InvalidConfiguration)
    }

    pub(crate) fn kafka_config(&self) -> KafkaConfig {
        KafkaConfig {
            bootstrap_servers: self.kafka_bootstrap.clone(),
            source_epoch_path: self.database.parent().unwrap().join("source-epoch.json"),
            max_records: 1,
            deadline: Duration::from_secs(5),
        }
    }
}

pub fn run_live(bytes: &[u8]) -> Result<(), IngestionError> {
    let config = LiveConfig::from_json(bytes)?;
    validate_database_location(&config)?;
    let manifest = config.manifest()?;
    let source = crate::kafka::live_source(&config)?;
    let store = Store::open(&config.database, &manifest, &source)?;
    let driver = crate::kafka::LiveIngestion::connect_live(config, source, &store)?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 8788))
        .map_err(|_| IngestionError::StorageUnavailable)?;
    crate::viewing::serve_with_driver(listener, store, driver, &AtomicBool::new(false))
}

fn validate_database_location(config: &LiveConfig) -> Result<(), IngestionError> {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or(IngestionError::InvalidConfiguration)?;
    if config.database.starts_with(checkout) {
        return Err(IngestionError::InvalidConfiguration);
    }
    Ok(())
}

pub fn rebuild_live(original_bytes: &[u8], replacement_bytes: &[u8]) -> Result<(), IngestionError> {
    let original = LiveConfig::from_json(original_bytes)?;
    let replacement = LiveConfig::from_json(replacement_bytes)?;
    validate_database_location(&original)?;
    validate_database_location(&replacement)?;
    if original.database.parent() == replacement.database.parent()
        || original.source_epoch == replacement.source_epoch
        || original.topic_namespace != replacement.topic_namespace
        || original.mappings != replacement.mappings
    {
        return Err(IngestionError::ConfigurationMismatch);
    }
    let source = crate::read_source_marker(&original.kafka_config().source_epoch_path)?;
    if source.epoch != original.source_epoch
        || source.topic != format!("{}_V", original.topic_namespace)
    {
        return Err(IngestionError::SourceMismatch);
    }
    let manifest = original.manifest()?;
    let (_preserved, required) =
        Store::preserve_live_source(&original.database, &manifest, &source)?;
    crate::kafka::LiveIngestion::rebuild(replacement, &source, &required)
}
