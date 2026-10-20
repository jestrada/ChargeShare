use std::collections::BTreeSet;

use chargeshare_core::LocationPoint;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::schema::storage_error;
use crate::{
    AcceptedEvidence, CommitOutcome, DeliveryCoordinate, EvidenceProvenance, IngestionError,
    MAX_PAYLOAD_BYTES, NormalizedChargeType, NormalizedCounter, NormalizedEvent,
    NormalizedEventKind, NormalizedLocation, RecordDisposition, RejectionReason, Store,
};

mod counter_window;
mod decimal;
use decimal::{DecimalValue, normalize_counter};

pub(crate) const LIVE_INPUT_CONTRACT: &str = "tesla-decoded-live-v1";
pub(crate) const LIVE_NORMALIZER_VERSION: &str = "chargeshare-live-adapter-v2";

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum ChargeState {
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
    #[serde(default)]
    battery_percent: Option<crate::BatteryPercent>,
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
    pub(crate) fn needs_battery_backfill(&mut self) -> Result<bool, IngestionError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS live_battery_backfill (singleton INTEGER PRIMARY KEY CHECK(singleton = 1)) STRICT;",
        ).map_err(storage_error)?;
        self.connection
            .query_row(
                "SELECT NOT EXISTS(SELECT 1 FROM live_battery_backfill) AND EXISTS(SELECT 1 FROM live_records WHERE json_type(signal_json, '$.battery_percent') IS NULL)",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)
    }

    pub(crate) fn backfill_live_battery(
        &mut self,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
    ) -> Result<(), IngestionError> {
        let registry = self
            .registry
            .as_ref()
            .ok_or(IngestionError::RegistryIntegrity)?;
        let Ok((signal, record_id, _)) = decode(registry, headers, payload) else {
            return Ok(());
        };
        let legacy: Option<String> = self.connection.query_row(
            "SELECT signal_json FROM live_records WHERE vehicle = ?1 AND record_id = ?2 AND json_type(signal_json, '$.battery_percent') IS NULL",
            params![signal.vehicle, record_id], |row| row.get(0),
        ).optional().map_err(storage_error)?;
        if let Some(legacy) = legacy {
            self.prepare_live(registry, headers, payload)?;
            let mut retained: Signal =
                serde_json::from_str(&legacy).map_err(|_| IngestionError::StorageCorrupt)?;
            retained.battery_percent = signal.battery_percent;
            self.connection.execute(
                "UPDATE live_records SET signal_json = ?3 WHERE vehicle = ?1 AND record_id = ?2",
                params![signal.vehicle, record_id, encode(&retained)?],
            ).map_err(storage_error)?;
        }
        Ok(())
    }

    pub(crate) fn finish_battery_backfill(&mut self) -> Result<(), IngestionError> {
        self.connection
            .execute(
                "INSERT INTO live_battery_backfill VALUES (1) ON CONFLICT DO NOTHING",
                [],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub(crate) fn ingest_live(
        &mut self,
        settings: &crate::RuntimeSettings,
        coordinate: DeliveryCoordinate,
        expected_position: i64,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
    ) -> Result<CommitOutcome, IngestionError> {
        self.ingest_live_observed(
            settings,
            coordinate,
            expected_position,
            headers,
            payload,
            &mut |_| Ok(()),
        )
    }

    pub(crate) fn ingest_live_observed(
        &mut self,
        settings: &crate::RuntimeSettings,
        coordinate: DeliveryCoordinate,
        expected_position: i64,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
        observer: &mut dyn FnMut(crate::CommitBoundary) -> Result<(), IngestionError>,
    ) -> Result<CommitOutcome, IngestionError> {
        if !self.manifest.is_live()
            || crate::registry::json(settings.manifest()?.config())? != self.fixed_manifest_json()
        {
            return Err(IngestionError::ConfigurationMismatch);
        }
        if self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM deliveries WHERE partition = ?1 AND offset = ?2)",
                params![coordinate.partition, coordinate.offset],
                |row| row.get::<_, bool>(0),
            )
            .map_err(storage_error)?
        {
            self.verify_live_reread(coordinate, headers, payload)?;
            let next_offset = self
                .positions()?
                .into_iter()
                .find(|position| position.partition == coordinate.partition)
                .ok_or(IngestionError::SourceMismatch)?
                .next_offset;
            return Ok(CommitOutcome {
                duplicate_delivery: true,
                inserted_variant: false,
                next_offset,
            });
        }
        let registry = self
            .registry
            .as_ref()
            .ok_or(IngestionError::RegistryIntegrity)?
            .clone();
        let (disposition, checkpoint) = self.prepare_live(&registry, headers, payload)?;
        self.commit_with_checkpoint(
            coordinate,
            expected_position,
            disposition,
            checkpoint.as_ref(),
            observer,
        )
    }

    pub(crate) fn prepare_live(
        &self,
        registry: &crate::registry::RegistrySnapshot,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
    ) -> Result<(RecordDisposition, Option<LiveCheckpoint>), IngestionError> {
        let (signal, record_id, is_resend) = match decode(registry, headers, payload) {
            Ok(decoded) => decoded,
            Err(reason) => {
                return Ok((RecordDisposition::Rejected(reason), None));
            }
        };
        let signal_json = encode(&signal)?;
        let mut legacy_signal = signal.clone();
        legacy_signal.location = match &signal.location {
            Some(NormalizedLocation::MatchedCharger { charger_id, .. })
                if registry.legacy_charger.as_ref() == Some(charger_id) =>
            {
                Some(NormalizedLocation::InsideGeofence)
            }
            Some(NormalizedLocation::OutsideChargers { .. }) => {
                Some(NormalizedLocation::OutsideGeofence)
            }
            _ => signal.location.clone(),
        };
        let legacy_signal_json = encode(&legacy_signal)?;
        let soc_field = format!(",\"battery_percent\":{}", encode(&signal.battery_percent)?);
        let old_signal_json = signal_json.replacen(&soc_field, "", 1);
        let old_legacy_signal_json = legacy_signal_json.replacen(&soc_field, "", 1);
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
            let disposition: RecordDisposition =
                serde_json::from_str(&disposition).map_err(|_| IngestionError::StorageCorrupt)?;
            if original != signal_json
                && original != legacy_signal_json
                && original != old_signal_json
                && original != old_legacy_signal_json
            {
                return Err(IngestionError::DeliveryConflict);
            }
            return Ok((disposition, None));
        }
        let repeated: Option<(String, String)> = self.connection.query_row(
            "SELECT signal_json, disposition_json FROM live_records WHERE vehicle = ?1 AND (signal_json = ?2 OR signal_json = ?3 OR signal_json = ?4 OR signal_json = ?5) LIMIT 1",
            params![signal.vehicle, signal_json, legacy_signal_json, old_signal_json, old_legacy_signal_json], |row| Ok((row.get(0)?, row.get(1)?)),
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
        let (disposition, signal_json) = if let Some((retained_signal, repeated)) = repeated {
            (
                serde_json::from_str(&repeated).map_err(|_| IngestionError::StorageCorrupt)?,
                retained_signal,
            )
        } else {
            (
                adapt(&self.manifest, &mut state, &signal, is_resend)?,
                signal_json,
            )
        };
        let checkpoint = LiveCheckpoint {
            vehicle: signal.vehicle,
            record_id,
            signal_json,
            disposition_json: encode(&disposition)?,
            state_json: encode(&state)?,
        };
        Ok((disposition, Some(checkpoint)))
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
            battery_percent: signal.battery_percent,
            charging: None,
            location: if late {
                Some(NormalizedLocation::Invalid)
            } else {
                if matches!(
                    signal.location,
                    Some(NormalizedLocation::AmbiguousChargers { .. })
                ) {
                    signal.location.clone()
                } else {
                    state
                        .location
                        .as_ref()
                        .map(|location| location.classification.clone())
                }
            },
        },
        provenance: EvidenceProvenance {
            device: signal.vehicle.clone(),
            owner: mapping.owner.clone(),
            created_at: signal.created_at.clone(),
            input_contract: config.input_contract.clone(),
            mapping_version: manifest
                .live_identity_versions
                .get(&signal.vehicle)
                .and_then(|versions| versions.iter().find(|version| version.starts_with("car-")))
                .cloned()
                .unwrap_or_else(|| config.mapping_version.clone()),
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
                | NormalizedLocation::OutsideGeofence
                | NormalizedLocation::OutsideChargers { .. }
                | NormalizedLocation::MatchedCharger { .. }),
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
    registry: &crate::registry::RegistrySnapshot,
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
    let car = registry
        .cars
        .iter()
        .find(|car| car.registration.device == vin)
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
        vehicle: car.registration.vehicle.clone(),
        created_at: payload.created_at,
        time,
        state: None,
        counter: None,
        battery_percent: None,
        ac_power: None,
        location: None,
        odometer: None,
        fast_charger: None,
    };
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
                | "Soc"
        ) {
            continue;
        }
        if !keys.insert(field.key.clone()) {
            if field.key == "Soc" {
                signal.battery_percent = None;
                continue;
            }
            invalid_evidence = true;
            continue;
        }
        match field.key.as_str() {
            "Soc" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct SocValue {
                    double_value: f64,
                }
                signal.battery_percent = serde_json::from_str::<SocValue>(field.value.get())
                    .ok()
                    .and_then(|value| crate::BatteryPercent::try_from(value.double_value).ok());
            }
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
            "Location" => signal.location = Some(classify(&field.value, car, registry)),
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

fn classify(
    value: &serde_json::value::RawValue,
    car: &crate::registry::Car,
    registry: &crate::registry::RegistrySnapshot,
) -> NormalizedLocation {
    let point = serde_json::from_str::<LocationValue>(value.get())
        .ok()
        .and_then(|value| {
            LocationPoint::new(
                value.location_value.latitude,
                value.location_value.longitude,
            )
        });
    let Some(point) = point else {
        return NormalizedLocation::Invalid;
    };
    let matches: Vec<_> = car
        .registration
        .chargers
        .iter()
        .filter_map(|id| registry.chargers.get(id))
        .filter(|charger| {
            charger
                .geofence
                .to_core()
                .is_ok_and(|geofence| geofence.contains(point))
        })
        .collect();
    match matches.as_slice() {
        [] if car.identity_version.starts_with("live-config-") => {
            NormalizedLocation::OutsideGeofence
        }
        [] => NormalizedLocation::OutsideChargers {
            assignment_version: car.assignment_version.clone(),
            geofence_versions: car
                .registration
                .chargers
                .iter()
                .filter_map(|id| registry.chargers.get(id))
                .map(|charger| charger.geofence_version.clone())
                .collect(),
        },
        [_charger]
            if registry.legacy_charger.is_some()
                && car.identity_version.starts_with("live-config-") =>
        {
            NormalizedLocation::InsideGeofence
        }
        [charger] => NormalizedLocation::MatchedCharger {
            charger_id: charger.id.clone(),
            assignment_version: car.assignment_version.clone(),
            geofence_version: charger.geofence_version.clone(),
        },
        _ if car.identity_version.starts_with("live-config-") => NormalizedLocation::Invalid,
        _ => NormalizedLocation::AmbiguousChargers {
            assignment_version: car.assignment_version.clone(),
            geofence_versions: matches
                .iter()
                .map(|charger| charger.geofence_version.clone())
                .collect(),
        },
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

#[cfg(test)]
fn decode_bootstrap(
    bootstrap: &crate::RegistryBootstrap,
    headers: &[(String, Vec<u8>)],
    payload: &[u8],
) -> Result<(Signal, String, bool), RejectionReason> {
    decode(
        &bootstrap
            .decoder_registry()
            .map_err(|_| RejectionReason::InvalidManifest)?,
        headers,
        payload,
    )
}

#[cfg(test)]
impl Store {
    fn ingest_bootstrap(
        &mut self,
        bootstrap: &crate::RegistryBootstrap,
        coordinate: DeliveryCoordinate,
        expected_position: i64,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
    ) -> Result<CommitOutcome, IngestionError> {
        let (disposition, checkpoint) =
            self.prepare_live(&bootstrap.decoder_registry()?, headers, payload)?;
        self.commit_with_checkpoint(
            coordinate,
            expected_position,
            disposition,
            checkpoint.as_ref(),
            &mut |_| Ok(()),
        )
    }
}

impl Store {
    pub(crate) fn verify_live_reread(
        &self,
        coordinate: DeliveryCoordinate,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
    ) -> Result<(), IngestionError> {
        let admissions: String = self.connection.query_row("SELECT admitted_vehicles_json FROM live_delivery_admissions WHERE partition = ?1 AND offset = ?2",params![coordinate.partition,coordinate.offset],|row| row.get(0)).map_err(storage_error)?;
        let admitted: Vec<String> =
            serde_json::from_str(&admissions).map_err(|_| IngestionError::RegistryIntegrity)?;
        let mut registry = self
            .registry
            .as_ref()
            .ok_or(IngestionError::RegistryIntegrity)?
            .clone();
        registry
            .cars
            .retain(|car| admitted.contains(&car.registration.vehicle));
        let (disposition, _) = self.prepare_live(&registry, headers, payload)?;
        let stored: (Option<String>,Option<String>,Option<String>) = self.connection.query_row("SELECT v.event_json,d.provenance_json,d.rejection_reason FROM deliveries d LEFT JOIN event_variants v ON v.id = d.variant_id WHERE d.partition = ?1 AND d.offset = ?2",params![coordinate.partition,coordinate.offset],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(storage_error)?;
        let actual = match disposition {
            RecordDisposition::Accepted(evidence) => (
                Some(encode(&evidence.event)?),
                Some(encode(&evidence.provenance)?),
                None,
            ),
            RecordDisposition::Rejected(reason) => (None, None, Some(encode(&reason)?)),
        };
        if stored != actual {
            return Err(IngestionError::DeliveryConflict);
        }
        Ok(())
    }
}

impl Store {
    pub(crate) fn verify_live_registry_state(&self) -> Result<(), IngestionError> {
        let registry = self
            .registry
            .as_ref()
            .ok_or(IngestionError::RegistryIntegrity)?;
        let mut statement = self
            .connection
            .prepare("SELECT vehicle,signal_json,disposition_json FROM live_records")
            .map_err(storage_error)?;
        let records = statement
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
        for (vehicle, signal_json, disposition_json) in records {
            let signal: Signal = serde_json::from_str(&signal_json)
                .map_err(|_| IngestionError::RegistryIntegrity)?;
            let disposition: RecordDisposition = serde_json::from_str(&disposition_json)
                .map_err(|_| IngestionError::RegistryIntegrity)?;
            if signal.vehicle != vehicle
                || parse_utc_timestamp(&signal.created_at).map(|time| time.0) != Ok(signal.time)
            {
                return Err(IngestionError::RegistryIntegrity);
            }
            if let Some(location) = &signal.location {
                if matches!(
                    location,
                    NormalizedLocation::Valid { .. } | NormalizedLocation::RedactedGeofence
                ) {
                    return Err(IngestionError::RegistryIntegrity);
                }
                registry.validate_location(&vehicle, location)?;
            }
            if let RecordDisposition::Accepted(evidence) = &disposition {
                self.manifest
                    .validate_evidence(evidence)
                    .map_err(|_| IngestionError::RegistryIntegrity)?;
                if evidence.event.vehicle != vehicle {
                    return Err(IngestionError::RegistryIntegrity);
                }
                if let Some(location) = &evidence.event.location {
                    registry.validate_location(&vehicle, location)?;
                }
            }
        }
        let mut statement = self
            .connection
            .prepare("SELECT vehicle,state_json FROM live_adapter_state")
            .map_err(storage_error)?;
        let records = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)?;
        for (vehicle, state_json) in records {
            let state: AdapterState =
                serde_json::from_str(&state_json).map_err(|_| IngestionError::RegistryIntegrity)?;
            if state.last_nanoseconds >= 1_000_000_000 {
                return Err(IngestionError::RegistryIntegrity);
            }
            if let Some(location) = &state.location {
                if location.nanoseconds >= 1_000_000_000
                    || matches!(
                        location.classification,
                        NormalizedLocation::Valid { .. } | NormalizedLocation::RedactedGeofence
                    )
                {
                    return Err(IngestionError::RegistryIntegrity);
                }
                registry.validate_location(&vehicle, &location.classification)?;
            }
        }
        Ok(())
    }
}
