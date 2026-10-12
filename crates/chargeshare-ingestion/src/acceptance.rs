use std::time::{Duration, Instant};

use chargeshare_core::{OwnerId, VehicleId};
use chargeshare_ingestion::{
    AcceptedEvidence, CommitBoundary, IngestionError, KafkaConfig, Store, consume_bounded_observed,
    normalize,
};
use serde_json::json;

use crate::{owned_runtime, read_private_file, runtime_contract, write_unchanged_fixture};

pub fn status(path: &str) -> Result<(), IngestionError> {
    let runtime = owned_runtime(path)?;
    let (manifest, source) = runtime_contract(&runtime)?;
    let database = runtime.join("ingestion.sqlite3");
    if !database.is_file() {
        return Err(IngestionError::StorageUnavailable);
    }
    let store = Store::open(&database, &manifest, &source)?;
    let mut scopes = Vec::new();
    for (owner, vehicle) in [("owner-a", "vehicle-a"), ("owner-b", "vehicle-b")] {
        let owner = OwnerId::new(owner).map_err(|_| IngestionError::InvalidConfiguration)?;
        let vehicle = VehicleId::new(vehicle).map_err(|_| IngestionError::InvalidConfiguration)?;
        let summary = store.summary(&owner, &vehicle)?;
        let sessions = store.sessions(&owner, &vehicle)?;
        scopes.push(json!({
            "vehicle": vehicle.as_str(), "observed_ac": summary.observed_ac.to_string(),
            "eligible": summary.eligible_shared_charger.to_string(), "sessions": sessions.len(),
            "classifications": sessions.iter().map(|session| format!("{:?}", session.classification)).collect::<Vec<_>>(),
            "classification_reasons": sessions.iter().map(|session| format!("{:?}", session.classification_reason)).collect::<Vec<_>>(),
            "quality_flags": sessions.iter().map(|session| session.quality_flags.len()).sum::<usize>(),
        }));
    }
    let statistics = store.statistics()?;
    let positions: Vec<_> = store.positions()?.iter().map(|position| json!({"partition": position.partition, "next_offset": position.next_offset})).collect();
    println!(
        "{}",
        json!({"scopes": scopes, "positions": positions, "accepted": statistics.accepted_deliveries,
        "rejected": statistics.rejected_deliveries, "variants": statistics.event_variants})
    );
    Ok(())
}

pub fn verify_decoded(path: &str) -> Result<(), IngestionError> {
    let runtime = owned_runtime(path)?;
    let (manifest, _) = runtime_contract(&runtime)?;
    let capture: serde_json::Value = serde_json::from_slice(&read_private_file(
        &runtime.join("decoded-capture.json"),
        524_288,
    )?)
    .map_err(|_| IngestionError::InvalidConfiguration)?;
    if capture["receiver_revision"] != "bd076fe1494841707528449560c4a19d0d426da4"
        || capture["actual_receiver_transport_verified"] != true
        || capture["topic"] != "chargeshare_synthetic_V"
    {
        return Err(IngestionError::InvalidConfiguration);
    }
    let candidate: serde_json::Value =
        serde_json::from_slice(include_bytes!("../tests/fixtures/candidate-v.json"))
            .map_err(|_| IngestionError::InvalidConfiguration)?;
    let observed = capture["evidence"]
        .as_array()
        .ok_or(IngestionError::InvalidConfiguration)?;
    let expected = candidate["records"]
        .as_array()
        .ok_or(IngestionError::InvalidConfiguration)?;
    let normalize_records = |records: &[serde_json::Value]| -> Result<Vec<String>, IngestionError> {
        let mut normalized = Vec::new();
        for record in records {
            let key = record["key"]
                .as_str()
                .ok_or(IngestionError::InvalidConfiguration)?;
            let payload = serde_json::to_vec(&record["payload"])
                .map_err(|_| IngestionError::InvalidConfiguration)?;
            let evidence = normalize(&manifest, Some(key.as_bytes()), &payload)
                .map_err(|_| IngestionError::InvalidConfiguration)?;
            normalized.push(
                serde_json::to_string(&evidence)
                    .map_err(|_| IngestionError::InvalidConfiguration)?,
            );
        }
        normalized.sort();
        Ok(normalized)
    };
    let mut observed_evidence = Vec::new();
    for value in observed {
        let evidence: AcceptedEvidence = serde_json::from_value(value.clone())
            .map_err(|_| IngestionError::InvalidConfiguration)?;
        manifest
            .validate_evidence(&evidence)
            .map_err(|_| IngestionError::InvalidConfiguration)?;
        observed_evidence.push(
            serde_json::to_string(&evidence).map_err(|_| IngestionError::InvalidConfiguration)?,
        );
    }
    observed_evidence.sort();
    if observed_evidence != normalize_records(expected)? {
        return Err(IngestionError::InvalidConfiguration);
    }
    println!(
        "decoded contract: records=6 allowlist=tesla-v-bd076fe-v1 source_time=utc-seconds-v1 associations=fictional-manifest-v1"
    );
    Ok(())
}

pub fn consume_probe(
    path: &str,
    bootstrap: &str,
    boundary: &str,
    start: Instant,
) -> Result<(), IngestionError> {
    let boundary = match boundary {
        "before" => CommitBoundary::BeforeCommit,
        "after" => CommitBoundary::AfterCommit,
        _ => return Err(IngestionError::InvalidCommand),
    };
    let runtime = owned_runtime(path)?;
    let (manifest, source) = runtime_contract(&runtime)?;
    let mut config = KafkaConfig {
        bootstrap_servers: bootstrap.to_owned(),
        source_epoch_path: runtime.join("source-epoch.json"),
        max_records: 1,
        deadline: Duration::from_secs(30),
    };
    config.validate()?;
    let mut store = Store::open(&runtime.join("ingestion.sqlite3"), &manifest, &source)?;
    config.deadline = config
        .deadline
        .checked_sub(start.elapsed())
        .ok_or(IngestionError::Timeout)?;
    consume_bounded_observed(&config, &source, &manifest, &mut store, &mut |observed| {
        if observed == boundary {
            let pending = runtime.join("commit-boundary.pending.json");
            write_unchanged_fixture(
                &pending,
                match boundary {
                    CommitBoundary::BeforeCommit => b"\"before\"",
                    CommitBoundary::AfterCommit => b"\"after\"",
                },
            )?;
            std::fs::rename(&pending, runtime.join("commit-boundary.json"))
                .map_err(|_| IngestionError::StorageUnavailable)?;
            loop {
                std::thread::park_timeout(Duration::from_millis(100));
            }
        }
        Ok(())
    })?;
    Err(IngestionError::InvalidCommand)
}
