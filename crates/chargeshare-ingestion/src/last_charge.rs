use chargeshare_core::{Session, VehicleId};
use serde::Serialize;

use crate::schema::storage_error;
use crate::{BatteryPercent, IngestionError, NormalizedEvent, NormalizedEventKind, Store};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LastCharge {
    pub battery_percent: Option<BatteryPercent>,
    pub end_utc_seconds: String,
}

impl Store {
    pub(crate) fn last_charge(
        &self,
        vehicle: &VehicleId,
        sessions: &[Session],
    ) -> Result<Option<LastCharge>, IngestionError> {
        let Some(end_time) = sessions.iter().filter_map(|session| session.end_time).max() else {
            return Ok(None);
        };
        let mut batteries = std::collections::BTreeSet::new();
        let mut statement = self
            .connection
            .prepare(
                "SELECT event_json FROM event_variants WHERE vehicle = ?1 AND source_time = ?2",
            )
            .map_err(storage_error)?;
        let events = statement
            .query_map(rusqlite::params![vehicle.as_str(), end_time], |row| {
                row.get::<_, String>(0)
            })
            .map_err(storage_error)?;
        for row in events {
            let json = row.map_err(storage_error)?;
            let event: NormalizedEvent =
                serde_json::from_str(&json).map_err(|_| IngestionError::StorageCorrupt)?;
            if event.kind != NormalizedEventKind::End
                || !sessions.iter().any(|session| {
                    session.end_time == Some(end_time)
                        && session.id.connection.as_str() == event.connection
                })
            {
                continue;
            }
            let mut battery = event.battery_percent;
            if self.manifest.is_live() {
                let provenance: String = self.connection.query_row(
                    "SELECT json_extract(disposition_json, '$.Accepted.provenance') FROM live_records WHERE vehicle = ?1 AND json_extract(disposition_json, '$.Accepted.event.position') = ?2 LIMIT 1",
                    rusqlite::params![vehicle.as_str(), event.position], |row| row.get(0),
                ).map_err(storage_error)?;
                let provenance: crate::EvidenceProvenance = serde_json::from_str(&provenance)
                    .map_err(|_| IngestionError::StorageCorrupt)?;
                if !self.terminal_soc_agrees(vehicle.as_str(), &provenance.created_at, battery)? {
                    battery = None;
                }
            }
            batteries.insert(battery);
        }
        Ok(Some(LastCharge {
            battery_percent: if batteries.len() == 1 {
                batteries.into_iter().next().flatten()
            } else {
                None
            },
            end_utc_seconds: end_time.to_string(),
        }))
    }
}
