use std::collections::BTreeSet;

use crate::energy::{CounterProblem, Energy};
use crate::error::LedgerError;
use crate::event::{ChargeType, CounterReading, Event, EventKind};
use crate::identity::{OwnerId, SessionId};

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
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChargerClassification {
    #[default]
    Unconfirmed,
    SharedCharger,
    OtherCharger,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ExclusionReason {
    UnconfirmedCharger,
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
}
impl Session {
    pub fn is_eligible(&self) -> bool {
        self.quality_flags.is_empty() && self.exclusion_reasons.is_empty()
    }
}

pub(crate) struct ConnectionEvidence<'a> {
    pub(crate) event: &'a Event,
    pub(crate) is_conflicting: bool,
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
    };
    match classification {
        ChargerClassification::Unconfirmed => {
            session
                .exclusion_reasons
                .insert(ExclusionReason::UnconfirmedCharger);
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
    for (index, entry) in evidence.iter().enumerate() {
        let event = entry.event;
        match event.charge_type {
            ChargeType::Dc => {
                session
                    .exclusion_reasons
                    .insert(ExclusionReason::DcEvidence);
            }
            ChargeType::Ambiguous => {
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
                if event.charge_type == ChargeType::Ac {
                    if let Some((prior, start_time)) = previous_counter {
                        match current.checked_delta_from(prior) {
                            Some(delta) => {
                                session.observed_ac = session
                                    .observed_ac
                                    .checked_add(delta)
                                    .map_err(|_| LedgerError::EnergyOverflow)?;
                                if delta != Energy::ZERO {
                                    segments.push(CounterSegment {
                                        start_time,
                                        end_time: event.time,
                                        energy: delta,
                                    });
                                }
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
                if event.charge_type != ChargeType::Ac {
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
    Ok(ReconstructedSession { session, segments })
}
