use std::fmt;

use crate::energy::Energy;
use crate::error::LedgerError;
use crate::identity::{OwnerId, VehicleId};
use crate::money::{ExactUsd, MoneyError, UsdCents};
use crate::session::{EVIDENCE_LABEL, ReconstructedSession, Session};
use crate::tariff::{RateSchedule, RateWindow};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PricingError {
    Ledger(LedgerError),
    Money(MoneyError),
    EnergyOverflow,
}

impl fmt::Display for PricingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PricingError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PricingHold {
    IneligibleSession,
    MissingRate { start_time: i64, end_time: i64 },
    UnresolvedRateBoundary { start_time: i64, end_time: i64 },
    NonIncreasingTime { start_time: i64, end_time: i64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CostLine {
    pub start_time: i64,
    pub end_time: i64,
    pub energy: Energy,
    pub rate_window: RateWindow,
    pub exact_cost: ExactUsd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionQuote {
    pub lines: Vec<CostLine>,
    pub exact_cost: ExactUsd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionPricing {
    pub session: Session,
    pub quote: Result<SessionQuote, PricingHold>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PricingSummary {
    pub vehicle: VehicleId,
    pub owner: OwnerId,
    pub sessions: Vec<SessionPricing>,
    pub priced_energy: Energy,
    pub exact_priced_subtotal: ExactUsd,
    pub rounded_priced_subtotal: UsdCents,
    pub evidence_label: &'static str,
}

impl PricingSummary {
    pub fn has_unpriced_sessions(&self) -> bool {
        self.sessions.iter().any(|session| session.quote.is_err())
    }
}

fn quote_session(
    reconstructed: &ReconstructedSession,
    rates: &RateSchedule,
) -> Result<Result<SessionQuote, PricingHold>, MoneyError> {
    if !reconstructed.session.is_eligible() {
        return Ok(Err(PricingHold::IneligibleSession));
    }
    let mut quote = SessionQuote {
        lines: Vec::new(),
        exact_cost: ExactUsd::ZERO,
    };
    for segment in &reconstructed.segments {
        let start_time = segment.start_time;
        let end_time = segment.end_time;
        if start_time >= end_time {
            return Ok(Err(PricingHold::NonIncreasingTime {
                start_time,
                end_time,
            }));
        }
        let Some(window) = rates.at(start_time) else {
            return Ok(Err(PricingHold::MissingRate {
                start_time,
                end_time,
            }));
        };
        if end_time > window.end_time() {
            return Ok(Err(PricingHold::UnresolvedRateBoundary {
                start_time,
                end_time,
            }));
        }
        let cost = window.rate().cost_for(segment.energy);
        quote.exact_cost = quote.exact_cost.checked_add(cost)?;
        quote.lines.push(CostLine {
            start_time,
            end_time,
            energy: segment.energy,
            rate_window: window.clone(),
            exact_cost: cost,
        });
    }
    Ok(Ok(quote))
}

pub(crate) fn summarize_pricing(
    vehicle: VehicleId,
    owner: OwnerId,
    reconstructed: Vec<ReconstructedSession>,
    rates: &RateSchedule,
) -> Result<PricingSummary, PricingError> {
    let mut summary = PricingSummary {
        vehicle,
        owner,
        sessions: Vec::new(),
        priced_energy: Energy::ZERO,
        exact_priced_subtotal: ExactUsd::ZERO,
        rounded_priced_subtotal: UsdCents::default(),
        evidence_label: EVIDENCE_LABEL,
    };
    for reconstructed in reconstructed {
        let quote = quote_session(&reconstructed, rates).map_err(PricingError::Money)?;
        if let Ok(quote) = &quote {
            summary.exact_priced_subtotal = summary
                .exact_priced_subtotal
                .checked_add(quote.exact_cost)
                .map_err(PricingError::Money)?;
            summary.priced_energy = summary
                .priced_energy
                .checked_add(reconstructed.session.observed_ac)
                .map_err(|_| PricingError::EnergyOverflow)?;
        }
        summary.sessions.push(SessionPricing {
            session: reconstructed.session,
            quote,
        });
    }
    summary.rounded_priced_subtotal = summary.exact_priced_subtotal.round_half_up_to_cents();
    Ok(summary)
}
