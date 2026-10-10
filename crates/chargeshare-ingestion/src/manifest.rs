use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chargeshare_core::{ConnectionId, OwnerId, VehicleId};
use serde::{Deserialize, Serialize};

use crate::model::{
    AcceptedEvidence, INPUT_CONTRACT_VERSION, MAX_ANNOTATIONS, MAX_MANIFEST_BYTES, MAX_MAPPINGS,
    NORMALIZER_VERSION, NormalizedChargeType, NormalizedEventKind, RejectionReason,
    TIMESTAMP_VERSION, parse_position,
};

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceMapping {
    pub device: String,
    pub vehicle: String,
    pub owner: String,
}

impl fmt::Debug for DeviceMapping {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeviceMapping { aliases: <redacted> }")
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub device: String,
    pub created_at: String,
    pub position: String,
    pub connection: String,
    pub kind: NormalizedEventKind,
    pub charge_type: NormalizedChargeType,
}

impl fmt::Debug for Annotation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Annotation")
            .field("kind", &self.kind)
            .field("charge_type", &self.charge_type)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestConfig {
    pub input_contract: String,
    pub mapping_version: String,
    pub manifest_version: String,
    pub normalizer_version: String,
    pub timestamp_version: String,
    pub mappings: Vec<DeviceMapping>,
    pub annotations: Vec<Annotation>,
}

impl fmt::Debug for ManifestConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManifestConfig")
            .field("mappings", &self.mappings.len())
            .field("annotations", &self.annotations.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct Manifest {
    config: ManifestConfig,
    mapping_indices: BTreeMap<String, usize>,
    annotation_indices: BTreeMap<(String, i64), usize>,
}

impl Manifest {
    pub fn from_json(input: &[u8]) -> Result<Self, RejectionReason> {
        if input.len() > MAX_MANIFEST_BYTES {
            return Err(RejectionReason::InputTooLarge);
        }
        let config = serde_json::from_slice(input).map_err(|_| RejectionReason::InvalidManifest)?;
        Self::new(config)
    }

    pub fn new(mut config: ManifestConfig) -> Result<Self, RejectionReason> {
        if config.mappings.is_empty()
            || config.mappings.len() > MAX_MAPPINGS
            || config.annotations.len() > MAX_ANNOTATIONS
        {
            return Err(RejectionReason::InvalidManifest);
        }
        for version in [
            &config.input_contract,
            &config.mapping_version,
            &config.manifest_version,
            &config.normalizer_version,
            &config.timestamp_version,
        ] {
            OwnerId::new(version).map_err(|_| RejectionReason::InvalidManifest)?;
        }
        if config.input_contract != INPUT_CONTRACT_VERSION
            || config.normalizer_version != NORMALIZER_VERSION
            || config.timestamp_version != TIMESTAMP_VERSION
        {
            return Err(RejectionReason::IncompatibleVersion);
        }
        config
            .mappings
            .sort_by(|left, right| left.device.cmp(&right.device));
        config.annotations.sort_by(|left, right| {
            (&left.device, &left.created_at).cmp(&(&right.device, &right.created_at))
        });
        let mut mapping_indices = BTreeMap::new();
        let mut vehicles = BTreeSet::new();
        for (index, mapping) in config.mappings.iter().enumerate() {
            VehicleId::new(&mapping.device).map_err(|_| RejectionReason::InvalidManifest)?;
            VehicleId::new(&mapping.vehicle).map_err(|_| RejectionReason::InvalidManifest)?;
            OwnerId::new(&mapping.owner).map_err(|_| RejectionReason::InvalidManifest)?;
            if mapping_indices
                .insert(mapping.device.clone(), index)
                .is_some()
                || !vehicles.insert(mapping.vehicle.clone())
            {
                return Err(RejectionReason::DuplicateMapping);
            }
        }
        let mut annotation_indices = BTreeMap::new();
        for (index, annotation) in config.annotations.iter().enumerate() {
            VehicleId::new(&annotation.device).map_err(|_| RejectionReason::InvalidManifest)?;
            ConnectionId::new(&annotation.connection)
                .map_err(|_| RejectionReason::InvalidManifest)?;
            parse_position(&annotation.position)?;
            let time = parse_utc_seconds(&annotation.created_at)
                .map_err(|_| RejectionReason::InvalidManifest)?;
            if !mapping_indices.contains_key(&annotation.device) {
                return Err(RejectionReason::InvalidManifest);
            }
            if annotation_indices
                .insert((annotation.device.clone(), time), index)
                .is_some()
            {
                return Err(RejectionReason::DuplicateAssociation);
            }
        }
        Ok(Self {
            config,
            mapping_indices,
            annotation_indices,
        })
    }

    pub fn config(&self) -> &ManifestConfig {
        &self.config
    }

    pub fn validate_evidence(&self, evidence: &AcceptedEvidence) -> Result<(), RejectionReason> {
        evidence.event.to_core()?;
        let provenance = &evidence.provenance;
        let mapping = self
            .mapping(&provenance.device)
            .ok_or(RejectionReason::InvalidEvidence)?;
        let time = parse_utc_seconds(&provenance.created_at)
            .map_err(|_| RejectionReason::InvalidEvidence)?;
        let annotation = self
            .annotation(&mapping.device, time)
            .ok_or(RejectionReason::InvalidEvidence)?;
        let config = &self.config;
        if provenance.owner != mapping.owner
            || provenance.input_contract != config.input_contract
            || provenance.mapping_version != config.mapping_version
            || provenance.manifest_version != config.manifest_version
            || provenance.normalizer_version != config.normalizer_version
            || provenance.timestamp_version != config.timestamp_version
            || provenance.created_at != annotation.created_at
            || evidence.event.vehicle != mapping.vehicle
            || evidence.event.position != annotation.position
            || evidence.event.connection != annotation.connection
            || evidence.event.kind != annotation.kind
            || evidence.event.charge_type != annotation.charge_type
            || evidence.event.time != time
        {
            return Err(RejectionReason::InvalidEvidence);
        }
        Ok(())
    }

    pub(crate) fn mapping(&self, device: &str) -> Option<&DeviceMapping> {
        self.mapping_indices
            .get(device)
            .map(|index| &self.config.mappings[*index])
    }

    pub(crate) fn annotation(&self, device: &str, time: i64) -> Option<&Annotation> {
        self.annotation_indices
            .get(&(device.to_owned(), time))
            .map(|index| &self.config.annotations[*index])
    }
}

impl fmt::Debug for Manifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Manifest")
            .field(&self.config)
            .finish()
    }
}

pub(crate) fn parse_utc_seconds(input: &str) -> Result<i64, RejectionReason> {
    let bytes = input.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return Err(RejectionReason::InvalidTimestamp);
    }
    let year = parse_digits(&bytes[0..4])?;
    let month = parse_digits(&bytes[5..7])?;
    let day = parse_digits(&bytes[8..10])?;
    let hour = parse_digits(&bytes[11..13])?;
    let minute = parse_digits(&bytes[14..16])?;
    let second = parse_digits(&bytes[17..19])?;
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return Err(RejectionReason::InvalidTimestamp),
    };
    if year == 0 || day == 0 || day > days_in_month || hour > 23 || minute > 59 || second > 59 {
        return Err(RejectionReason::InvalidTimestamp);
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days_since_epoch = era * 146_097 + day_of_era - 719_468;
    Ok(days_since_epoch * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn parse_digits(bytes: &[u8]) -> Result<i64, RejectionReason> {
    bytes.iter().try_fold(0, |value, byte| {
        if !byte.is_ascii_digit() {
            return Err(RejectionReason::InvalidTimestamp);
        }
        Ok(value * 10 + i64::from(byte - b'0'))
    })
}
