use std::collections::BTreeSet;

use serde::Deserialize;

use crate::manifest::{Manifest, parse_utc_seconds};
use crate::model::{
    AcceptedEvidence, EvidenceProvenance, MAX_PAYLOAD_BYTES, NormalizedCounter, NormalizedEvent,
    RejectionReason,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiverPayload {
    data: Vec<ReceiverField>,
    created_at: String,
    vin: Option<String>,
    is_resend: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiverField {
    key: ReceiverFieldKey,
    value: ReceiverValue,
}

#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
enum ReceiverFieldKey {
    VehicleName,
    BatteryLevel,
    DetailedChargeState,
    ACChargingEnergyIn,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiverValue {
    string_value: Option<String>,
    detailed_charge_state_value: Option<ReceiverChargeState>,
}

#[derive(Deserialize)]
enum ReceiverChargeState {
    #[serde(rename = "DetailedChargeStateCharging")]
    Charging,
    #[serde(rename = "DetailedChargeStateComplete")]
    Complete,
}

pub fn normalize(
    manifest: &Manifest,
    broker_key: Option<&[u8]>,
    payload: &[u8],
) -> Result<AcceptedEvidence, RejectionReason> {
    if payload.len() > MAX_PAYLOAD_BYTES || broker_key.is_some_and(|key| key.len() > 64) {
        return Err(RejectionReason::InputTooLarge);
    }
    let broker_key = broker_key
        .filter(|key| !key.is_empty())
        .ok_or(RejectionReason::MissingIdentity)?;
    let payload: ReceiverPayload =
        serde_json::from_slice(payload).map_err(|_| RejectionReason::UnsupportedPayload)?;
    let device = payload
        .vin
        .as_deref()
        .filter(|device| !device.is_empty())
        .ok_or(RejectionReason::MissingIdentity)?;
    if broker_key != device.as_bytes() {
        return Err(RejectionReason::IdentityMismatch);
    }
    let mapping = manifest
        .mapping(device)
        .ok_or(RejectionReason::UnknownIdentity)?;
    let time = parse_utc_seconds(&payload.created_at)?;
    let annotation = manifest
        .annotation(device, time)
        .ok_or(RejectionReason::MissingAssociation)?;
    let counter = decode_counter(payload.data)?;
    let config = manifest.config();
    let evidence = AcceptedEvidence {
        event: NormalizedEvent {
            vehicle: mapping.vehicle.clone(),
            connection: annotation.connection.clone(),
            position: annotation.position.clone(),
            time,
            kind: annotation.kind,
            charge_type: annotation.charge_type,
            counter,
        },
        provenance: EvidenceProvenance {
            device: mapping.device.clone(),
            owner: mapping.owner.clone(),
            created_at: annotation.created_at.clone(),
            input_contract: config.input_contract.clone(),
            mapping_version: config.mapping_version.clone(),
            manifest_version: config.manifest_version.clone(),
            normalizer_version: config.normalizer_version.clone(),
            timestamp_version: config.timestamp_version.clone(),
            is_resend: payload.is_resend,
        },
    };
    manifest.validate_evidence(&evidence)?;
    Ok(evidence)
}

fn decode_counter(
    fields: Vec<ReceiverField>,
) -> Result<Option<NormalizedCounter>, RejectionReason> {
    let mut observed_keys = BTreeSet::new();
    let mut counter = None;
    for field in fields {
        if !observed_keys.insert(field.key) {
            return Err(RejectionReason::DuplicateSourceField);
        }
        match field.key {
            ReceiverFieldKey::VehicleName | ReceiverFieldKey::BatteryLevel => {
                if field.value.string_value.is_none()
                    || field.value.detailed_charge_state_value.is_some()
                {
                    return Err(RejectionReason::UnsupportedPayload);
                }
            }
            ReceiverFieldKey::DetailedChargeState => {
                if field.value.detailed_charge_state_value.is_none()
                    || field.value.string_value.is_some()
                {
                    return Err(RejectionReason::UnsupportedPayload);
                }
            }
            ReceiverFieldKey::ACChargingEnergyIn => {
                let value = field
                    .value
                    .string_value
                    .ok_or(RejectionReason::UnsupportedPayload)?;
                if field.value.detailed_charge_state_value.is_some() {
                    return Err(RejectionReason::UnsupportedPayload);
                }
                counter = Some(NormalizedCounter::parse_kwh(&value));
            }
        }
    }
    Ok(counter)
}
