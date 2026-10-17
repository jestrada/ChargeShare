use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use chargeshare_core::VehicleId;
use serde::{Deserialize, Serialize};

use crate::{
    IngestionError, KafkaConfig, Manifest, ManifestConfig, RateModelConfig, Store,
    TIMESTAMP_VERSION,
};

#[derive(Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSettings {
    pub(crate) kafka_bootstrap: String,
    pub(crate) topic_namespace: String,
    pub(crate) source_epoch: String,
    pub(crate) database: PathBuf,
    #[serde(default = "default_api_port")]
    pub(crate) api_port: u16,
    #[serde(default, skip_serializing)]
    pub(crate) private_car_images_dir: Option<PathBuf>,
    pub(crate) rate_models: Vec<RateModelConfig>,
}

fn default_api_port() -> u16 {
    8788
}

impl RuntimeSettings {
    pub fn from_json(bytes: &[u8]) -> Result<Self, IngestionError> {
        if bytes.len() > crate::MAX_MANIFEST_BYTES {
            return Err(IngestionError::InvalidConfiguration);
        }
        let mut value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| IngestionError::InvalidConfiguration)?;
        let object = value
            .as_object_mut()
            .ok_or(IngestionError::InvalidConfiguration)?;
        object.remove("mappings");
        object.remove("shared_charger");
        object.remove(crate::registry_bootstrap::STARTUP_SETUP_SECTION);
        let mut settings: Self =
            serde_json::from_value(value).map_err(|_| IngestionError::InvalidConfiguration)?;
        settings.normalize_rates()?;
        settings.validate()?;
        Ok(settings)
    }

    pub(crate) fn normalize_rates(&mut self) -> Result<(), IngestionError> {
        self.rate_models = self.manifest()?.config().rate_models.clone();
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), IngestionError> {
        if self.api_port == 0
            || self.kafka_bootstrap.is_empty()
            || self.kafka_bootstrap.len() > 2048
            || self
                .kafka_bootstrap
                .bytes()
                .any(|byte| byte.is_ascii_control())
            || !self.source_epoch.starts_with("live-")
            || self.topic_namespace.len() > 60
            || VehicleId::new(&self.topic_namespace).is_err()
            || VehicleId::new(&self.source_epoch).is_err()
            || !self.database.is_absolute()
            || self.database.file_name().is_none()
            || self
                .database
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(IngestionError::InvalidConfiguration);
        }
        self.manifest()?;
        Ok(())
    }

    pub(crate) fn validate_database_location(&self) -> Result<(), IngestionError> {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or(IngestionError::InvalidConfiguration)?;
        if self.database.starts_with(checkout) {
            return Err(IngestionError::InvalidConfiguration);
        }
        Ok(())
    }

    pub(crate) fn manifest(&self) -> Result<Manifest, IngestionError> {
        Manifest::new(ManifestConfig {
            input_contract: crate::live::LIVE_INPUT_CONTRACT.to_owned(),
            normalizer_version: crate::live::LIVE_NORMALIZER_VERSION.to_owned(),
            timestamp_version: TIMESTAMP_VERSION.to_owned(),
            mapping_version: "registry-identity-v1".to_owned(),
            manifest_version: "live-manifest-v1".to_owned(),
            geofence_version: "registry-geofences-v1".to_owned(),
            mappings: Vec::new(),
            annotations: Vec::new(),
            geofences: Vec::new(),
            owner_chargers: Vec::new(),
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
    let settings = RuntimeSettings::from_json(bytes)?;
    settings.validate_database_location()?;
    crate::registry::migrate_on_startup(bytes, &settings, &mut |_| Ok(()))?;
    let source = crate::kafka::live_source(&settings)?;
    let mut store = Store::open_runtime(&settings, &source)?;
    let driver = crate::kafka::LiveIngestion::connect_live(settings.clone(), source, &mut store)?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, settings.api_port))
        .map_err(|_| IngestionError::StorageUnavailable)?;
    crate::viewing::serve_with_driver(
        listener,
        store,
        driver,
        &AtomicBool::new(false),
        settings.private_car_images_dir.as_deref(),
    )
}
