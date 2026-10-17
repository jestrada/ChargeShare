use chargeshare_core::VehicleId;
use serde::{Deserialize, Serialize};

use crate::live::{ChargeState, parse_utc_timestamp};
use crate::schema::storage_error;
use crate::{BatteryPercent, IngestionError, NormalizedEvent, Store};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BatteryReading {
    pub battery_percent: BatteryPercent,
    pub observed_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChargingObservation {
    pub active: bool,
    pub observed_at: String,
}

#[derive(Default)]
pub(crate) struct VehicleStatus {
    pub battery_reading: Option<BatteryReading>,
    pub charging: Option<ChargingObservation>,
}

#[derive(Deserialize)]
struct RetainedSignal {
    created_at: String,
    #[serde(default)]
    battery_percent: Option<BatteryPercent>,
    state: Option<ChargeState>,
}

impl Store {
    pub(crate) fn vehicle_status(
        &self,
        vehicle: &VehicleId,
    ) -> Result<VehicleStatus, IngestionError> {
        let mut status = VehicleStatus::default();
        let mut battery_time = None;
        let mut charging_time = None;
        let query = if self.manifest.is_live() {
            "SELECT signal_json, NULL FROM live_records WHERE vehicle = ?1 ORDER BY signal_json"
        } else {
            "SELECT v.event_json, MIN(json_extract(d.provenance_json, '$.created_at'))
             FROM event_variants v JOIN deliveries d ON d.variant_id = v.id
             WHERE v.vehicle = ?1 GROUP BY v.id ORDER BY v.event_json"
        };
        let mut statement = self.connection.prepare(query).map_err(storage_error)?;
        let rows = statement
            .query_map([vehicle.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (json, source_time) = row.map_err(storage_error)?;
            let (observed_at, battery_percent, charging) = if self.manifest.is_live() {
                let signal: RetainedSignal =
                    serde_json::from_str(&json).map_err(|_| IngestionError::StorageCorrupt)?;
                (
                    signal.created_at,
                    signal.battery_percent,
                    signal
                        .state
                        .filter(|state| *state != ChargeState::Unknown)
                        .map(|state| state == ChargeState::Charging),
                )
            } else {
                let event: NormalizedEvent =
                    serde_json::from_str(&json).map_err(|_| IngestionError::StorageCorrupt)?;
                (
                    source_time.ok_or(IngestionError::StorageCorrupt)?,
                    event.battery_percent,
                    event.charging,
                )
            };
            let timestamp =
                parse_utc_timestamp(&observed_at).map_err(|_| IngestionError::StorageCorrupt)?;
            if let Some(battery_percent) = battery_percent
                && battery_time.is_none_or(|previous| timestamp > previous)
            {
                battery_time = Some(timestamp);
                status.battery_reading = Some(BatteryReading {
                    battery_percent,
                    observed_at: observed_at.clone(),
                });
            }
            if let Some(active) = charging
                && charging_time.is_none_or(|previous| timestamp > previous)
            {
                charging_time = Some(timestamp);
                status.charging = Some(ChargingObservation {
                    active,
                    observed_at,
                });
            }
        }
        Ok(status)
    }
}
