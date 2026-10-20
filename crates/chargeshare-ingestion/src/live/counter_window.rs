use chargeshare_core::{ChargeType, ConnectionId, CounterLocation, CounterObservation, VehicleId};
use rusqlite::params;

use super::{AdapterState, ChargeState, OdometerReading, Signal, parse_utc_timestamp};
use crate::schema::storage_error;
use crate::{IngestionError, NormalizedLocation, RecordDisposition, Store};

impl Store {
    pub(crate) fn counter_observations(
        &self,
        vehicle: &VehicleId,
    ) -> Result<Vec<CounterObservation>, IngestionError> {
        let mut statement = self
            .connection
            .prepare("SELECT DISTINCT signal_json, disposition_json FROM live_records WHERE vehicle = ?1 ORDER BY signal_json, disposition_json")
            .map_err(storage_error)?;
        let mut rows = statement
            .query(params![vehicle.as_str()])
            .map_err(storage_error)?;
        let mut signals = Vec::new();
        while let Some(row) = rows.next().map_err(storage_error)? {
            let signal: Signal =
                serde_json::from_str(&row.get::<_, String>(0).map_err(storage_error)?)
                    .map_err(|_| IngestionError::StorageCorrupt)?;
            let disposition: RecordDisposition =
                serde_json::from_str(&row.get::<_, String>(1).map_err(storage_error)?)
                    .map_err(|_| IngestionError::StorageCorrupt)?;
            let timestamp = parse_utc_timestamp(&signal.created_at)
                .map_err(|_| IngestionError::StorageCorrupt)?;
            if signal.vehicle != vehicle.as_str() || timestamp.0 != signal.time {
                return Err(IngestionError::StorageCorrupt);
            }
            signals.push((timestamp, signal, disposition));
        }
        signals.sort_by_key(|(timestamp, _, _)| *timestamp);
        signals.dedup_by(|left, right| left.1 == right.1 && left.2 == right.2);
        let mut location_state = AdapterState::default();
        let mut retained_location = CounterLocation::Missing;
        let mut previous_state = None;
        let mut observations = Vec::new();
        for ((_, nanoseconds), signal, disposition) in signals {
            let starts_connection = signal.state == Some(ChargeState::Starting)
                && previous_state != Some(ChargeState::Starting);
            previous_state = signal.state.or(previous_state);
            let movement = match &signal.odometer {
                Some(OdometerReading::Valid(current)) => location_state
                    .odometer
                    .as_ref()
                    .is_some_and(|prior| prior != current),
                Some(OdometerReading::Invalid) => true,
                None => false,
            };
            location_state.observe_location(&signal, nanoseconds);
            let event = match disposition {
                RecordDisposition::Accepted(evidence) => Some(evidence.event),
                RecordDisposition::Rejected(_) => None,
            };
            if signal.location.is_some() || movement {
                retained_location = match &signal.location {
                    Some(NormalizedLocation::InsideGeofence) => {
                        CounterLocation::Charger("shared-charger".to_owned())
                    }
                    Some(NormalizedLocation::MatchedCharger { charger_id, .. }) => {
                        CounterLocation::Charger(charger_id.clone())
                    }
                    _ => CounterLocation::Contradictory,
                };
            }
            if location_state
                .location_conflict_time
                .is_some_and(|conflict| (signal.time, nanoseconds) <= conflict)
            {
                retained_location = CounterLocation::Contradictory;
            }
            observations.push(CounterObservation {
                time: signal.time,
                nanoseconds,
                connection: event
                    .as_ref()
                    .map(|event| ConnectionId::new(&event.connection))
                    .transpose()
                    .map_err(|_| IngestionError::StorageCorrupt)?,
                kind: event.as_ref().map(|event| event.kind.into()),
                counter: signal
                    .counter
                    .as_ref()
                    .map(crate::NormalizedCounter::to_core)
                    .transpose()
                    .map_err(|_| IngestionError::StorageCorrupt)?,
                charge_type: if signal.fast_charger == Some(true) {
                    ChargeType::Dc
                } else if previous_state == Some(ChargeState::Unknown) {
                    ChargeType::Ambiguous
                } else {
                    event
                        .as_ref()
                        .map_or(ChargeType::Unknown, |event| event.charge_type.into())
                },
                location: retained_location.clone(),
                breaks_connection: matches!(signal.state, Some(ChargeState::Disconnected)),
                starts_connection,
                movement,
            });
        }
        Ok(observations)
    }
}
