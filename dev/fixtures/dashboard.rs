use std::error::Error;
use std::fs;
use std::path::PathBuf;

use chargeshare_core::{OwnerId, VehicleId};
use chargeshare_ingestion::{
    DeliveryCoordinate, Manifest, ManifestConfig, PartitionStart, SYNTHETIC_TOPIC, SourceBinding,
    Store,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DashboardFixture {
    notice: String,
    as_of_utc_seconds: i64,
    manifest: ManifestConfig,
    records: Vec<FixtureRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureRecord {
    key: String,
    payload: Value,
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() != 2 {
        return Err("usage: dashboard-fixture <generated-json> <fresh-output-directory>".into());
    }
    let fixture: DashboardFixture = serde_json::from_slice(&fs::read(&arguments[0])?)?;
    if fixture.notice != "Synthetic data for demos and screenshots only; not real" {
        return Err("synthetic fixture notice required".into());
    }
    let output = PathBuf::from(&arguments[1]);
    fs::DirBuilder::new().create(&output)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&output, fs::Permissions::from_mode(0o700))?;
    }
    let manifest = Manifest::new(fixture.manifest)?;
    let source = SourceBinding {
        epoch: "synthetic-dashboard-v1".to_owned(),
        topic: SYNTHETIC_TOPIC.to_owned(),
        cluster_id: None,
        topic_id: None,
        partitions: vec![PartitionStart {
            partition: 0,
            initial_offset: 0,
        }],
    };
    let mut store = Store::open(&output.join("ingestion.sqlite3"), &manifest, &source)?;
    for (offset, record) in fixture.records.iter().enumerate() {
        let offset = i64::try_from(offset)?;
        store.ingest(
            &manifest,
            DeliveryCoordinate {
                partition: 0,
                offset,
            },
            offset,
            Some(record.key.as_bytes()),
            &serde_json::to_vec(&record.payload)?,
        )?;
    }
    let statistics = store.statistics()?;
    if statistics.rejected_deliveries != 0 {
        return Err("synthetic fixture ingestion rejected records".into());
    }
    let api = output.join("http/api/poc");
    fs::create_dir_all(&api)?;
    let scopes: Vec<_> = manifest
        .config()
        .mappings
        .iter()
        .map(|mapping| json!({"owner": mapping.owner, "vehicle": mapping.vehicle}))
        .collect();
    fs::write(
        api.join("scopes"),
        serde_json::to_vec(&json!({"synthetic_only": true, "scopes": scopes}))?,
    )?;
    for mapping in &manifest.config().mappings {
        let owner = OwnerId::new(&mapping.owner)?;
        let vehicle = VehicleId::new(&mapping.vehicle)?;
        let mut result = store.results(&owner, &vehicle)?;
        let ledger = store.replay(&owner, &vehicle)?;
        let prices = ledger
            .time_of_use_pricing(&vehicle, &manifest.rate_history(&owner)?)
            .map_err(|_| "synthetic fixture pricing failed")?;
        result.monthly = prices
            .monthly_history(fixture.as_of_utc_seconds)
            .map_err(|_| "synthetic fixture monthly history failed")?
            .into();
        result.revision.clear();
        let hash = serde_json::to_vec(&result)?
            .iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        result.revision = format!("results-v2-{hash:016x}");
        let scoped_path = api
            .join("owners")
            .join(owner.as_str())
            .join("vehicles")
            .join(vehicle.as_str());
        fs::create_dir_all(&scoped_path)?;
        fs::write(scoped_path.join("results"), serde_json::to_vec(&result)?)?;
    }
    println!(
        "Synthetic demo: {} records ingested into SQLite; Rust-priced dashboard snapshot exported. Receiver/Kafka transport not exercised.",
        statistics.accepted_deliveries
    );
    Ok(())
}
