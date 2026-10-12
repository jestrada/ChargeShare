use std::collections::{BTreeMap, BTreeSet};

use chargeshare_core::{
    ChargerGeofence, LocationSample, OwnerId, RateModelDefinition, RateModelHistory,
    RateModelVersion, RatePeriod, Season, UsdRate,
};
use serde::{Deserialize, Serialize};

use crate::{ManifestConfig, NormalizedLocation, RejectionReason};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChargerGeofenceConfig {
    pub center: NormalizedLocation,
    pub radius_meters: u32,
}

impl ChargerGeofenceConfig {
    pub fn to_core(&self) -> Result<ChargerGeofence, RejectionReason> {
        let LocationSample::Valid(center) = self.center.to_core()? else {
            return Err(RejectionReason::InvalidManifest);
        };
        ChargerGeofence::new(center, self.radius_meters).ok_or(RejectionReason::InvalidManifest)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerChargerConfig {
    pub owner_id: String,
    pub charger_id: String,
    pub geofence: ChargerGeofenceConfig,
    pub rate_model_versions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RatePeriodConfig {
    pub season: String,
    pub period: String,
    pub start_second: u32,
    pub end_second: u32,
    pub usd_per_kwh: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RateModelConfig {
    pub id: String,
    pub plan: String,
    pub source_label: String,
    pub source_url: String,
    pub effective_from: String,
    pub effective_until: Option<String>,
    pub timezone: String,
    pub timezone_rules_version: String,
    pub season_rules_version: String,
    pub day_type_rules_version: String,
    pub holidays: Vec<String>,
    pub periods: Vec<RatePeriodConfig>,
}

impl RateModelConfig {
    pub fn to_core(&self) -> Result<RateModelVersion, RejectionReason> {
        let invalid = RejectionReason::InvalidManifest;
        let periods = self
            .periods
            .iter()
            .map(|period| {
                Ok(RatePeriod {
                    season: match period.season.as_str() {
                        "Summer" => Season::Summer,
                        "Winter" => Season::Winter,
                        _ => return Err(invalid),
                    },
                    period: period.period.clone(),
                    start_second: period.start_second,
                    end_second: period.end_second,
                    rate: UsdRate::parse_per_kwh(&period.usd_per_kwh).map_err(|_| invalid)?,
                })
            })
            .collect::<Result<_, _>>()?;
        RateModelVersion::new(RateModelDefinition {
            id: self.id.clone(),
            plan: self.plan.clone(),
            source_label: self.source_label.clone(),
            source_url: self.source_url.clone(),
            effective_from: self.effective_from.clone(),
            effective_until: self.effective_until.clone(),
            timezone: self.timezone.clone(),
            timezone_rules_version: self.timezone_rules_version.clone(),
            season_rules_version: self.season_rules_version.clone(),
            day_type_rules_version: self.day_type_rules_version.clone(),
            holidays: self.holidays.clone(),
            periods,
        })
        .map_err(|_| invalid)
    }
}

pub(crate) fn validate_rates(config: &mut ManifestConfig) -> Result<(), RejectionReason> {
    let invalid = RejectionReason::InvalidManifest;
    if config.rate_models.len() > 64
        || config.owner_chargers.len() > crate::MAX_MAPPINGS
        || (!config.owner_chargers.is_empty() && !config.geofences.is_empty())
    {
        return Err(invalid);
    }
    config
        .rate_models
        .sort_by(|left, right| left.id.cmp(&right.id));
    config
        .owner_chargers
        .sort_by(|left, right| left.owner_id.cmp(&right.owner_id));
    let mut versions = BTreeMap::new();
    for model in &mut config.rate_models {
        let core = model.to_core()?;
        model.holidays.sort();
        model.periods.sort_by(|left, right| {
            (&left.season, left.start_second).cmp(&(&right.season, right.start_second))
        });
        if versions.insert(model.id.clone(), core).is_some() {
            return Err(invalid);
        }
    }
    let owners: BTreeSet<_> = config
        .mappings
        .iter()
        .map(|mapping| mapping.owner.clone())
        .collect();
    let mut configured = BTreeSet::new();
    let mut chargers = BTreeMap::new();
    for charger in &mut config.owner_chargers {
        OwnerId::new(&charger.charger_id).map_err(|_| invalid)?;
        if !owners.contains(&charger.owner_id) || !configured.insert(charger.owner_id.clone()) {
            return Err(invalid);
        }
        charger.geofence.to_core()?;
        if charger.rate_model_versions.len() > 64 {
            return Err(invalid);
        }
        charger.rate_model_versions.sort();
        let history = charger
            .rate_model_versions
            .iter()
            .map(|id| versions.get(id).cloned().ok_or(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        RateModelHistory::new(history).map_err(|_| invalid)?;
        if let Some(previous) = chargers.insert(&charger.charger_id, &charger.rate_model_versions)
            && previous != &charger.rate_model_versions
        {
            return Err(invalid);
        }
    }
    if !config.owner_chargers.is_empty() && owners != configured {
        return Err(invalid);
    }
    Ok(())
}
