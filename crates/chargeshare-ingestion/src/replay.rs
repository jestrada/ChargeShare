use chargeshare_core::{Ledger, OwnerId, Session, Summary, VehicleId};

use crate::schema::storage_error;
use crate::{IngestionError, NormalizedEvent, Store};

impl Store {
    pub fn replay(&self, owner: &OwnerId, vehicle: &VehicleId) -> Result<Ledger, IngestionError> {
        let mapping = self
            .manifest
            .config()
            .mappings
            .iter()
            .find(|mapping| mapping.vehicle == vehicle.as_str() && mapping.owner == owner.as_str())
            .ok_or(IngestionError::DomainReplay)?;
        let mut ledger = Ledger::default();
        ledger
            .register(vehicle.clone(), owner.clone())
            .map_err(|_| IngestionError::DomainReplay)?;
        let mut statement = self
            .connection
            .prepare("SELECT event_json FROM event_variants WHERE vehicle = ?1 ORDER BY event_json")
            .map_err(storage_error)?;
        let mut rows = statement.query([&mapping.vehicle]).map_err(storage_error)?;
        while let Some(row) = rows.next().map_err(storage_error)? {
            let event_json: String = row.get(0).map_err(storage_error)?;
            let event: NormalizedEvent =
                serde_json::from_str(&event_json).map_err(|_| IngestionError::StorageCorrupt)?;
            if event.vehicle != mapping.vehicle {
                return Err(IngestionError::StorageCorrupt);
            }
            ledger
                .ingest(
                    event
                        .to_core()
                        .map_err(|_| IngestionError::StorageCorrupt)?,
                )
                .map_err(|_| IngestionError::DomainReplay)?;
        }
        Ok(ledger)
    }

    pub fn summary(&self, owner: &OwnerId, vehicle: &VehicleId) -> Result<Summary, IngestionError> {
        self.replay(owner, vehicle)?
            .summary(vehicle)
            .map_err(|_| IngestionError::DomainReplay)
    }

    pub fn sessions(
        &self,
        owner: &OwnerId,
        vehicle: &VehicleId,
    ) -> Result<Vec<Session>, IngestionError> {
        self.replay(owner, vehicle)?
            .sessions(vehicle)
            .map_err(|_| IngestionError::DomainReplay)
    }
}
