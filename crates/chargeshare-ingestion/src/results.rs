use chargeshare_core::{ChargerClassification, Energy, OwnerId, Session, VehicleId};
use serde::Serialize;

use crate::{IngestionError, Store};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScopedResults {
    pub owner: String,
    pub vehicle: String,
    pub state: &'static str,
    pub evidence_boundary: &'static str,
    pub observed_ac_kwh: String,
    pub eligible_shared_charger_kwh: String,
    pub sessions: Vec<SessionResult>,
    pub provenance: ResultProvenance,
    pub revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionResult {
    pub vehicle: String,
    pub connection: String,
    pub classification: &'static str,
    pub evidence_state: &'static str,
    pub observed_ac_kwh: String,
    pub eligible_shared_charger_kwh: String,
    pub quality_flags: Vec<String>,
    pub exclusion_reasons: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResultProvenance {
    pub source: &'static str,
    pub source_epoch: String,
    pub input_contract: String,
    pub mapping_version: String,
    pub manifest_version: String,
    pub normalizer_version: String,
    pub calculation_version: &'static str,
}

impl Store {
    pub fn results(
        &self,
        owner: &OwnerId,
        vehicle: &VehicleId,
    ) -> Result<ScopedResults, IngestionError> {
        self.validate_live_storage()?;
        let ledger = self.replay(owner, vehicle)?;
        let summary = ledger
            .summary(vehicle)
            .map_err(|_| IngestionError::DomainReplay)?;
        let sessions = ledger
            .sessions(vehicle)
            .map_err(|_| IngestionError::DomainReplay)?;
        if sessions
            .iter()
            .any(|session| session.id.vehicle != *vehicle || session.owner != *owner)
        {
            return Err(IngestionError::StorageCorrupt);
        }
        let sessions: Vec<_> = sessions.into_iter().map(SessionResult::from).collect();
        let config = self.manifest.config();
        let mut result = ScopedResults {
            owner: owner.as_str().to_owned(),
            vehicle: vehicle.as_str().to_owned(),
            state: if sessions.is_empty() { "empty" } else { "held" },
            evidence_boundary: summary.evidence_label,
            observed_ac_kwh: kwh(summary.observed_ac),
            eligible_shared_charger_kwh: kwh(summary.eligible_shared_charger),
            sessions,
            provenance: ResultProvenance {
                source: "synthetic-receiver-kafka",
                source_epoch: self.source_epoch().to_owned(),
                input_contract: config.input_contract.clone(),
                mapping_version: config.mapping_version.clone(),
                manifest_version: config.manifest_version.clone(),
                normalizer_version: config.normalizer_version.clone(),
                calculation_version: "scoped-core-replay-v1",
            },
            revision: String::new(),
        };
        let canonical = serde_json::to_vec(&result).map_err(|_| IngestionError::DomainReplay)?;
        let hash = canonical.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        result.revision = format!("results-v1-{hash:016x}");
        Ok(result)
    }
}

impl From<Session> for SessionResult {
    fn from(session: Session) -> Self {
        let eligible = if session.is_eligible() {
            session.observed_ac
        } else {
            Energy::ZERO
        };
        Self {
            vehicle: session.id.vehicle.as_str().to_owned(),
            connection: session.id.connection.as_str().to_owned(),
            classification: match session.classification {
                ChargerClassification::Unconfirmed => "Unconfirmed",
                ChargerClassification::SharedCharger => "SharedCharger",
                ChargerClassification::OtherCharger => "OtherCharger",
            },
            evidence_state: if !session.quality_flags.is_empty() {
                "incomplete"
            } else if session.observed_ac == Energy::ZERO {
                "complete_zero"
            } else {
                "complete"
            },
            observed_ac_kwh: kwh(session.observed_ac),
            eligible_shared_charger_kwh: kwh(eligible),
            quality_flags: session
                .quality_flags
                .iter()
                .map(|flag| format!("{flag:?}"))
                .collect(),
            exclusion_reasons: session
                .exclusion_reasons
                .iter()
                .map(|reason| format!("{reason:?}"))
                .collect(),
        }
    }
}

fn kwh(energy: Energy) -> String {
    energy.to_string().trim_end_matches(" kWh").to_owned()
}
