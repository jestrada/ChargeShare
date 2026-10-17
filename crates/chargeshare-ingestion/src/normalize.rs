use std::collections::BTreeSet;

use serde::Deserialize;

use crate::manifest::{Manifest, parse_utc_seconds};
use crate::model::{
    AcceptedEvidence, EvidenceProvenance, MAX_PAYLOAD_BYTES, NormalizedCounter, NormalizedEvent,
    NormalizedLocation, RejectionReason,
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
    value: Box<serde_json::value::RawValue>,
}

#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
enum ReceiverFieldKey {
    VehicleName,
    BatteryLevel,
    DetailedChargeState,
    ACChargingEnergyIn,
    Location,
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
    let samples = decode_samples(payload.data)?;
    let config = manifest.config();
    let evidence = AcceptedEvidence {
        event: NormalizedEvent {
            vehicle: mapping.vehicle.clone(),
            connection: annotation.connection.clone(),
            position: annotation.position.clone(),
            time,
            kind: annotation.kind,
            charge_type: annotation.charge_type,
            counter: samples.counter,
            battery_percent: samples.battery_percent,
            charging: samples.charging,
            location: samples.location,
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

#[derive(Default)]
struct Samples {
    counter: Option<NormalizedCounter>,
    battery_percent: Option<crate::BatteryPercent>,
    charging: Option<bool>,
    location: Option<NormalizedLocation>,
}

fn decode_samples(fields: Vec<ReceiverField>) -> Result<Samples, RejectionReason> {
    let mut observed_keys = BTreeSet::new();
    let mut samples = Samples::default();
    for field in fields {
        if !observed_keys.insert(field.key) {
            if field.key == ReceiverFieldKey::BatteryLevel {
                samples.battery_percent = None;
                continue;
            }
            return Err(RejectionReason::DuplicateSourceField);
        }
        if field.key == ReceiverFieldKey::Location {
            samples.location = Some(decode_location(&field.value));
            continue;
        }
        if field.key == ReceiverFieldKey::BatteryLevel {
            samples.battery_percent = serde_json::from_str::<ReceiverValue>(field.value.get())
                .ok()
                .filter(|value| value.detailed_charge_state_value.is_none())
                .and_then(|value| value.string_value)
                .and_then(|value| value.parse::<f64>().ok())
                .and_then(|value| crate::BatteryPercent::try_from(value).ok());
            continue;
        }
        let value: ReceiverValue = serde_json::from_str(field.value.get())
            .map_err(|_| RejectionReason::UnsupportedPayload)?;
        match field.key {
            ReceiverFieldKey::VehicleName => {
                if value.string_value.is_none() || value.detailed_charge_state_value.is_some() {
                    return Err(RejectionReason::UnsupportedPayload);
                }
            }
            ReceiverFieldKey::DetailedChargeState => {
                if value.detailed_charge_state_value.is_none() || value.string_value.is_some() {
                    return Err(RejectionReason::UnsupportedPayload);
                }
                samples.charging = value
                    .detailed_charge_state_value
                    .map(|state| matches!(state, ReceiverChargeState::Charging));
            }
            ReceiverFieldKey::ACChargingEnergyIn => {
                let counter_value = value
                    .string_value
                    .ok_or(RejectionReason::UnsupportedPayload)?;
                if value.detailed_charge_state_value.is_some() {
                    return Err(RejectionReason::UnsupportedPayload);
                }
                samples.counter = Some(NormalizedCounter::parse_kwh(&counter_value));
            }
            ReceiverFieldKey::Location | ReceiverFieldKey::BatteryLevel => unreachable!(),
        }
    }
    Ok(samples)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiverLocationValue {
    location_value: ReceiverLocation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiverLocation {
    latitude: f64,
    longitude: f64,
}

fn decode_location(value: &serde_json::value::RawValue) -> NormalizedLocation {
    match serde_json::from_str::<ReceiverLocationValue>(value.get()) {
        Ok(value) => NormalizedLocation::from_coordinates(
            value.location_value.latitude,
            value.location_value.longitude,
        ),
        Err(_) => NormalizedLocation::Invalid,
    }
}
