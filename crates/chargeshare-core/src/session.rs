use std::collections::{BTreeMap, BTreeSet};

use crate::energy::{CounterProblem, Energy};
use crate::error::LedgerError;
use crate::event::{ChargeType, CounterReading, Event, EventKind};
use crate::identity::{OwnerId, SessionId};
use crate::location::{ChargerGeofence, LocationSample};

pub const EVIDENCE_LABEL: &str =
    "Synthetic review: vehicle-reported AC energy; physical accuracy unvalidated";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QualityFlag {
    MissingConnectionStart,
    MissingConnectionEnd,
    MissingBaseline,
    MissingTerminalSample,
    InvalidCounter(CounterProblem),
    CounterRollback,
    ConflictingEvidence,
    InvalidBoundaryOrder,
    InsufficientCounterReadings,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChargerClassification {
    #[default]
    Held,
    SharedCharger,
    OtherCharger,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassificationReason {
    InsideGeofence,
    OutsideGeofence,
    LocationUnavailable,
    LocationAmbiguous,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ExclusionReason {
    LocationUnavailable,
    LocationAmbiguous,
    OtherCharger,
    DcEvidence,
    AmbiguousChargeType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub evidence_label: &'static str,
    pub id: SessionId,
    pub owner: OwnerId,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub observed_ac: Energy,
    pub quality_flags: BTreeSet<QualityFlag>,
    pub exclusion_reasons: BTreeSet<ExclusionReason>,
    pub classification: ChargerClassification,
    pub classification_reason: ClassificationReason,
    pub observed_window: Option<ObservedCounterWindow>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObservedCounterWindow {
    pub first_time: Option<i64>,
    pub last_time: Option<i64>,
    pub usable_readings: usize,
}

impl Session {
    pub fn is_eligible(&self) -> bool {
        self.exclusion_reasons.is_empty()
            && self.quality_flags.iter().all(|flag| {
                self.observed_window.is_some()
                    && matches!(
                        flag,
                        QualityFlag::MissingBaseline | QualityFlag::MissingTerminalSample
                    )
            })
    }
}

pub(crate) struct ConnectionEvidence<'a> {
    pub(crate) event: &'a Event,
    pub(crate) is_conflicting: bool,
}

#[derive(Default)]
struct LocationCoverage {
    inside: bool,
    outside: bool,
}

pub(crate) fn classify_location(
    evidence: &[ConnectionEvidence<'_>],
    geofence: Option<ChargerGeofence>,
) -> (ChargerClassification, ClassificationReason) {
    let mut samples = BTreeMap::<u64, LocationCoverage>::new();
    let mut conflicting = false;
    for entry in evidence {
        let sample = samples.entry(entry.event.position).or_default();
        match entry.event.location {
            LocationSample::InsideGeofence => sample.inside = true,
            LocationSample::OutsideGeofence => sample.outside = true,
            LocationSample::Valid(point) => {
                if let Some(geofence) = geofence {
                    if geofence.contains(point) {
                        sample.inside = true;
                    } else {
                        sample.outside = true;
                    }
                }
            }
            LocationSample::Missing | LocationSample::Invalid => {}
        }
        conflicting |= entry.is_conflicting;
    }
    let inside = samples.values().any(|sample| sample.inside);
    let outside = samples.values().any(|sample| sample.outside);
    let unavailable = samples
        .values()
        .any(|sample| !sample.inside && !sample.outside);
    if !inside && !outside {
        (
            ChargerClassification::Held,
            ClassificationReason::LocationUnavailable,
        )
    } else if conflicting || unavailable || (inside && outside) {
        (
            ChargerClassification::Held,
            ClassificationReason::LocationAmbiguous,
        )
    } else if inside {
        (
            ChargerClassification::SharedCharger,
            ClassificationReason::InsideGeofence,
        )
    } else {
        (
            ChargerClassification::OtherCharger,
            ClassificationReason::OutsideGeofence,
        )
    }
}

pub(crate) struct CounterSegment {
    pub(crate) start_time: i64,
    pub(crate) end_time: i64,
    pub(crate) energy: Energy,
}

pub(crate) struct ReconstructedSession {
    pub(crate) session: Session,
    pub(crate) segments: Vec<CounterSegment>,
}

pub(crate) fn reconstruct_session(
    id: SessionId,
    owner: OwnerId,
    evidence: &[ConnectionEvidence<'_>],
    classification: ChargerClassification,
    classification_reason: ClassificationReason,
    observed_window: bool,
) -> Result<ReconstructedSession, LedgerError> {
    let mut session = Session {
        evidence_label: EVIDENCE_LABEL,
        id,
        owner,
        start_time: None,
        end_time: None,
        observed_ac: Energy::ZERO,
        quality_flags: BTreeSet::new(),
        exclusion_reasons: BTreeSet::new(),
        classification,
        classification_reason,
        observed_window: observed_window.then(ObservedCounterWindow::default),
    };
    match classification {
        ChargerClassification::Held => {
            session
                .exclusion_reasons
                .insert(match classification_reason {
                    ClassificationReason::LocationAmbiguous => ExclusionReason::LocationAmbiguous,
                    _ => ExclusionReason::LocationUnavailable,
                });
        }
        ChargerClassification::OtherCharger => {
            session
                .exclusion_reasons
                .insert(ExclusionReason::OtherCharger);
        }
        ChargerClassification::SharedCharger => {}
    }
    let mut has_baseline = false;
    let mut has_terminal_sample = false;
    let mut previous_counter: Option<(Energy, i64)> = None;
    let mut segments = Vec::new();
    let resolved_ac = observed_window
        && evidence
            .iter()
            .any(|entry| !entry.is_conflicting && entry.event.charge_type == ChargeType::Ac);
    for (index, entry) in evidence.iter().enumerate() {
        let event = entry.event;
        let charge_type = if resolved_ac && event.charge_type == ChargeType::Unknown {
            ChargeType::Ac
        } else {
            event.charge_type
        };
        match charge_type {
            ChargeType::Dc => {
                session
                    .exclusion_reasons
                    .insert(ExclusionReason::DcEvidence);
            }
            ChargeType::Ambiguous | ChargeType::Unknown => {
                session
                    .exclusion_reasons
                    .insert(ExclusionReason::AmbiguousChargeType);
            }
            ChargeType::Ac => {}
        }
        if entry.is_conflicting {
            session
                .quality_flags
                .insert(QualityFlag::ConflictingEvidence);
            if let Some(CounterReading::Rejected(problem)) = event.counter {
                session
                    .quality_flags
                    .insert(QualityFlag::InvalidCounter(problem));
            }
            previous_counter = None;
            continue;
        }
        if event.kind == EventKind::Start {
            if session.start_time.is_some() || index != 0 {
                session
                    .quality_flags
                    .insert(QualityFlag::InvalidBoundaryOrder);
            }
            session.start_time.get_or_insert(event.time);
            has_baseline |= matches!(event.counter, Some(CounterReading::Valid(_)));
        }
        if event.kind == EventKind::End {
            if session.end_time.is_some() || index + 1 != evidence.len() {
                session
                    .quality_flags
                    .insert(QualityFlag::InvalidBoundaryOrder);
            }
            session.end_time.get_or_insert(event.time);
            has_terminal_sample |= matches!(event.counter, Some(CounterReading::Valid(_)));
        }
        match event.counter {
            Some(CounterReading::Rejected(problem)) => {
                session
                    .quality_flags
                    .insert(QualityFlag::InvalidCounter(problem));
                previous_counter = None;
            }
            Some(CounterReading::Valid(current)) => {
                if charge_type == ChargeType::Ac {
                    if let Some(window) = &mut session.observed_window {
                        window.first_time.get_or_insert(event.time);
                        window.last_time = Some(event.time);
                        window.usable_readings += 1;
                    }
                    if let Some((prior, start_time)) = previous_counter {
                        match current.checked_delta_from(prior) {
                            Some(delta) => {
                                session.observed_ac = session
                                    .observed_ac
                                    .checked_add(delta)
                                    .map_err(|_| LedgerError::EnergyOverflow)?;
                                segments.push(CounterSegment {
                                    start_time,
                                    end_time: event.time,
                                    energy: delta,
                                });
                            }
                            None => {
                                session.quality_flags.insert(QualityFlag::CounterRollback);
                            }
                        }
                    }
                    previous_counter = Some((current, event.time));
                } else {
                    previous_counter = None;
                }
            }
            None => {
                if charge_type != ChargeType::Ac {
                    previous_counter = None;
                }
            }
        }
    }
    if session.start_time.is_none() {
        session
            .quality_flags
            .insert(QualityFlag::MissingConnectionStart);
    }
    if session.end_time.is_none() {
        session
            .quality_flags
            .insert(QualityFlag::MissingConnectionEnd);
    }
    if !has_baseline {
        session.quality_flags.insert(QualityFlag::MissingBaseline);
    }
    if !has_terminal_sample {
        session
            .quality_flags
            .insert(QualityFlag::MissingTerminalSample);
    }
    if session
        .observed_window
        .as_ref()
        .is_some_and(|window| window.usable_readings < 2)
    {
        session
            .quality_flags
            .insert(QualityFlag::InsufficientCounterReadings);
    }
    Ok(ReconstructedSession { session, segments })
}
