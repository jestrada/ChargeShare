use std::fmt;

use chargeshare_core::{
    ChargeType, ConnectionId, CounterProblem, CounterReading, Event, EventKind, LocationPoint,
    LocationSample, VehicleId,
};
use serde::{Deserialize, Serialize};

pub const INPUT_CONTRACT_VERSION: &str = "tesla-v-bd076fe-v1";
pub const NORMALIZER_VERSION: &str = "chargeshare-normalizer-v1";
pub const TIMESTAMP_VERSION: &str = "utc-seconds-v1";
pub const MAX_PAYLOAD_BYTES: usize = 65_536;
pub const MAX_MANIFEST_BYTES: usize = 2_097_152;
pub const MAX_MAPPINGS: usize = 256;
pub const MAX_ANNOTATIONS: usize = 8_192;

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NormalizedLocation {
    Valid { latitude: String, longitude: String },
    Invalid,
    InsideGeofence,
    OutsideGeofence,
    RedactedGeofence,
}

impl NormalizedLocation {
    pub fn from_coordinates(latitude: f64, longitude: f64) -> Self {
        match LocationPoint::new(latitude, longitude) {
            Some(point) => Self::Valid {
                latitude: point.latitude().to_string(),
                longitude: point.longitude().to_string(),
            },
            None => Self::Invalid,
        }
    }

    pub fn to_core(&self) -> Result<LocationSample, RejectionReason> {
        match self {
            Self::InsideGeofence => Ok(LocationSample::InsideGeofence),
            Self::OutsideGeofence => Ok(LocationSample::OutsideGeofence),
            Self::RedactedGeofence => Err(RejectionReason::InvalidNormalizedEvent),
            Self::Invalid => Ok(LocationSample::Invalid),
            Self::Valid {
                latitude,
                longitude,
            } => {
                let point = LocationPoint::new(
                    latitude
                        .parse()
                        .map_err(|_| RejectionReason::InvalidNormalizedEvent)?,
                    longitude
                        .parse()
                        .map_err(|_| RejectionReason::InvalidNormalizedEvent)?,
                )
                .ok_or(RejectionReason::InvalidNormalizedEvent)?;
                if point.latitude().to_string() != *latitude
                    || point.longitude().to_string() != *longitude
                {
                    return Err(RejectionReason::InvalidNormalizedEvent);
                }
                Ok(LocationSample::Valid(point))
            }
        }
    }
}

impl fmt::Debug for NormalizedLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Valid { .. } => "Valid { coordinates: <redacted> }",
            Self::Invalid => "Invalid",
            Self::InsideGeofence => "InsideGeofence",
            Self::OutsideGeofence => "OutsideGeofence",
            Self::RedactedGeofence => "RedactedGeofence",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    InputTooLarge,
    UnsupportedPayload,
    DuplicateSourceField,
    MissingIdentity,
    UnknownIdentity,
    IdentityMismatch,
    InvalidTimestamp,
    MissingAssociation,
    InvalidManifest,
    IncompatibleVersion,
    DuplicateMapping,
    DuplicateAssociation,
    InvalidNormalizedEvent,
    InvalidEvidence,
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::InputTooLarge => "input_too_large",
            Self::UnsupportedPayload => "unsupported_payload",
            Self::DuplicateSourceField => "duplicate_source_field",
            Self::MissingIdentity => "missing_identity",
            Self::UnknownIdentity => "unknown_identity",
            Self::IdentityMismatch => "identity_mismatch",
            Self::InvalidTimestamp => "invalid_timestamp",
            Self::MissingAssociation => "missing_association",
            Self::InvalidManifest => "invalid_manifest",
            Self::IncompatibleVersion => "incompatible_version",
            Self::DuplicateMapping => "duplicate_mapping",
            Self::DuplicateAssociation => "duplicate_association",
            Self::InvalidNormalizedEvent => "invalid_normalized_event",
            Self::InvalidEvidence => "invalid_evidence",
        };
        formatter.write_str(reason)
    }
}

impl std::error::Error for RejectionReason {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedEventKind {
    Start,
    Sample,
    Pause,
    Resume,
    End,
}

impl From<NormalizedEventKind> for EventKind {
    fn from(kind: NormalizedEventKind) -> Self {
        match kind {
            NormalizedEventKind::Start => Self::Start,
            NormalizedEventKind::Sample => Self::Sample,
            NormalizedEventKind::Pause => Self::Pause,
            NormalizedEventKind::Resume => Self::Resume,
            NormalizedEventKind::End => Self::End,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedChargeType {
    Ac,
    Dc,
    Ambiguous,
}

impl From<NormalizedChargeType> for ChargeType {
    fn from(charge_type: NormalizedChargeType) -> Self {
        match charge_type {
            NormalizedChargeType::Ac => Self::Ac,
            NormalizedChargeType::Dc => Self::Dc,
            NormalizedChargeType::Ambiguous => Self::Ambiguous,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeCounterProblem {
    Invalid,
    Negative,
    UnsupportedPrecision,
    Overflow,
}

impl From<CounterProblem> for SafeCounterProblem {
    fn from(problem: CounterProblem) -> Self {
        match problem {
            CounterProblem::Invalid => Self::Invalid,
            CounterProblem::Negative => Self::Negative,
            CounterProblem::UnsupportedPrecision => Self::UnsupportedPrecision,
            CounterProblem::Overflow => Self::Overflow,
        }
    }
}

impl From<SafeCounterProblem> for CounterProblem {
    fn from(problem: SafeCounterProblem) -> Self {
        match problem {
            SafeCounterProblem::Invalid => Self::Invalid,
            SafeCounterProblem::Negative => Self::Negative,
            SafeCounterProblem::UnsupportedPrecision => Self::UnsupportedPrecision,
            SafeCounterProblem::Overflow => Self::Overflow,
        }
    }
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum NormalizedCounter {
    Valid { kwh: String },
    Rejected { reason: SafeCounterProblem },
}

impl NormalizedCounter {
    pub(crate) fn parse_kwh(input: &str) -> Self {
        match CounterReading::parse_kwh(input) {
            CounterReading::Valid(energy) => Self::Valid {
                kwh: energy.to_string().trim_end_matches(" kWh").to_owned(),
            },
            CounterReading::Rejected(reason) => Self::Rejected {
                reason: reason.into(),
            },
        }
    }

    fn to_core(&self) -> Result<CounterReading, RejectionReason> {
        match self {
            Self::Valid { kwh } => match CounterReading::parse_kwh(kwh) {
                CounterReading::Valid(energy)
                    if energy.to_string().trim_end_matches(" kWh") == kwh =>
                {
                    Ok(CounterReading::Valid(energy))
                }
                _ => Err(RejectionReason::InvalidNormalizedEvent),
            },
            Self::Rejected { reason } => Ok(CounterReading::Rejected((*reason).into())),
        }
    }
}

impl fmt::Debug for NormalizedCounter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Valid { .. } => formatter.write_str("Valid { kwh: <redacted> }"),
            Self::Rejected { reason } => formatter
                .debug_struct("Rejected")
                .field("reason", reason)
                .finish(),
        }
    }
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedEvent {
    pub vehicle: String,
    pub connection: String,
    pub position: String,
    pub time: i64,
    pub kind: NormalizedEventKind,
    pub charge_type: NormalizedChargeType,
    pub counter: Option<NormalizedCounter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<NormalizedLocation>,
}

impl NormalizedEvent {
    pub fn to_core(&self) -> Result<Event, RejectionReason> {
        Ok(Event {
            vehicle: Some(
                VehicleId::new(&self.vehicle)
                    .map_err(|_| RejectionReason::InvalidNormalizedEvent)?,
            ),
            connection: ConnectionId::new(&self.connection)
                .map_err(|_| RejectionReason::InvalidNormalizedEvent)?,
            position: parse_position(&self.position)
                .map_err(|_| RejectionReason::InvalidNormalizedEvent)?,
            time: self.time,
            kind: self.kind.into(),
            charge_type: self.charge_type.into(),
            counter: self
                .counter
                .as_ref()
                .map(NormalizedCounter::to_core)
                .transpose()?,
            location: self
                .location
                .as_ref()
                .map(NormalizedLocation::to_core)
                .transpose()?
                .unwrap_or_default(),
        })
    }
}

impl fmt::Debug for NormalizedEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NormalizedEvent")
            .field("time", &self.time)
            .field("kind", &self.kind)
            .field("charge_type", &self.charge_type)
            .field("counter", &self.counter)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceProvenance {
    pub device: String,
    pub owner: String,
    pub created_at: String,
    pub input_contract: String,
    pub mapping_version: String,
    pub manifest_version: String,
    pub normalizer_version: String,
    pub timestamp_version: String,
    pub is_resend: bool,
}

impl fmt::Debug for EvidenceProvenance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EvidenceProvenance")
            .field("is_resend", &self.is_resend)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedEvidence {
    pub event: NormalizedEvent,
    pub provenance: EvidenceProvenance,
}

pub(crate) fn parse_position(position: &str) -> Result<u64, RejectionReason> {
    let value = position
        .parse::<u64>()
        .map_err(|_| RejectionReason::InvalidManifest)?;
    if value.to_string() != position {
        return Err(RejectionReason::InvalidManifest);
    }
    Ok(value)
}
