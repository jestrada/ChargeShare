use std::collections::BTreeSet;

use chargeshare_core::ChargerGeofence;
use serde::{Deserialize, Serialize};

use crate::live::{LIVE_INPUT_CONTRACT, LIVE_NORMALIZER_VERSION, fingerprint};
use crate::{
    ChargerGeofenceConfig, DeviceMapping, IngestionError, Manifest, ManifestConfig,
    NormalizedLocation, OwnerChargerConfig, Store, TIMESTAMP_VERSION,
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryBootstrap {
    #[serde(flatten)]
    pub(crate) settings: crate::runtime_settings::RuntimeSettings,
    pub(crate) mappings: Vec<DeviceMapping>,
    pub(crate) shared_charger: SharedCharger,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SharedCharger {
    pub(crate) charger_id: String,
    pub(crate) geofence: ChargerGeofenceConfig,
    pub(crate) rate_model_versions: Vec<String>,
}

impl std::ops::Deref for RegistryBootstrap {
    type Target = crate::runtime_settings::RuntimeSettings;

    fn deref(&self) -> &Self::Target {
        &self.settings
    }
}

impl RegistryBootstrap {
    pub fn from_json(bytes: &[u8]) -> Result<Self, IngestionError> {
        if bytes.len() > crate::MAX_MANIFEST_BYTES {
            return Err(IngestionError::InvalidConfiguration);
        }
        let mut config: Self =
            serde_json::from_slice(bytes).map_err(|_| IngestionError::InvalidConfiguration)?;
        config.settings.normalize_rates()?;
        config.settings.validate()?;
        let mut devices = BTreeSet::new();
        for mapping in &config.mappings {
            if mapping.device.trim().is_empty()
                || mapping.device.chars().any(char::is_control)
                || mapping.device.len() > 64
                || !devices.insert(&mapping.device)
            {
                return Err(IngestionError::InvalidConfiguration);
            }
        }
        config.geofence()?;
        config.legacy_manifest()?;
        Ok(config)
    }

    pub(crate) fn decoder_registry(
        &self,
    ) -> Result<crate::registry::RegistrySnapshot, IngestionError> {
        let charger = crate::registry::Charger {
            id: self.shared_charger.charger_id.clone(),
            name: "Legacy charger".to_owned(),
            owner: "legacy-owner".to_owned(),
            geofence: self.shared_charger.geofence.clone(),
            geofence_version: self.legacy_manifest()?.config().geofence_version.clone(),
            rate_plan: self.shared_charger.rate_model_versions.clone(),
        };
        let cars = self
            .mappings
            .iter()
            .map(|mapping| crate::registry::Car {
                registration: crate::registry::CarRegistration {
                    device: mapping.device.clone(),
                    vehicle: mapping.vehicle.clone(),
                    owner: mapping.owner.clone(),
                    name: "Legacy car".to_owned(),
                    make: None,
                    model: None,
                    year: None,
                    chargers: vec![charger.id.clone()],
                },
                active: true,
                identity_version: self
                    .legacy_manifest()
                    .unwrap()
                    .config()
                    .mapping_version
                    .clone(),
                assignment_version: "legacy-assignment-v1".to_owned(),
            })
            .collect();
        Ok(crate::registry::RegistrySnapshot {
            cars,
            chargers: [(charger.id.clone(), charger)].into(),
            legacy_charger: Some(self.shared_charger.charger_id.clone()),
            assignments: std::collections::BTreeMap::new(),
            geofence_versions: std::collections::BTreeMap::new(),
        })
    }

    pub(crate) fn geofence(&self) -> Result<ChargerGeofence, IngestionError> {
        self.shared_charger
            .geofence
            .to_core()
            .map_err(|_| IngestionError::InvalidConfiguration)
    }

    pub(crate) fn legacy_manifest(&self) -> Result<Manifest, IngestionError> {
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
}

pub fn rebuild_live(original_bytes: &[u8], replacement_bytes: &[u8]) -> Result<(), IngestionError> {
    let original = RegistryBootstrap::from_json(original_bytes)?;
    let replacement = RegistryBootstrap::from_json(replacement_bytes)?;
    original.settings.validate_database_location()?;
    replacement.settings.validate_database_location()?;
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
    let manifest = original.legacy_manifest()?;
    let (_preserved, required) =
        Store::preserve_live_source(&original.database, &manifest, &source)?;
    crate::kafka::LiveIngestion::rebuild(replacement, &source, &required)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config_json() -> serde_json::Value {
        json!({
            "kafka_bootstrap": "kafka:9092",
            "topic_namespace": "unit",
            "source_epoch": "live-unit-v2",
            "database": "/private/unit/ingestion.sqlite3",
            "mappings": [{"device": "device-a", "vehicle": "vehicle-a", "owner": "owner-a"}],
            "shared_charger": {
                "charger_id": "charger-a",
                "geofence": {"center": {"status": "valid", "latitude": "0", "longitude": "0"}},
                "rate_model_versions": []
            },
            "rate_models": []
        })
    }

    #[test]
    fn api_port_defaults_and_overrides_preserve_the_evidence_manifest() {
        let mut json = config_json();
        let original = RegistryBootstrap::from_json(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(original.api_port, 8788);
        let original_manifest = original.legacy_manifest().unwrap();
        for port in [1, 8789, u16::MAX] {
            json["api_port"] = json!(port);
            let config = RegistryBootstrap::from_json(&serde_json::to_vec(&json).unwrap()).unwrap();
            assert_eq!(config.api_port, port);
            assert_eq!(
                config.legacy_manifest().unwrap().config(),
                original_manifest.config()
            );
        }
    }

    #[test]
    fn api_port_rejects_zero_out_of_range_and_noninteger_values() {
        for port in [
            json!(0),
            json!(-1),
            json!(65536),
            json!(8788.5),
            json!("8788"),
            json!(true),
            json!(null),
        ] {
            let mut json = config_json();
            json["api_port"] = port;
            assert!(matches!(
                RegistryBootstrap::from_json(&serde_json::to_vec(&json).unwrap()),
                Err(IngestionError::InvalidConfiguration)
            ));
        }
    }
}
