use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::registry::json;
use crate::schema::storage_error;
use crate::{
    CommitBoundary, DeliveryCoordinate, IngestionError, PartitionRange, RecordDisposition,
    SourceBinding, Store,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryPosition {
    pub(crate) partition: i32,
    pub(crate) next_offset: i64,
    pub(crate) high: i64,
}

impl Store {
    pub(crate) fn pending_recoveries(&self) -> Result<Vec<String>, IngestionError> {
        if self.registry.is_none() {
            return Ok(Vec::new());
        }
        self.connection
            .prepare("SELECT vehicle FROM recovery_jobs WHERE state != 'complete' ORDER BY vehicle")
            .map_err(storage_error)?
            .query_map([], |row| row.get(0))
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)
    }

    pub(crate) fn capture_recovery(
        &mut self,
        vehicle: &str,
        source: &SourceBinding,
        bounds: &[PartitionRange],
    ) -> Result<Vec<RecoveryPosition>, IngestionError> {
        let (retained_source, state): (String, String) = self
            .connection
            .query_row(
                "SELECT source_json,state FROM recovery_jobs WHERE vehicle = ?1",
                [vehicle],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(storage_error)?;
        let retained: SourceBinding = serde_json::from_str(&retained_source)
            .map_err(|_| IngestionError::RegistryIntegrity)?;
        retained.verify_identity(source)?;
        if retained != *source {
            return Err(IngestionError::SourceMismatch);
        }
        if state == "pending" {
            if bounds.len() != source.partitions.len() {
                return Err(IngestionError::SourceMismatch);
            }
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(storage_error)?;
            for partition in &source.partitions {
                let matches: Vec<_> = bounds
                    .iter()
                    .filter(|bound| bound.partition == partition.partition)
                    .collect();
                let [bound] = matches.as_slice() else {
                    return Err(IngestionError::SourceMismatch);
                };
                if bound.low < 0 || bound.high < bound.low {
                    return Err(IngestionError::RecoveryGap);
                }
                transaction
                    .execute(
                        "INSERT INTO recovery_progress VALUES (?1,?2,?3,?4,?3)",
                        params![vehicle, bound.partition, bound.low, bound.high],
                    )
                    .map_err(storage_error)?;
            }
            transaction
                .execute(
                    "UPDATE recovery_jobs SET state = 'scanning' WHERE vehicle = ?1",
                    [vehicle],
                )
                .map_err(storage_error)?;
            transaction.commit().map_err(storage_error)?;
        }
        let positions = self.recovery_positions(vehicle)?;
        for position in &positions {
            let bound = bounds
                .iter()
                .find(|bound| bound.partition == position.partition)
                .ok_or(IngestionError::RecoveryGap)?;
            if bound.low > position.next_offset || bound.high < position.high {
                return Err(IngestionError::RecoveryGap);
            }
        }
        Ok(positions)
    }

    pub(crate) fn recovery_positions(
        &self,
        vehicle: &str,
    ) -> Result<Vec<RecoveryPosition>, IngestionError> {
        self.connection.prepare("SELECT partition,next_offset,high FROM recovery_progress WHERE vehicle = ?1 ORDER BY partition").map_err(storage_error)?
            .query_map([vehicle], |row| Ok(RecoveryPosition { partition: row.get(0)?, next_offset: row.get(1)?, high: row.get(2)? })).map_err(storage_error)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage_error)
    }

    pub(crate) fn recover_live_observed(
        &mut self,
        vehicle: &str,
        coordinate: DeliveryCoordinate,
        headers: &[(String, Vec<u8>)],
        payload: &[u8],
        observer: &mut dyn FnMut(CommitBoundary) -> Result<(), IngestionError>,
    ) -> Result<(), IngestionError> {
        let registry = self
            .registry
            .as_ref()
            .ok_or(IngestionError::RegistryIntegrity)?;
        let car = registry
            .cars
            .iter()
            .find(|car| car.registration.vehicle == vehicle)
            .ok_or(IngestionError::RegistryIntegrity)?;
        let target = headers.iter().any(|(name, value)| {
            name == "vin" && value.as_slice() == car.registration.device.as_bytes()
        });
        let (disposition, checkpoint) = if target {
            let (disposition, checkpoint) = self.prepare_live(registry, headers, payload)?;
            (Some(disposition), checkpoint)
        } else {
            (None, None)
        };
        let disposition_json = json(&disposition)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let (next, high): (i64, i64) = transaction.query_row("SELECT next_offset,high FROM recovery_progress WHERE vehicle = ?1 AND partition = ?2", params![vehicle,coordinate.partition], |row| Ok((row.get(0)?, row.get(1)?))).map_err(storage_error)?;
        if coordinate.offset != next || next >= high {
            return Err(IngestionError::RecoveryGap);
        }
        let mut variant_id = None;
        if let Some(RecordDisposition::Accepted(evidence)) = &disposition {
            self.manifest
                .validate_evidence(evidence)
                .map_err(|_| IngestionError::RegistryIntegrity)?;
            if evidence.event.vehicle != vehicle {
                return Err(IngestionError::RegistryIntegrity);
            }
            let event_json = json(&evidence.event)?;
            transaction.execute("INSERT INTO event_variants(vehicle,position,connection,source_time,event_json) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(event_json) DO NOTHING", params![vehicle,evidence.event.position,evidence.event.connection,evidence.event.time,event_json]).map_err(storage_error)?;
            variant_id = Some(
                transaction
                    .query_row::<i64, _, _>(
                        "SELECT id FROM event_variants WHERE event_json = ?1",
                        [&event_json],
                        |row| row.get(0),
                    )
                    .map_err(storage_error)?,
            );
        }
        transaction
            .execute(
                "INSERT INTO recovery_evidence VALUES (?1,?2,?3,?4,?5)",
                params![
                    vehicle,
                    coordinate.partition,
                    coordinate.offset,
                    variant_id,
                    disposition_json
                ],
            )
            .map_err(storage_error)?;
        if let Some(checkpoint) = checkpoint {
            checkpoint.save(&transaction)?;
        }
        transaction.execute("UPDATE recovery_progress SET next_offset = next_offset + 1 WHERE vehicle = ?1 AND partition = ?2", params![vehicle,coordinate.partition]).map_err(storage_error)?;
        observer(CommitBoundary::BeforeCommit)?;
        transaction.commit().map_err(storage_error)?;
        observer(CommitBoundary::AfterCommit)?;
        Ok(())
    }

    pub(crate) fn complete_recovery(
        &mut self,
        vehicle: &str,
        observer: &mut dyn FnMut(CommitBoundary) -> Result<(), IngestionError>,
    ) -> Result<(), IngestionError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let incomplete = transaction.query_row("SELECT EXISTS(SELECT 1 FROM recovery_progress WHERE vehicle = ?1 AND next_offset != high)", [vehicle], |row| row.get::<_,bool>(0)).map_err(storage_error)?;
        let scanning: Option<bool> = transaction
            .query_row(
                "SELECT state = 'scanning' FROM recovery_jobs WHERE vehicle = ?1",
                [vehicle],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if incomplete || scanning != Some(true) {
            return Err(IngestionError::RecoveryPending);
        }
        transaction
            .execute(
                "UPDATE recovery_jobs SET state = 'complete' WHERE vehicle = ?1",
                [vehicle],
            )
            .map_err(storage_error)?;
        observer(CommitBoundary::BeforeCommit)?;
        transaction.commit().map_err(storage_error)?;
        observer(CommitBoundary::AfterCommit)?;
        Ok(())
    }
}

impl Store {
    pub(crate) fn verify_recovery_state(&self) -> Result<(), IngestionError> {
        let mut statement = self
            .connection
            .prepare("SELECT vehicle,source_json,state FROM recovery_jobs")
            .map_err(storage_error)?;
        let jobs = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(storage_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage_error)?;
        for (vehicle, source_json, state) in jobs {
            let source: SourceBinding = serde_json::from_str(&source_json)
                .map_err(|_| IngestionError::RegistryIntegrity)?;
            let fixed: String = self
                .connection
                .query_row("SELECT source_json FROM runtime_metadata", [], |row| {
                    row.get(0)
                })
                .map_err(storage_error)?;
            if fixed != json(&source)? || source_json != fixed {
                return Err(IngestionError::SourceMismatch);
            }
            let positions = self.recovery_positions(&vehicle)?;
            if state == "pending" && !positions.is_empty()
                || state != "pending" && positions.len() != source.partitions.len()
            {
                return Err(IngestionError::RegistryIntegrity);
            }
            for position in positions {
                if state == "complete" && position.next_offset != position.high {
                    return Err(IngestionError::RegistryIntegrity);
                }
                let (low,count,min,max):(i64,i64,Option<i64>,Option<i64>)=self.connection.query_row("SELECT p.low,count(r.offset),min(r.offset),max(r.offset) FROM recovery_progress p LEFT JOIN recovery_evidence r ON r.vehicle=p.vehicle AND r.partition=p.partition WHERE p.vehicle=?1 AND p.partition=?2 GROUP BY p.low",params![vehicle,position.partition],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).map_err(storage_error)?;
                if count != position.next_offset - low
                    || count > 0 && (min != Some(low) || max != Some(position.next_offset - 1))
                {
                    return Err(IngestionError::RegistryIntegrity);
                }
            }
        }
        let mut statement=self.connection.prepare("SELECT r.vehicle,r.disposition_json,v.vehicle,v.position,v.connection,v.source_time,v.event_json FROM recovery_evidence r LEFT JOIN event_variants v ON v.id=r.variant_id").map_err(storage_error)?;
        let mut rows = statement.query([]).map_err(storage_error)?;
        while let Some(row) = rows.next().map_err(storage_error)? {
            let vehicle: String = row.get(0).map_err(storage_error)?;
            let disposition_json: String = row.get(1).map_err(storage_error)?;
            let disposition: Option<RecordDisposition> = serde_json::from_str(&disposition_json)
                .map_err(|_| IngestionError::RegistryIntegrity)?;
            if json(&disposition)? != disposition_json {
                return Err(IngestionError::RegistryIntegrity);
            }
            match disposition {
                Some(RecordDisposition::Accepted(evidence)) => {
                    self.manifest
                        .validate_evidence(&evidence)
                        .map_err(|_| IngestionError::RegistryIntegrity)?;
                    if evidence.event.vehicle != vehicle
                        || row.get::<_, String>(2).map_err(storage_error)? != vehicle
                        || row.get::<_, String>(3).map_err(storage_error)?
                            != evidence.event.position
                        || row.get::<_, String>(4).map_err(storage_error)?
                            != evidence.event.connection
                        || row.get::<_, i64>(5).map_err(storage_error)? != evidence.event.time
                        || row.get::<_, String>(6).map_err(storage_error)? != json(&evidence.event)?
                    {
                        return Err(IngestionError::RegistryIntegrity);
                    }
                    if let Some(location) = &evidence.event.location {
                        self.registry
                            .as_ref()
                            .ok_or(IngestionError::RegistryIntegrity)?
                            .validate_location(&vehicle, location)?;
                    }
                }
                _ => {
                    if row
                        .get::<_, Option<String>>(6)
                        .map_err(storage_error)?
                        .is_some()
                    {
                        return Err(IngestionError::RegistryIntegrity);
                    }
                }
            }
        }
        Ok(())
    }
}
