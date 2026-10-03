use std::collections::{BTreeMap, BTreeSet};

use crate::energy::Energy;
use crate::error::LedgerError;
use crate::event::Event;
use crate::identity::{ConnectionId, OwnerId, SessionId, VehicleId};
use crate::pricing::{PricingError, PricingSummary, summarize_pricing};
use crate::session::{
    ChargerClassification, ConnectionEvidence, EVIDENCE_LABEL, ExclusionReason, QualityFlag,
    ReconstructedSession, Session, reconstruct_session,
};
use crate::tariff::RateSchedule;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionReasons {
    pub id: SessionId,
    pub quality_flags: BTreeSet<QualityFlag>,
    pub exclusion_reasons: BTreeSet<ExclusionReason>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Summary {
    pub vehicle: VehicleId,
    pub owner: OwnerId,
    pub observed_ac: Energy,
    pub eligible_shared_charger: Energy,
    pub held_or_excluded: Vec<SessionReasons>,
    pub evidence_label: &'static str,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Ledger {
    owners: BTreeMap<VehicleId, OwnerId>,
    events: BTreeMap<VehicleId, BTreeMap<u64, BTreeSet<Event>>>,
    reviews: BTreeMap<SessionId, ChargerClassification>,
}

impl Ledger {
    pub fn register(&mut self, vehicle: VehicleId, owner: OwnerId) -> Result<(), LedgerError> {
        if self.owners.contains_key(&vehicle) {
            return Err(LedgerError::DuplicateVehicle);
        }
        self.owners.insert(vehicle, owner);
        Ok(())
    }

    fn owner(&self, vehicle: &VehicleId) -> Result<&OwnerId, LedgerError> {
        self.owners.get(vehicle).ok_or(LedgerError::UnknownVehicle)
    }

    pub fn ingest(&mut self, event: Event) -> Result<(), LedgerError> {
        let vehicle = event.vehicle.as_ref().ok_or(LedgerError::MissingVehicle)?;
        self.owner(vehicle)?;
        self.events
            .entry(vehicle.clone())
            .or_default()
            .entry(event.position)
            .or_default()
            .insert(event);
        Ok(())
    }

    pub fn classify(
        &mut self,
        scope: &VehicleId,
        target: &SessionId,
        classification: ChargerClassification,
    ) -> Result<(), LedgerError> {
        self.owner(scope)?;
        if scope != &target.vehicle {
            return Err(LedgerError::OutsideVehicleScope);
        }
        if !self.sessions(scope)?.iter().any(|s| &s.id == target) {
            return Err(LedgerError::UnknownSession);
        }
        self.reviews.insert(target.clone(), classification);
        Ok(())
    }

    pub fn sessions(&self, scope: &VehicleId) -> Result<Vec<Session>, LedgerError> {
        Ok(self
            .reconstruct(scope)?
            .into_iter()
            .map(|result| result.session)
            .collect())
    }

    pub fn pricing(
        &self,
        scope: &VehicleId,
        rates: &RateSchedule,
    ) -> Result<PricingSummary, PricingError> {
        let owner = self.owner(scope).map_err(PricingError::Ledger)?.clone();
        let sessions = self.reconstruct(scope).map_err(PricingError::Ledger)?;
        summarize_pricing(scope.clone(), owner, sessions, rates)
    }

    fn reconstruct(&self, scope: &VehicleId) -> Result<Vec<ReconstructedSession>, LedgerError> {
        let owner = self.owner(scope)?;
        let mut connections: BTreeMap<ConnectionId, Vec<ConnectionEvidence<'_>>> = BTreeMap::new();
        if let Some(positions) = self.events.get(scope) {
            for values in positions.values() {
                for event in values {
                    connections
                        .entry(event.connection.clone())
                        .or_default()
                        .push(ConnectionEvidence {
                            event,
                            is_conflicting: values.len() > 1,
                        });
                }
            }
        }
        let mut sessions = Vec::new();
        for (connection, mut evidence) in connections {
            evidence.sort_by_key(|entry| (entry.event.time, entry.event.position));
            let id = SessionId {
                vehicle: scope.clone(),
                connection,
            };
            let classification = self.reviews.get(&id).copied().unwrap_or_default();
            sessions.push(reconstruct_session(
                id,
                owner.clone(),
                &evidence,
                classification,
            )?);
        }
        Ok(sessions)
    }

    pub fn summary(&self, scope: &VehicleId) -> Result<Summary, LedgerError> {
        let mut summary = Summary {
            vehicle: scope.clone(),
            owner: self.owner(scope)?.clone(),
            observed_ac: Energy::ZERO,
            eligible_shared_charger: Energy::ZERO,
            held_or_excluded: Vec::new(),
            evidence_label: EVIDENCE_LABEL,
        };
        for session in self.sessions(scope)? {
            summary.observed_ac = summary
                .observed_ac
                .checked_add(session.observed_ac)
                .map_err(|_| LedgerError::EnergyOverflow)?;
            if session.is_eligible() {
                summary.eligible_shared_charger = summary
                    .eligible_shared_charger
                    .checked_add(session.observed_ac)
                    .map_err(|_| LedgerError::EnergyOverflow)?;
            } else {
                summary.held_or_excluded.push(SessionReasons {
                    id: session.id,
                    quality_flags: session.quality_flags,
                    exclusion_reasons: session.exclusion_reasons,
                });
            }
        }
        Ok(summary)
    }
}
