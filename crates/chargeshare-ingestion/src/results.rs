use chargeshare_core::{
    CHARGING_PERIODS, ChargerClassification, ChargingEvidence, ChargingTotal, Energy,
    MonthlyHistory, MonthlyHistoryError, OwnerId, PricingError, PricingHold, QualityFlag, Session,
    TOU_CALCULATION_VERSION, TimeOfUseQuote, VehicleId,
};
use serde::Serialize;

use crate::{IngestionError, Store};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScopedResults {
    pub owner: String,
    pub vehicle: String,
    pub last_charge: Option<crate::LastCharge>,
    pub state: &'static str,
    pub evidence_boundary: &'static str,
    pub observed_ac_kwh: String,
    pub eligible_shared_charger_kwh: String,
    pub sessions: Vec<SessionResult>,
    pub pricing: PricingResult,
    pub monthly: MonthlyResult,
    pub provenance: ResultProvenance,
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<ResultRegistry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionResult {
    pub vehicle: String,
    pub connection: String,
    pub start_utc_seconds: Option<String>,
    pub end_utc_seconds: Option<String>,
    pub classification: &'static str,
    pub classification_reason: String,
    pub evidence_state: &'static str,
    pub observed_ac_kwh: String,
    pub eligible_shared_charger_kwh: String,
    pub quality_flags: Vec<String>,
    pub exclusion_reasons: Vec<String>,
    pub pricing: SessionPrice,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<CounterCoverageResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charger: Option<SessionChargerResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_versions: Option<Vec<SessionLocationVersion>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChargerSummary {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResultRegistry {
    pub car_name: String,
    pub chargers: Vec<ChargerSummary>,
    pub selected_charger: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionChargerResult {
    pub id: String,
    pub name: String,
    pub assignment_versions: Vec<String>,
    pub geofence_versions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct SessionLocationVersion {
    pub charger_id: String,
    pub assignment_version: String,
    pub geofence_version: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CounterCoverageResult {
    pub connection_start_utc_seconds: Option<String>,
    pub connection_end_utc_seconds: Option<String>,
    pub first_counter_utc_seconds: Option<String>,
    pub last_counter_utc_seconds: Option<String>,
    pub usable_readings: usize,
    pub partial_start: bool,
    pub partial_end: bool,
    pub quantization: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChargingTotalResult {
    pub ac_kwh: String,
    pub exact_cost: String,
    pub display_cost: String,
}

impl From<ChargingTotal> for ChargingTotalResult {
    fn from(total: ChargingTotal) -> Self {
        Self {
            ac_kwh: kwh(total.energy),
            exact_cost: total.exact_cost.to_string(),
            display_cost: display_usd(total.exact_cost.round_half_up_to_cents()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PeriodTotalResult {
    pub period: &'static str,
    #[serde(flatten)]
    pub total: ChargingTotalResult,
}

fn period_totals(periods: [ChargingTotal; 3]) -> Vec<PeriodTotalResult> {
    CHARGING_PERIODS
        .into_iter()
        .zip(periods)
        .map(|(period, total)| PeriodTotalResult {
            period,
            total: total.into(),
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChargingEvidenceResult {
    pub quoted_sessions: usize,
    pub held_sessions: usize,
    pub other_sessions: usize,
    pub partial_sessions: usize,
    pub complete_zero_sessions: usize,
    pub quality_flags: Vec<String>,
}

impl From<ChargingEvidence> for ChargingEvidenceResult {
    fn from(evidence: ChargingEvidence) -> Self {
        Self {
            quoted_sessions: evidence.quoted_sessions,
            held_sessions: evidence.held_sessions,
            other_sessions: evidence.other_sessions,
            partial_sessions: evidence.partial_sessions,
            complete_zero_sessions: evidence.complete_zero_sessions,
            quality_flags: evidence
                .quality_flags
                .iter()
                .map(|flag| format!("{flag:?}"))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DayResult {
    pub local_date: String,
    pub future: bool,
    pub periods: Vec<PeriodTotalResult>,
    pub total: Option<ChargingTotalResult>,
    pub evidence: ChargingEvidenceResult,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MonthResult {
    pub month: String,
    pub first_date: String,
    pub last_date: String,
    pub periods: Vec<PeriodTotalResult>,
    pub total: Option<ChargingTotalResult>,
    pub evidence: ChargingEvidenceResult,
    pub days: Vec<DayResult>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MonthlyResult {
    pub timezone: &'static str,
    pub today: String,
    pub evidence: ChargingEvidenceResult,
    pub undated_sessions: usize,
    pub months: Vec<MonthResult>,
}

impl From<MonthlyHistory> for MonthlyResult {
    fn from(history: MonthlyHistory) -> Self {
        Self {
            timezone: "America/Los_Angeles",
            today: history.today.to_string(),
            evidence: history.evidence.into(),
            undated_sessions: history.undated_sessions,
            months: history
                .months
                .into_iter()
                .map(|month| MonthResult {
                    month: month.first_date.format("%Y-%m").to_string(),
                    first_date: month.first_date.to_string(),
                    last_date: month.last_date.to_string(),
                    periods: period_totals(month.periods),
                    total: (month.evidence.quoted_sessions > 0).then(|| month.total.into()),
                    evidence: month.evidence.into(),
                    days: month
                        .days
                        .into_iter()
                        .map(|day| DayResult {
                            local_date: day.local_date.to_string(),
                            future: day.future,
                            periods: period_totals(day.periods),
                            total: (day.evidence.quoted_sessions > 0).then(|| day.total.into()),
                            evidence: day.evidence.into(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

pub const LIVE_QUANTIZATION: &str = "Live counters quantized once to 0.000001 kWh, decimal half-up";
pub const LIVE_PRICING_BOUNDARY: &str = "Public rate estimate UNVERIFIED against a real bill; charger-assigned PG&E bundled or PG&E delivery plus CleanPowerSF Green generation and 2018 PCIA/franchise fees; observed-window AC-input counter energy allocated by elapsed UTC time; physical accuracy unvalidated; excludes Base Services Charge, Climate Credit and taxes; not a bill";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResultProvenance {
    pub source: &'static str,
    pub source_epoch: String,
    pub input_contract: String,
    pub mapping_version: String,
    pub manifest_version: String,
    pub normalizer_version: String,
    pub calculation_version: &'static str,
    pub geofence_version: String,
}

pub const PRICING_BOUNDARY: &str = "Synthetic public rate estimate UNVERIFIED against a real bill; PG&E bundled or PG&E delivery plus CleanPowerSF Green generation and 2018 PCIA/franchise fees; AC-input counter energy allocated by elapsed UTC time; physical accuracy unvalidated; excludes Base Services Charge, Climate Credit and taxes; not a bill";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PricingResult {
    pub state: &'static str,
    pub charger_id: Option<String>,
    pub rate_models: Vec<crate::RateModelConfig>,
    pub calculation_version: &'static str,
    pub estimate_boundary: &'static str,
    pub priced_ac_kwh: String,
    pub exact_subtotal: Option<String>,
    pub display_subtotal: Option<String>,
    pub quoted_session_count: usize,
    pub rate_held_session_count: usize,
    pub ineligible_session_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionPrice {
    pub state: &'static str,
    pub hold_reason: Option<&'static str>,
    pub quote: Option<QuoteResult>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QuoteResult {
    pub exact_cost: String,
    pub display_cost: String,
    pub slices: Vec<SliceResult>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SliceResult {
    pub start_utc_seconds: String,
    pub end_utc_seconds: String,
    pub local_date: String,
    pub local_start: String,
    pub local_end: String,
    pub utc_offset_seconds: String,
    pub season: String,
    pub day_type: String,
    pub period: String,
    pub model_version: String,
    pub allocated_ac_kwh: String,
    pub usd_per_kwh: String,
    pub exact_cost: String,
}

impl SessionPrice {
    fn from_quote(quote: Result<TimeOfUseQuote, PricingHold>) -> Self {
        match quote {
            Ok(quote) => Self {
                state: "quoted",
                hold_reason: None,
                quote: Some(QuoteResult {
                    exact_cost: quote.exact_cost.to_string(),
                    display_cost: display_usd(quote.exact_cost.round_half_up_to_cents()),
                    slices: quote
                        .slices
                        .into_iter()
                        .map(|slice| SliceResult {
                            start_utc_seconds: slice.start_time.to_string(),
                            end_utc_seconds: slice.end_time.to_string(),
                            local_date: slice.local_date,
                            local_start: slice.local_start,
                            local_end: slice.local_end,
                            utc_offset_seconds: slice.utc_offset_seconds.to_string(),
                            season: format!("{:?}", slice.season),
                            day_type: format!("{:?}", slice.day_type),
                            period: slice.period,
                            model_version: slice.model_version,
                            allocated_ac_kwh: kwh(slice.energy),
                            usd_per_kwh: slice.rate.to_string(),
                            exact_cost: slice.exact_cost.to_string(),
                        })
                        .collect(),
                }),
            },
            Err(hold) => Self {
                state: if hold == PricingHold::IneligibleSession {
                    "ineligible"
                } else {
                    "rate_held"
                },
                hold_reason: Some(match hold {
                    PricingHold::IneligibleSession => "IneligibleSession",
                    PricingHold::MissingRate { .. } => "MissingRate",
                    PricingHold::UnresolvedRateBoundary { .. } => "UnresolvedRateBoundary",
                    PricingHold::NonIncreasingTime { .. } => "NonIncreasingTime",
                }),
                quote: None,
            },
        }
    }
}

fn display_usd(cents: chargeshare_core::UsdCents) -> String {
    format!("${}", cents.to_string().trim_end_matches(" USD"))
}

impl Store {
    pub fn results(
        &self,
        owner: &OwnerId,
        vehicle: &VehicleId,
    ) -> Result<ScopedResults, IngestionError> {
        self.validate_live_storage()?;
        let ledger = self.replay(owner, vehicle)?;
        results_for_view(
            &ledger,
            &self.manifest,
            self.source_epoch(),
            owner,
            vehicle,
            Some(self),
            None,
        )
    }
}

#[cfg(test)]
fn results_from_replay(
    ledger: &chargeshare_core::Ledger,
    manifest: &crate::Manifest,
    source_epoch: &str,
    owner: &OwnerId,
    vehicle: &VehicleId,
) -> Result<ScopedResults, IngestionError> {
    results_for_view(ledger, manifest, source_epoch, owner, vehicle, None, None)
}

fn results_for_view(
    ledger: &chargeshare_core::Ledger,
    manifest: &crate::Manifest,
    source_epoch: &str,
    owner: &OwnerId,
    vehicle: &VehicleId,
    store: Option<&Store>,
    selected_charger: Option<&str>,
) -> Result<ScopedResults, IngestionError> {
    let summary = ledger
        .summary(vehicle)
        .map_err(|_| IngestionError::DomainReplay)?;
    let history = manifest
        .rate_history(owner)
        .map_err(|_| IngestionError::InvalidConfiguration)?;
    let mut prices =
        ledger
            .time_of_use_pricing(vehicle, &history)
            .map_err(|error| match error {
                PricingError::Money(_) | PricingError::EnergyOverflow => {
                    IngestionError::PricingOverflow
                }
                PricingError::Ledger(_) => IngestionError::DomainReplay,
            })?;
    let mut registry_result = None;
    let mut session_chargers = std::collections::BTreeMap::new();
    let mut selected_models = Vec::new();
    if let Some(store) = store
        && let Some(registry) = &store.registry
    {
        let car = registry.car(vehicle.as_str())?;
        if store
            .pending_recoveries()?
            .iter()
            .any(|pending| pending == vehicle.as_str())
        {
            return Err(IngestionError::RecoveryPending);
        }
        if selected_charger.is_some_and(|id| {
            !car.registration
                .chargers
                .iter()
                .any(|charger| charger == id)
        }) {
            return Err(IngestionError::RegistryConflict);
        }
        let matches = store.connection_chargers(vehicle.as_str())?;
        let mut charger_prices = std::collections::BTreeMap::new();
        for id in &car.registration.chargers {
            let charger = registry
                .chargers
                .get(id)
                .ok_or(IngestionError::RegistryIntegrity)?;
            let history = crate::charger_rates::history(&store.connection, charger)?;
            let models = history
                .versions()
                .iter()
                .map(|version| {
                    manifest
                        .config()
                        .rate_models
                        .iter()
                        .find(|model| model.id == version.definition().id)
                        .ok_or(IngestionError::RegistryIntegrity)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let pricing = ledger
                .time_of_use_pricing(vehicle, &history)
                .map_err(|_| IngestionError::PricingOverflow)?;
            charger_prices.insert(
                id.clone(),
                pricing
                    .sessions
                    .into_iter()
                    .map(|priced| {
                        (
                            priced.session.id.connection.as_str().to_owned(),
                            priced.quote,
                        )
                    })
                    .collect::<std::collections::BTreeMap<_, _>>(),
            );
            if selected_charger.is_none_or(|selected| selected == id) {
                selected_models.extend(models.into_iter().cloned());
            }
        }
        prices.sessions.retain(|priced| {
            selected_charger.is_none_or(|id| {
                matches
                    .get(priced.session.id.connection.as_str())
                    .is_some_and(|ids| ids.contains(id))
            })
        });
        for priced in &mut prices.sessions {
            let connection = priced.session.id.connection.as_str();
            if let Some(ids) = matches.get(connection)
                && ids.len() == 1
            {
                let id = ids.first().unwrap();
                let charger = registry
                    .chargers
                    .get(id)
                    .ok_or(IngestionError::RegistryIntegrity)?;
                priced.quote = charger_prices
                    .get(id)
                    .and_then(|prices| prices.get(connection))
                    .ok_or(IngestionError::RegistryIntegrity)?
                    .clone();
                let (assignments, geofences) =
                    store.session_location_versions(vehicle.as_str(), connection, id)?;
                session_chargers.insert(
                    connection.to_owned(),
                    SessionChargerResult {
                        id: id.clone(),
                        name: charger.name.clone(),
                        assignment_versions: assignments,
                        geofence_versions: geofences,
                    },
                );
            }
        }
        prices.priced_energy = Energy::ZERO;
        prices.exact_subtotal = chargeshare_core::ExactUsd::ZERO;
        for priced in &prices.sessions {
            if let Ok(quote) = &priced.quote {
                prices.priced_energy = prices
                    .priced_energy
                    .checked_add(priced.session.observed_ac)
                    .map_err(|_| IngestionError::PricingOverflow)?;
                prices.exact_subtotal = prices
                    .exact_subtotal
                    .checked_add(quote.exact_cost)
                    .map_err(|_| IngestionError::PricingOverflow)?;
            }
        }
        prices.rounded_subtotal = prices.exact_subtotal.round_half_up_to_cents();
        selected_models.sort_by(|left, right| left.id.cmp(&right.id));
        selected_models.dedup_by(|left, right| left.id == right.id);
        registry_result = Some(ResultRegistry {
            car_name: car.registration.name.clone(),
            chargers: car
                .registration
                .chargers
                .iter()
                .filter_map(|id| registry.chargers.get(id))
                .map(|charger| ChargerSummary {
                    id: charger.id.clone(),
                    name: charger.name.clone(),
                })
                .collect(),
            selected_charger: selected_charger.map(str::to_owned),
        });
    }
    let mut observed = Energy::ZERO;
    let mut eligible = Energy::ZERO;
    for priced in &prices.sessions {
        observed = observed
            .checked_add(priced.session.observed_ac)
            .map_err(|_| IngestionError::PricingOverflow)?;
        if priced.session.is_eligible() {
            eligible = eligible
                .checked_add(priced.session.observed_ac)
                .map_err(|_| IngestionError::PricingOverflow)?;
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| IngestionError::DomainReplay)?
        .as_secs();
    let monthly = prices
        .monthly_history(i64::try_from(now).map_err(|_| IngestionError::DomainReplay)?)
        .map_err(|error| match error {
            MonthlyHistoryError::Calendar => IngestionError::DomainReplay,
            MonthlyHistoryError::EnergyOverflow | MonthlyHistoryError::Money(_) => {
                IngestionError::PricingOverflow
            }
        })?
        .into();
    let all_eligible = prices
        .sessions
        .iter()
        .all(|priced| priced.session.is_eligible());
    let mut sessions = Vec::new();
    for priced in prices.sessions {
        if priced.session.id.vehicle != *vehicle || priced.session.owner != *owner {
            return Err(IngestionError::StorageCorrupt);
        }
        let mut session = SessionResult::from(priced.session);
        session.pricing = SessionPrice::from_quote(priced.quote);
        session.charger = session_chargers.remove(&session.connection);
        if let Some(store) = store
            && store.registry.is_some()
        {
            session.location_versions =
                Some(store.session_location_provenance(vehicle.as_str(), &session.connection)?);
        }
        sessions.push(session);
    }
    let quoted_session_count = sessions
        .iter()
        .filter(|session| session.pricing.state == "quoted")
        .count();
    let rate_held_session_count = sessions
        .iter()
        .filter(|session| session.pricing.state == "rate_held")
        .count();
    let ineligible_session_count = sessions.len() - quoted_session_count - rate_held_session_count;
    let config = manifest.config();
    let mut result = ScopedResults {
        owner: owner.as_str().to_owned(),
        vehicle: vehicle.as_str().to_owned(),
        last_charge: match store {
            Some(store) => store.last_charge(
                vehicle,
                &ledger
                    .sessions(vehicle)
                    .map_err(|_| IngestionError::DomainReplay)?,
            )?,
            None => None,
        },
        state: if sessions.is_empty() {
            "empty"
        } else if all_eligible {
            "eligible"
        } else {
            "held"
        },
        evidence_boundary: if manifest.is_live() {
            "Vehicle-reported AC input counter energy; physical accuracy unvalidated"
        } else {
            summary.evidence_label
        },
        observed_ac_kwh: kwh(observed),
        eligible_shared_charger_kwh: kwh(eligible),
        pricing: PricingResult {
            state: if sessions.is_empty() {
                "empty"
            } else if rate_held_session_count + ineligible_session_count > 0 {
                "held"
            } else {
                "quoted"
            },
            charger_id: if registry_result.is_some() {
                selected_charger.map(str::to_owned)
            } else {
                config
                    .owner_chargers
                    .iter()
                    .find(|charger| charger.owner_id == owner.as_str())
                    .map(|charger| charger.charger_id.clone())
            },
            rate_models: if registry_result.is_some() {
                selected_models
            } else {
                history
                    .versions()
                    .iter()
                    .filter_map(|version| {
                        config
                            .rate_models
                            .iter()
                            .find(|model| model.id == version.definition().id)
                            .cloned()
                    })
                    .collect()
            },
            calculation_version: TOU_CALCULATION_VERSION,
            estimate_boundary: if manifest.is_live() {
                LIVE_PRICING_BOUNDARY
            } else {
                PRICING_BOUNDARY
            },
            priced_ac_kwh: kwh(prices.priced_energy),
            exact_subtotal: (quoted_session_count > 0).then(|| prices.exact_subtotal.to_string()),
            display_subtotal: (quoted_session_count > 0)
                .then(|| display_usd(prices.rounded_subtotal)),
            quoted_session_count,
            rate_held_session_count,
            ineligible_session_count,
        },
        sessions,
        monthly,
        provenance: ResultProvenance {
            source: if manifest.is_live() {
                "live-receiver-kafka"
            } else {
                "synthetic-receiver-kafka"
            },
            source_epoch: source_epoch.to_owned(),
            input_contract: config.input_contract.clone(),
            mapping_version: config.mapping_version.clone(),
            manifest_version: config.manifest_version.clone(),
            normalizer_version: config.normalizer_version.clone(),
            calculation_version: "scoped-core-replay-v3",
            geofence_version: config.geofence_version.clone(),
        },
        registry: registry_result,
        revision: String::new(),
    };
    if let Some(store) = store
        && let Some(registry) = &store.registry
    {
        result.provenance.mapping_version =
            registry.car(vehicle.as_str())?.identity_version.clone();
    }
    let canonical = serde_json::to_vec(&result).map_err(|_| IngestionError::DomainReplay)?;
    let hash = canonical.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    result.revision = format!("results-v2-{hash:016x}");
    Ok(result)
}

impl From<Session> for SessionResult {
    fn from(session: Session) -> Self {
        let eligible = if session.is_eligible() {
            session.observed_ac
        } else {
            Energy::ZERO
        };
        Self {
            charger: None,
            location_versions: None,
            pricing: SessionPrice {
                state: "ineligible",
                hold_reason: Some("IneligibleSession"),
                quote: None,
            },
            vehicle: session.id.vehicle.as_str().to_owned(),
            connection: session.id.connection.as_str().to_owned(),
            start_utc_seconds: session.start_time.map(|time| time.to_string()),
            end_utc_seconds: session.end_time.map(|time| time.to_string()),
            classification: match session.classification {
                ChargerClassification::Held => "Held",
                ChargerClassification::SharedCharger => "SharedCharger",
                ChargerClassification::OtherCharger => "OtherCharger",
            },
            classification_reason: format!("{:?}", session.classification_reason),
            coverage: session
                .observed_window
                .as_ref()
                .map(|window| CounterCoverageResult {
                    connection_start_utc_seconds: session.start_time.map(|time| time.to_string()),
                    connection_end_utc_seconds: session.end_time.map(|time| time.to_string()),
                    first_counter_utc_seconds: window.first_time.map(|time| time.to_string()),
                    last_counter_utc_seconds: window.last_time.map(|time| time.to_string()),
                    usable_readings: window.usable_readings,
                    partial_start: session
                        .quality_flags
                        .contains(&QualityFlag::MissingBaseline),
                    partial_end: session
                        .quality_flags
                        .contains(&QualityFlag::MissingTerminalSample),
                    quantization: LIVE_QUANTIZATION,
                }),
            evidence_state: if session.observed_window.is_some()
                && session.is_eligible()
                && !session.quality_flags.is_empty()
            {
                "partial"
            } else if !session.quality_flags.is_empty() {
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

impl Store {
    pub fn results_for_charger(
        &self,
        owner: &OwnerId,
        vehicle: &VehicleId,
        charger: &str,
    ) -> Result<ScopedResults, IngestionError> {
        self.validate_live_storage()?;
        let ledger = self.replay(owner, vehicle)?;
        results_for_view(
            &ledger,
            &self.manifest,
            self.source_epoch(),
            owner,
            vehicle,
            Some(self),
            Some(charger),
        )
    }

    fn session_location_provenance(
        &self,
        vehicle: &str,
        connection: &str,
    ) -> Result<Vec<SessionLocationVersion>, IngestionError> {
        let registry = self
            .registry
            .as_ref()
            .ok_or(IngestionError::RegistryIntegrity)?;
        let mut versions = std::collections::BTreeSet::new();
        let events = self
            .connection
            .prepare("SELECT id,event_json FROM event_variants WHERE vehicle=?1 AND connection=?2")
            .map_err(crate::schema::storage_error)?
            .query_map(rusqlite::params![vehicle, connection], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(crate::schema::storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(crate::schema::storage_error)?;
        for (id, json) in events {
            let event: crate::NormalizedEvent =
                serde_json::from_str(&json).map_err(|_| IngestionError::RegistryIntegrity)?;
            match event.location {
                Some(crate::NormalizedLocation::MatchedCharger {
                    charger_id,
                    assignment_version,
                    geofence_version,
                }) => {
                    versions.insert(SessionLocationVersion {
                        charger_id,
                        assignment_version,
                        geofence_version,
                    });
                }
                Some(crate::NormalizedLocation::OutsideChargers {
                    assignment_version,
                    geofence_versions,
                })
                | Some(crate::NormalizedLocation::AmbiguousChargers {
                    assignment_version,
                    geofence_versions,
                }) => {
                    for geofence_version in geofence_versions {
                        let (charger_id, _) = registry
                            .geofence_versions
                            .get(&geofence_version)
                            .ok_or(IngestionError::RegistryIntegrity)?;
                        versions.insert(SessionLocationVersion {
                            charger_id: charger_id.clone(),
                            assignment_version: assignment_version.clone(),
                            geofence_version,
                        });
                    }
                }
                Some(
                    crate::NormalizedLocation::InsideGeofence
                    | crate::NormalizedLocation::OutsideGeofence,
                ) => {
                    let charger_id = registry
                        .legacy_charger
                        .as_ref()
                        .ok_or(IngestionError::RegistryIntegrity)?
                        .clone();
                    let assignment_version = format!(
                        "assignment-{:016x}",
                        crate::live::fingerprint(
                            crate::registry::json(&(vehicle, vec![charger_id.clone()]))?.as_bytes()
                        )
                    );
                    if !registry.assignments.contains_key(&assignment_version) {
                        return Err(IngestionError::RegistryIntegrity);
                    }
                    let legacy: String = self
                        .connection
                        .query_row(
                            "SELECT manifest_json FROM legacy_registry_contract",
                            [],
                            |row| row.get(0),
                        )
                        .map_err(crate::schema::storage_error)?;
                    let legacy = crate::Manifest::from_json(legacy.as_bytes())
                        .map_err(|_| IngestionError::RegistryIntegrity)?;
                    let provenance = self
                        .connection
                        .prepare(
                            "SELECT DISTINCT provenance_json FROM deliveries WHERE variant_id=?1",
                        )
                        .map_err(crate::schema::storage_error)?
                        .query_map([id], |row| row.get::<_, String>(0))
                        .map_err(crate::schema::storage_error)?
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map_err(crate::schema::storage_error)?;
                    let mut geofences = std::collections::BTreeSet::new();
                    for json in provenance {
                        let provenance: crate::EvidenceProvenance = serde_json::from_str(&json)
                            .map_err(|_| IngestionError::RegistryIntegrity)?;
                        if provenance.mapping_version.starts_with("live-config-") {
                            geofences.insert(provenance.mapping_version);
                        }
                    }
                    if geofences.is_empty() {
                        geofences.insert(legacy.config().geofence_version.clone());
                    }
                    for geofence_version in geofences {
                        versions.insert(SessionLocationVersion {
                            charger_id: charger_id.clone(),
                            assignment_version: assignment_version.clone(),
                            geofence_version,
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(versions.into_iter().collect())
    }

    fn session_location_versions(
        &self,
        vehicle: &str,
        connection: &str,
        charger_id: &str,
    ) -> Result<(Vec<String>, Vec<String>), IngestionError> {
        let versions = self.session_location_provenance(vehicle, connection)?;
        let assignments = versions
            .iter()
            .filter(|version| version.charger_id == charger_id)
            .map(|version| version.assignment_version.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let geofences = versions
            .into_iter()
            .filter(|version| version.charger_id == charger_id)
            .map(|version| version.geofence_version)
            .collect::<std::collections::BTreeSet<_>>();
        Ok((
            assignments.into_iter().collect(),
            geofences.into_iter().collect(),
        ))
    }
}

fn kwh(energy: Energy) -> String {
    energy.to_string().trim_end_matches(" kWh").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Manifest, normalize};
    use chargeshare_core::Ledger;

    fn replay_fixture(manifest: &Manifest, zero: bool) -> Ledger {
        let mut ledger = Ledger::default();
        for mapping in &manifest.config().mappings {
            ledger
                .register(
                    VehicleId::new(&mapping.vehicle).unwrap(),
                    OwnerId::new(&mapping.owner).unwrap(),
                )
                .unwrap();
        }
        for charger in &manifest.config().owner_chargers {
            ledger.configure_charger(
                OwnerId::new(&charger.owner_id).unwrap(),
                charger.geofence.to_core().unwrap(),
            );
        }
        let fixture: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../dev/fixtures/complete.json")).unwrap();
        for record in fixture["expected"].as_array().unwrap() {
            let mut payload = record["payload"].clone();
            if zero {
                for field in payload["data"].as_array_mut().unwrap() {
                    if field["key"] == "ACChargingEnergyIn" {
                        field["value"]["stringValue"] = serde_json::json!("0");
                    }
                }
            }
            let evidence = normalize(
                manifest,
                Some(record["key"].as_str().unwrap().as_bytes()),
                &serde_json::to_vec(&payload).unwrap(),
            )
            .unwrap();
            ledger.ingest(evidence.event.to_core().unwrap()).unwrap();
        }
        ledger
    }

    fn response(ledger: &Ledger, manifest: &Manifest, suffix: &str) -> ScopedResults {
        results_from_replay(
            ledger,
            manifest,
            "synthetic-api-epoch",
            &OwnerId::new(&format!("owner-{suffix}")).unwrap(),
            &VehicleId::new(&format!("vehicle-{suffix}")).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn scoped_response_serializes_ev2_a_prices_and_time_provenance_from_normalized_replay() {
        let manifest =
            Manifest::from_json(include_bytes!("../../../dev/ingestion-manifest.json")).unwrap();
        let ledger = replay_fixture(&manifest, false);
        for (suffix, exact, display, energy) in [
            ("a", "0.1127900000000000 USD", "$0.11", "0.500000"),
            ("b", "0.0563950000000000 USD", "$0.06", "0.250000"),
        ] {
            let result = response(&ledger, &manifest, suffix);
            assert_eq!(result.pricing.exact_subtotal.as_deref(), Some(exact));
            assert_eq!(result.pricing.display_subtotal.as_deref(), Some(display));
            assert_eq!(result.pricing.priced_ac_kwh, energy);
            let quote = result.sessions[0].pricing.quote.as_ref().unwrap();
            assert_eq!(quote.exact_cost, exact);
            assert_eq!(quote.slices.len(), 2);
            assert_eq!(quote.slices[0].start_utc_seconds, "1788264000");
            assert_eq!(quote.slices[0].local_date, "2026-09-01");
            assert_eq!(quote.slices[0].period, "off_peak");
            assert_eq!(quote.slices[0].usd_per_kwh, "0.2255800000 USD/kWh");
            assert_eq!(result, response(&ledger, &manifest, suffix));
            let serialized = serde_json::to_string(&result).unwrap();
            for forbidden in [
                "latitude",
                "longitude",
                "12.3456789",
                "45.6789123",
                "device-1",
            ] {
                assert!(!serialized.contains(forbidden));
            }
        }
        assert!(
            results_from_replay(
                &ledger,
                &manifest,
                "synthetic-api-epoch",
                &OwnerId::new("owner-b").unwrap(),
                &VehicleId::new("vehicle-a").unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn response_distinguishes_zero_quotes_and_rate_holds_while_revision_tracks_selected_rates() {
        let manifest =
            Manifest::from_json(include_bytes!("../../../dev/ingestion-manifest.json")).unwrap();
        let ledger = replay_fixture(&manifest, false);
        let before = response(&ledger, &manifest, "a");
        let mut future = manifest.config().clone();
        future.rate_models[0].id = "pge-future-v1".to_owned();
        future.rate_models[0].effective_from = "2026-09-02".to_owned();
        for charger in &mut future.owner_chargers {
            charger.rate_model_versions = vec!["pge-future-v1".to_owned()];
        }
        let future = Manifest::new(future).unwrap();
        let held = response(&ledger, &future, "a");
        assert_eq!(held.sessions[0].pricing.hold_reason, Some("MissingRate"));
        assert_eq!(held.sessions[0].pricing.quote, None);
        assert_eq!(held.pricing.display_subtotal, None);
        assert_eq!(held.pricing.exact_subtotal, None);
        assert_eq!(held.observed_ac_kwh, before.observed_ac_kwh);
        assert_eq!(
            held.eligible_shared_charger_kwh,
            before.eligible_shared_charger_kwh
        );
        assert_ne!(held.revision, before.revision);
        let zero = response(&replay_fixture(&future, true), &future, "a");
        let month = zero
            .monthly
            .months
            .iter()
            .find(|month| month.month == "2026-09")
            .unwrap();
        assert_eq!(month.evidence.complete_zero_sessions, 1);
        assert_eq!(month.days[0].total.as_ref().unwrap().display_cost, "$0.00");
        assert_eq!(month.days[0].evidence.complete_zero_sessions, 1);
        assert_eq!(zero.sessions[0].evidence_state, "complete_zero");
        assert_eq!(zero.pricing.display_subtotal.as_deref(), Some("$0.00"));
        assert_eq!(
            zero.pricing.exact_subtotal.as_deref(),
            Some("0.0000000000000000 USD")
        );
    }
    #[test]
    fn a_new_selected_rate_changes_only_quote_rate_provenance_and_revision() {
        let original =
            Manifest::from_json(include_bytes!("../../../dev/ingestion-manifest.json")).unwrap();
        let ledger = replay_fixture(&original, false);
        let before = response(&ledger, &original, "a");
        let mut revised = original.config().clone();
        revised.rate_models[0].id = "synthetic-repricing-v2".to_owned();
        revised.rate_models[0].source_label = "Public synthetic repricing test sample".to_owned();
        for period in &mut revised.rate_models[0].periods {
            period.usd_per_kwh = "0.3".to_owned();
        }
        for charger in &mut revised.owner_chargers {
            charger.rate_model_versions = vec!["synthetic-repricing-v2".to_owned()];
        }
        let revised = Manifest::new(revised).unwrap();
        let after = response(&ledger, &revised, "a");
        assert_eq!(
            after.pricing.exact_subtotal.as_deref(),
            Some("0.1500000000000000 USD")
        );
        assert_eq!(after.pricing.display_subtotal.as_deref(), Some("$0.15"));
        assert_eq!(before.provenance, after.provenance);
        assert_eq!(before.observed_ac_kwh, after.observed_ac_kwh);
        assert_eq!(
            before.eligible_shared_charger_kwh,
            after.eligible_shared_charger_kwh
        );
        assert_eq!(before.evidence_boundary, after.evidence_boundary);
        assert_ne!(before.revision, after.revision);
        let mut preserved = after.sessions.clone();
        preserved[0].pricing = before.sessions[0].pricing.clone();
        assert_eq!(preserved, before.sessions);
        assert_eq!(before, response(&ledger, &original, "a"));
    }
}
