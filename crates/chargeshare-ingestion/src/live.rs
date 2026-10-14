use std::collections::BTreeSet;

use chargeshare_core::{ChargerGeofence, LocationPoint};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::schema::storage_error;
use crate::{
    AcceptedEvidence, CommitOutcome, DeliveryCoordinate, EvidenceProvenance, IngestionError,
    MAX_PAYLOAD_BYTES, NormalizedChargeType, NormalizedCounter, NormalizedEvent,
    NormalizedEventKind, NormalizedLocation, RecordDisposition, RejectionReason, Store,
};

mod decimal;
use decimal::{DecimalValue, normalize_counter};

pub(crate) const LIVE_INPUT_CONTRACT: &str = "tesla-decoded-live-v1";
pub(crate) const LIVE_NORMALIZER_VERSION: &str = "chargeshare-live-adapter-v2";

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
enum ChargeState {
    #[serde(rename = "DetailedChargeStateUnknown")]
    Unknown,
    #[serde(rename = "DetailedChargeStateDisconnected")]
    Disconnected,
    #[serde(rename = "DetailedChargeStateNoPower")]
    NoPower,
    #[serde(rename = "DetailedChargeStateStarting")]
    Starting,
    #[serde(rename = "DetailedChargeStateCharging")]
    Charging,
    #[serde(rename = "DetailedChargeStateComplete")]
    Complete,
    #[serde(rename = "DetailedChargeStateStopped")]
    Stopped,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Payload {
    data: Vec<Field>,
    created_at: String,
    #[serde(default)]
    vin: Option<String>,
    #[serde(default)]
    is_resend: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    key: String,
    value: Box<serde_json::value::RawValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocationValue {
    location_value: Coordinates,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Coordinates {
    latitude: f64,
    longitude: f64,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
struct Signal {
    vehicle: String,
    created_at: String,
    time: i64,
    state: Option<ChargeState>,
    counter: Option<NormalizedCounter>,
    ac_power: Option<bool>,
    location: Option<NormalizedLocation>,
    odometer: Option<OdometerReading>,
    fast_charger: Option<bool>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
enum OdometerReading {
    Valid(DecimalValue),
    Invalid,
}

#[derive(Clone, Deserialize, Serialize)]
struct QualifiedLocation {
    classification: NormalizedLocation,
    time: i64,
    nanoseconds: u32,
    odometer: DecimalValue,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct AdapterState {
    previous: Option<ChargeState>,
    connection: Option<String>,
    next_position: u64,
    last_time: Option<i64>,
    #[serde(default)]
    last_nanoseconds: u32,
    ac_evidence: bool,
    #[serde(default)]
    previous_counter: Option<NormalizedCounter>,
    #[serde(default)]
    dc_evidence: bool,
    #[serde(default)]
    odometer: Option<DecimalValue>,
    #[serde(default)]
    location: Option<QualifiedLocation>,
    #[serde(default)]
    location_conflict_time: Option<(i64, u32)>,
}

pub(crate) struct LiveCheckpoint {
    vehicle: String,
    record_id: String,
    signal_json: String,
    disposition_json: String,
    state_json: String,
}

impl LiveCheckpoint {
    pub(crate) fn save(&self, transaction: &Transaction<'_>) -> Result<(), IngestionError> {
        transaction
            .execute(
                "INSERT INTO live_records(vehicle, record_id, signal_json, disposition_json)
             VALUES (?1, ?2, ?3, ?4)",
                params![
                    self.vehicle,
                    self.record_id,
                    self.signal_json,
                    self.disposition_json
                ],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "INSERT INTO live_adapter_state(vehicle, state_json) VALUES (?1, ?2)
             ON CONFLICT(vehicle) DO UPDATE SET state_json = excluded.state_json",
                params![self.vehicle, self.state_json],
            )
            .map_err(storage_error)?;
        Ok(())
    }
}

pub(crate) fn initialize(connection: &Connection) -> Result<(), IngestionError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS live_adapter_state (
            vehicle TEXT PRIMARY KEY REFERENCES registrations(vehicle), state_json TEXT NOT NULL
         ) STRICT;
         CREATE TABLE IF NOT EXISTS live_records (
            vehicle TEXT NOT NULL REFERENCES registrations(vehicle), record_id TEXT NOT NULL,
            signal_json TEXT NOT NULL, disposition_json TEXT NOT NULL,
            PRIMARY KEY(vehicle, record_id)
         ) STRICT;
         CREATE INDEX IF NOT EXISTS live_signal_identity ON live_records(vehicle, signal_json);",
        )
        .map_err(storage_error)
}

impl Store {
    pub(crate) fn ingest_live(
        &mut self,
        config: &crate::live_config::LiveConfig,
        coordinate: DeliveryCoordinate,
        expected_position: i64,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
    ) -> Result<CommitOutcome, IngestionError> {
        self.ingest_live_observed(
            config,
            coordinate,
            expected_position,
            headers,
            payload,
            &mut |_| Ok(()),
        )
    }

    pub(crate) fn ingest_live_observed(
        &mut self,
        config: &crate::live_config::LiveConfig,
        coordinate: DeliveryCoordinate,
        expected_position: i64,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
        observer: &mut dyn FnMut(crate::CommitBoundary) -> Result<(), IngestionError>,
    ) -> Result<CommitOutcome, IngestionError> {
        if !self.manifest.is_live() || config.manifest()?.config() != self.manifest.config() {
            return Err(IngestionError::ConfigurationMismatch);
        }
        let (signal, record_id, is_resend) = match decode(config, headers, payload) {
            Ok(decoded) => decoded,
            Err(reason) => {
                return self.commit_with_checkpoint(
                    coordinate,
                    expected_position,
                    RecordDisposition::Rejected(reason),
                    None,
                    observer,
                );
            }
        };
        let signal_json = encode(&signal)?;
        let retained: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT signal_json, disposition_json FROM live_records
             WHERE vehicle = ?1 AND record_id = ?2",
                params![signal.vehicle, record_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((original, disposition)) = retained {
            if original != signal_json {
                return Err(IngestionError::DeliveryConflict);
            }
            return self.commit_with_checkpoint(
                coordinate,
                expected_position,
                serde_json::from_str(&disposition).map_err(|_| IngestionError::StorageCorrupt)?,
                None,
                observer,
            );
        }
        let repeated: Option<String> = self.connection.query_row(
            "SELECT disposition_json FROM live_records WHERE vehicle = ?1 AND signal_json = ?2 LIMIT 1",
            params![signal.vehicle, signal_json], |row| row.get(0),
        ).optional().map_err(storage_error)?;
        let state_json: Option<String> = self
            .connection
            .query_row(
                "SELECT state_json FROM live_adapter_state WHERE vehicle = ?1",
                [&signal.vehicle],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let mut state: AdapterState = state_json
            .map(|json| serde_json::from_str(&json).map_err(|_| IngestionError::StorageCorrupt))
            .transpose()?
            .unwrap_or_default();
        let disposition = if let Some(repeated) = repeated {
            serde_json::from_str(&repeated).map_err(|_| IngestionError::StorageCorrupt)?
        } else {
            adapt(&self.manifest, &mut state, &signal, is_resend)?
        };
        let checkpoint = LiveCheckpoint {
            vehicle: signal.vehicle,
            record_id,
            signal_json,
            disposition_json: encode(&disposition)?,
            state_json: encode(&state)?,
        };
        self.commit_with_checkpoint(
            coordinate,
            expected_position,
            disposition,
            Some(&checkpoint),
            observer,
        )
    }
}

fn adapt(
    manifest: &crate::Manifest,
    state: &mut AdapterState,
    signal: &Signal,
    is_resend: bool,
) -> Result<RecordDisposition, IngestionError> {
    let timestamp = parse_utc_timestamp(&signal.created_at)
        .map_err(|_| IngestionError::InvalidConfiguration)?;
    let late = state
        .last_time
        .is_some_and(|last| timestamp < (last, state.last_nanoseconds));
    let prior = state.previous;
    let current = signal.state.or(prior);
    let starts = state.connection.is_none() && current == Some(ChargeState::Charging);
    let kind = if starts
        && matches!(
            prior,
            Some(
                ChargeState::Disconnected
                    | ChargeState::Starting
                    | ChargeState::NoPower
                    | ChargeState::Complete
                    | ChargeState::Stopped
            )
        )
        && !late
    {
        NormalizedEventKind::Start
    } else if state.connection.is_some()
        && matches!(
            signal.state,
            Some(ChargeState::Complete | ChargeState::Stopped | ChargeState::Disconnected)
        )
        && !late
    {
        NormalizedEventKind::End
    } else {
        NormalizedEventKind::Sample
    };
    if starts {
        state.connection = Some(format!("live-{}", state.next_position));
        state.ac_evidence = false;
        state.dc_evidence = false;
        state.previous_counter = None;
    }
    if !late {
        state.observe_location(signal, timestamp.1);
        state.previous = current;
        state.last_time = Some(signal.time);
        state.last_nanoseconds = timestamp.1;
    } else if signal.location.is_some() || signal.odometer.is_some() {
        state.location = None;
        if signal.odometer.is_some() {
            state.odometer = None;
        }
    }
    let Some(connection) = state.connection.clone() else {
        return Ok(RecordDisposition::Rejected(
            RejectionReason::MissingAssociation,
        ));
    };
    state.ac_evidence |= !late
        && (signal.ac_power == Some(true)
            || counter_increased(&state.previous_counter, &signal.counter));
    if !late && signal.counter.is_some() {
        state.previous_counter = signal.counter.clone();
    }
    state.dc_evidence |= signal.fast_charger == Some(true);
    let ambiguous = late || current == Some(ChargeState::Unknown);
    let position = state.next_position.to_string();
    state.next_position = state
        .next_position
        .checked_add(1)
        .ok_or(IngestionError::ProgressConflict)?;
    if kind == NormalizedEventKind::End {
        state.connection = None;
    }
    let mapping = manifest
        .mapping(&signal.vehicle)
        .ok_or(IngestionError::ConfigurationMismatch)?;
    let config = manifest.config();
    Ok(RecordDisposition::Accepted(Box::new(AcceptedEvidence {
        event: NormalizedEvent {
            vehicle: signal.vehicle.clone(),
            connection,
            position,
            time: signal.time,
            kind,
            charge_type: if state.dc_evidence {
                NormalizedChargeType::Dc
            } else if ambiguous {
                NormalizedChargeType::Ambiguous
            } else if state.ac_evidence {
                NormalizedChargeType::Ac
            } else {
                NormalizedChargeType::Unknown
            },
            counter: signal.counter.clone(),
            location: if late {
                Some(NormalizedLocation::Invalid)
            } else {
                state
                    .location
                    .as_ref()
                    .map(|location| location.classification.clone())
            },
        },
        provenance: EvidenceProvenance {
            device: signal.vehicle.clone(),
            owner: mapping.owner.clone(),
            created_at: signal.created_at.clone(),
            input_contract: config.input_contract.clone(),
            mapping_version: config.mapping_version.clone(),
            manifest_version: config.manifest_version.clone(),
            normalizer_version: config.normalizer_version.clone(),
            timestamp_version: config.timestamp_version.clone(),
            is_resend,
        },
    })))
}

impl AdapterState {
    fn observe_location(&mut self, signal: &Signal, nanoseconds: u32) {
        let timestamp = (signal.time, nanoseconds);
        if let (Some(location), Some(classification)) = (&self.location, &signal.location)
            && timestamp == (location.time, location.nanoseconds)
            && classification != &location.classification
        {
            self.location_conflict_time = Some(timestamp);
        }
        match &signal.odometer {
            Some(OdometerReading::Valid(current)) => {
                if self
                    .odometer
                    .as_ref()
                    .is_some_and(|prior| current.compare(prior).is_lt())
                {
                    self.odometer = None;
                    self.location = None;
                    return;
                }
                self.odometer = Some(current.clone());
                if self
                    .location
                    .as_ref()
                    .is_some_and(|location| current != &location.odometer)
                {
                    self.location = None;
                }
            }
            Some(OdometerReading::Invalid) => {
                self.odometer = None;
                self.location = None;
                return;
            }
            None => {}
        }
        match &signal.location {
            Some(
                classification @ (NormalizedLocation::InsideGeofence
                | NormalizedLocation::OutsideGeofence),
            ) => {
                if self
                    .location_conflict_time
                    .is_some_and(|conflict| timestamp <= conflict)
                {
                    self.location = None;
                    return;
                }
                self.location = self.odometer.clone().map(|odometer| QualifiedLocation {
                    classification: classification.clone(),
                    time: signal.time,
                    nanoseconds,
                    odometer,
                });
            }
            Some(_) => self.location = None,
            None => {}
        }
    }
}

fn counter_increased(
    prior: &Option<NormalizedCounter>,
    current: &Option<NormalizedCounter>,
) -> bool {
    match (prior, current) {
        (
            Some(NormalizedCounter::Valid { kwh: prior }),
            Some(NormalizedCounter::Valid { kwh: current }),
        ) => {
            matches!((chargeshare_core::Energy::parse_kwh(prior), chargeshare_core::Energy::parse_kwh(current)),
                (Ok(prior), Ok(current)) if current > prior)
        }
        _ => false,
    }
}

fn decode(
    config: &crate::live_config::LiveConfig,
    headers: &[(String, Vec<u8>)],
    payload: &[u8],
) -> Result<(Signal, String, bool), RejectionReason> {
    if payload.len() > MAX_PAYLOAD_BYTES
        || headers.len() > 32
        || headers
            .iter()
            .any(|(key, value)| key.len() > 64 || value.len() > 256)
    {
        return Err(RejectionReason::InputTooLarge);
    }
    let header = |name: &str| -> Result<&str, RejectionReason> {
        let mut matching = headers.iter().filter(|(key, _)| key == name);
        let value = matching.next().ok_or(RejectionReason::MissingIdentity)?;
        if matching.next().is_some() {
            return Err(RejectionReason::DuplicateSourceField);
        }
        std::str::from_utf8(&value.1).map_err(|_| RejectionReason::UnsupportedPayload)
    };
    let vin = header("vin")?;
    let txid = header("txid")?;
    if vin.is_empty() || txid.is_empty() || header("txtype")? != "V" {
        return Err(RejectionReason::UnsupportedPayload);
    }
    let mapping = config
        .mappings
        .iter()
        .find(|mapping| mapping.device == vin)
        .ok_or(RejectionReason::UnknownIdentity)?;
    let payload: Payload =
        serde_json::from_slice(payload).map_err(|_| RejectionReason::UnsupportedPayload)?;
    if payload
        .vin
        .as_deref()
        .is_some_and(|value| !value.is_empty() && value != vin)
    {
        return Err(RejectionReason::IdentityMismatch);
    }
    let (time, _) = parse_utc_timestamp(&payload.created_at)?;
    let mut signal = Signal {
        vehicle: mapping.vehicle.clone(),
        created_at: payload.created_at,
        time,
        state: None,
        counter: None,
        ac_power: None,
        location: None,
        odometer: None,
        fast_charger: None,
    };
    let geofence = config
        .geofence()
        .map_err(|_| RejectionReason::InvalidManifest)?;
    let mut keys = BTreeSet::new();
    let mut invalid_evidence = false;
    for field in payload.data {
        if !matches!(
            field.key.as_str(),
            "DetailedChargeState"
                | "ACChargingEnergyIn"
                | "ACChargingPower"
                | "Location"
                | "Odometer"
                | "FastChargerPresent"
        ) {
            continue;
        }
        if !keys.insert(field.key.clone()) {
            invalid_evidence = true;
            continue;
        }
        match field.key.as_str() {
            "DetailedChargeState" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct StateValue {
                    detailed_charge_state_value: ChargeState,
                }
                signal.state = Some(
                    serde_json::from_str::<StateValue>(field.value.get())
                        .map(|value| value.detailed_charge_state_value)
                        .unwrap_or(ChargeState::Unknown),
                );
            }
            "ACChargingEnergyIn" => {
                signal.counter = Some(
                    numeric_text(&field.value)
                        .map(|value| normalize_counter(&value))
                        .unwrap_or(NormalizedCounter::Rejected {
                            reason: crate::SafeCounterProblem::Invalid,
                        }),
                );
            }
            "ACChargingPower" => {
                signal.ac_power = Some(
                    numeric_text(&field.value)
                        .and_then(|value| DecimalValue::parse(&value).ok())
                        .is_some_and(|power| power.is_positive()),
                );
            }
            "Location" => signal.location = Some(classify(&field.value, geofence)),
            "Odometer" => {
                signal.odometer = Some(
                    numeric_text(&field.value)
                        .and_then(|value| DecimalValue::parse(&value).ok())
                        .map(OdometerReading::Valid)
                        .unwrap_or(OdometerReading::Invalid),
                );
            }
            "FastChargerPresent" => {
                signal.fast_charger = boolean_value(&field.value);
                invalid_evidence |= signal.fast_charger.is_none();
            }
            _ => unreachable!(),
        }
    }
    if invalid_evidence {
        signal.state = Some(ChargeState::Unknown);
        signal.counter = Some(NormalizedCounter::Rejected {
            reason: crate::SafeCounterProblem::Invalid,
        });
        signal.location = Some(NormalizedLocation::Invalid);
        signal.odometer = Some(OdometerReading::Invalid);
    }
    Ok((
        signal,
        format!("record-{:016x}", fingerprint(txid.as_bytes())),
        payload.is_resend,
    ))
}

fn numeric_text(value: &serde_json::value::RawValue) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct NumericValue {
        string_value: Option<String>,
        long_value: Option<String>,
        double_value: Option<Box<serde_json::value::RawValue>>,
        float_value: Option<Box<serde_json::value::RawValue>>,
        int_value: Option<Box<serde_json::value::RawValue>>,
    }
    let value: NumericValue = serde_json::from_str(value.get()).ok()?;
    let values = [
        value.string_value,
        value.long_value,
        value.double_value.map(|value| value.get().to_owned()),
        value.float_value.map(|value| value.get().to_owned()),
        value.int_value.map(|value| value.get().to_owned()),
    ];
    let mut values = values.into_iter().flatten();
    let first = values.next()?;
    if values.next().is_some() {
        return None;
    }
    Some(first)
}

fn boolean_value(value: &serde_json::value::RawValue) -> Option<bool> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct BooleanValue {
        boolean_value: Option<bool>,
        string_value: Option<String>,
    }
    let value: BooleanValue = serde_json::from_str(value.get()).ok()?;
    match (value.boolean_value, value.string_value.as_deref()) {
        (Some(value), None) => Some(value),
        (None, Some("true")) => Some(true),
        (None, Some("false")) => Some(false),
        _ => None,
    }
}

pub(crate) fn parse_utc_timestamp(input: &str) -> Result<(i64, u32), RejectionReason> {
    let utc = input
        .strip_suffix('Z')
        .or_else(|| input.strip_suffix("+00:00"))
        .ok_or(RejectionReason::InvalidTimestamp)?;
    let (seconds, nanoseconds) = match utc.split_once('.') {
        Some((seconds, fraction)) => {
            if fraction.is_empty()
                || fraction.len() > 9
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(RejectionReason::InvalidTimestamp);
            }
            let nanoseconds = fraction
                .parse::<u32>()
                .map_err(|_| RejectionReason::InvalidTimestamp)?
                * 10_u32.pow(9 - fraction.len() as u32);
            (seconds, nanoseconds)
        }
        None => (utc, 0),
    };
    Ok((
        crate::manifest::parse_utc_seconds(&format!("{seconds}Z"))?,
        nanoseconds,
    ))
}

fn classify(value: &serde_json::value::RawValue, geofence: ChargerGeofence) -> NormalizedLocation {
    let point = serde_json::from_str::<LocationValue>(value.get())
        .ok()
        .and_then(|value| {
            LocationPoint::new(
                value.location_value.latitude,
                value.location_value.longitude,
            )
        });
    match point {
        Some(point) if geofence.contains(point) => NormalizedLocation::InsideGeofence,
        Some(_) => NormalizedLocation::OutsideGeofence,
        None => NormalizedLocation::Invalid,
    }
}

pub(crate) fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn encode(value: &impl Serialize) -> Result<String, IngestionError> {
    serde_json::to_string(value).map_err(|_| IngestionError::InvalidConfiguration)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod rules;
