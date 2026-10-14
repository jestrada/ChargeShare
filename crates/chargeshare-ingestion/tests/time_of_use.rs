use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use chargeshare_core::{OwnerId, VehicleId};
use chargeshare_ingestion::*;
use rusqlite::Connection;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct SyntheticRuntime(PathBuf);

impl SyntheticRuntime {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "chargeshare-synthetic-tou-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = fs::DirBuilder::new();
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700).create(&path).unwrap();
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("evidence.sqlite")
    }
}

impl Drop for SyntheticRuntime {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn manifest() -> Manifest {
    Manifest::from_json(include_bytes!("../../../dev/ingestion-manifest.json")).unwrap()
}

fn source() -> SourceBinding {
    SourceBinding {
        epoch: "synthetic-tou-epoch".to_owned(),
        topic: SYNTHETIC_TOPIC.to_owned(),
        cluster_id: None,
        topic_id: None,
        partitions: vec![PartitionStart {
            partition: 0,
            initial_offset: 0,
        }],
    }
}

fn ingest_complete(store: &mut Store, manifest: &Manifest) {
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../dev/fixtures/complete.json")).unwrap();
    for (offset, record) in fixture["expected"].as_array().unwrap().iter().enumerate() {
        let payload = serde_json::to_vec(&record["payload"]).unwrap();
        store
            .ingest(
                manifest,
                DeliveryCoordinate {
                    partition: 0,
                    offset: offset as i64,
                },
                offset as i64,
                Some(record["key"].as_str().unwrap().as_bytes()),
                &payload,
            )
            .unwrap();
    }
}

fn results(store: &Store, suffix: &str) -> ScopedResults {
    store
        .results(
            &OwnerId::new(&format!("owner-{suffix}")).unwrap(),
            &VehicleId::new(&format!("vehicle-{suffix}")).unwrap(),
        )
        .unwrap()
}

fn retained_evidence(runtime: &SyntheticRuntime) -> Vec<(String, String)> {
    let connection = Connection::open(runtime.database()).unwrap();
    connection.prepare("SELECT event_json, provenance_json FROM event_variants JOIN deliveries ON deliveries.variant_id = event_variants.id ORDER BY event_json").unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn selected_shared_charger_rates_price_isolated_owner_scopes_and_restart_replay_is_stable() {
    let runtime = SyntheticRuntime::new();
    let manifest = manifest();
    let mut store = Store::open(&runtime.database(), &manifest, &source()).unwrap();
    let empty = results(&store, "a");
    assert_eq!(empty.pricing.state, "empty");
    assert_eq!(empty.pricing.display_subtotal, None);
    ingest_complete(&mut store, &manifest);
    let a = results(&store, "a");
    let b = results(&store, "b");
    assert_eq!(
        a.pricing.exact_subtotal.as_deref(),
        Some("0.1127900000000000 USD")
    );
    assert_eq!(a.pricing.display_subtotal.as_deref(), Some("$0.11"));
    assert_eq!(
        b.pricing.exact_subtotal.as_deref(),
        Some("0.0563950000000000 USD")
    );
    assert_eq!(b.pricing.display_subtotal.as_deref(), Some("$0.06"));
    assert_eq!(a.pricing.charger_id, b.pricing.charger_id);
    assert_eq!(a.pricing.rate_models, b.pricing.rate_models);
    for result in [&a, &b] {
        let month = result
            .monthly
            .months
            .iter()
            .find(|month| month.month == "2026-09")
            .unwrap();
        let total = month.total.as_ref().unwrap();
        assert_eq!(total.ac_kwh, result.pricing.priced_ac_kwh);
        assert_eq!(
            Some(&total.exact_cost),
            result.pricing.exact_subtotal.as_ref()
        );
        assert_eq!(
            Some(&total.display_cost),
            result.pricing.display_subtotal.as_ref()
        );
        assert_eq!(month.days.len(), 30);
        assert_eq!(month.days[0].total, month.total);
        assert!(month.days[1..].iter().all(|day| day.total.is_none()));
        assert_eq!(month.evidence.quoted_sessions, 1);
        assert_eq!(result.monthly.evidence.held_sessions, 0);
        assert_eq!(result.monthly.undated_sessions, 0);
        assert_eq!(
            result.sessions[0].start_utc_seconds.as_deref(),
            Some("1788264000")
        );
        assert_eq!(
            result.sessions[0].end_utc_seconds.as_deref(),
            Some("1788264600")
        );
    }
    assert_eq!(a.observed_ac_kwh, "0.500000");
    assert_eq!(b.observed_ac_kwh, "0.250000");
    assert_eq!(a.eligible_shared_charger_kwh, a.observed_ac_kwh);
    assert_eq!(b.eligible_shared_charger_kwh, b.observed_ac_kwh);
    assert_eq!(
        a.sessions[0].pricing.quote.as_ref().unwrap().slices.len(),
        2
    );
    assert!(
        store
            .results(
                &OwnerId::new("owner-a").unwrap(),
                &VehicleId::new("vehicle-b").unwrap()
            )
            .is_err()
    );
    let evidence = retained_evidence(&runtime);
    drop(store);
    let mut store = Store::open(&runtime.database(), &manifest, &source()).unwrap();
    assert_eq!(results(&store, "a"), a);
    assert_eq!(results(&store, "b"), b);
    ingest_complete(&mut store, &manifest);
    assert_eq!(results(&store, "a"), a);
    assert_eq!(retained_evidence(&runtime), evidence);
    let serialized = serde_json::to_string(&a).unwrap();
    for forbidden in [
        "latitude",
        "longitude",
        "locationValue",
        "12.3456789",
        "45.6789123",
        "device-1",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

#[test]
fn immutable_new_versions_reprice_preserved_evidence_and_reused_ids_are_refused() {
    let runtime = SyntheticRuntime::new();
    let original = manifest();
    let mut store = Store::open(&runtime.database(), &original, &source()).unwrap();
    ingest_complete(&mut store, &original);
    let a = results(&store, "a");
    let evidence = retained_evidence(&runtime);
    let stats = store.statistics().unwrap();
    let positions = store.positions().unwrap();
    drop(store);
    let mut revised = original.config().clone();
    revised.rate_models[0].id = "pge-ev2-a-v2".to_owned();
    for period in &mut revised.rate_models[0].periods {
        period.usd_per_kwh = "0.3".to_owned();
    }
    for charger in &mut revised.owner_chargers {
        charger.rate_model_versions = vec!["pge-ev2-a-v2".to_owned()];
    }
    let revised = Manifest::new(revised).unwrap();
    let store = Store::open(&runtime.database(), &revised, &source()).unwrap();
    let after = results(&store, "a");
    assert_eq!(
        after.pricing.exact_subtotal.as_deref(),
        Some("0.1500000000000000 USD")
    );
    assert_ne!(a.revision, after.revision);
    assert_eq!(a.observed_ac_kwh, after.observed_ac_kwh);
    assert_eq!(
        a.eligible_shared_charger_kwh,
        after.eligible_shared_charger_kwh
    );
    let mut preserved = after.sessions.clone();
    preserved[0].pricing = a.sessions[0].pricing.clone();
    assert_eq!(preserved, a.sessions);
    assert_eq!(store.statistics().unwrap(), stats);
    assert_eq!(store.positions().unwrap(), positions);
    drop(store);
    let mut reused = revised.config().clone();
    reused.rate_models[0].periods[0].usd_per_kwh = "0.31".to_owned();
    let reused = Manifest::new(reused).unwrap();
    assert!(matches!(
        Store::open(&runtime.database(), &reused, &source()),
        Err(IngestionError::ConfigurationMismatch)
    ));
    assert_eq!(retained_evidence(&runtime), evidence);
    let restored = Store::open(&runtime.database(), &original, &source()).unwrap();
    assert_eq!(results(&restored, "a"), a);
}

#[test]
fn missing_selected_coverage_holds_the_whole_session_without_changing_eligibility() {
    let runtime = SyntheticRuntime::new();
    let original = manifest();
    let mut store = Store::open(&runtime.database(), &original, &source()).unwrap();
    ingest_complete(&mut store, &original);
    let before = results(&store, "a");
    drop(store);
    let mut revised = original.config().clone();
    revised.rate_models[0].id = "pge-future-rate-v1".to_owned();
    revised.rate_models[0].effective_from = "2026-09-02".to_owned();
    for charger in &mut revised.owner_chargers {
        charger.rate_model_versions = vec!["pge-future-rate-v1".to_owned()];
    }
    let revised = Manifest::new(revised).unwrap();
    let store = Store::open(&runtime.database(), &revised, &source()).unwrap();
    let held = results(&store, "a");
    assert_eq!(held.state, "eligible");
    assert_eq!(held.pricing.state, "held");
    assert_eq!(held.pricing.rate_held_session_count, 1);
    assert_eq!(held.sessions[0].pricing.hold_reason, Some("MissingRate"));
    assert_eq!(held.sessions[0].pricing.quote, None);
    assert_eq!(held.pricing.exact_subtotal, None);
    assert_eq!(held.pricing.display_subtotal, None);
    assert_eq!(held.monthly.evidence.held_sessions, 1);
    let month = held
        .monthly
        .months
        .iter()
        .find(|month| month.month == "2026-09")
        .unwrap();
    assert_eq!(month.evidence.held_sessions, 1);
    assert_eq!(month.days[0].evidence.held_sessions, 1);
    assert!(month.total.is_none());
    assert!(month.days.iter().all(|day| day.total.is_none()));
    assert!(
        month
            .periods
            .iter()
            .all(|period| period.total.ac_kwh == "0.000000")
    );
    assert_eq!(held.observed_ac_kwh, before.observed_ac_kwh);
    assert_eq!(
        held.eligible_shared_charger_kwh,
        before.eligible_shared_charger_kwh
    );
    drop(store);
    let store = Store::open(&runtime.database(), &revised, &source()).unwrap();
    assert_eq!(results(&store, "a"), held);
}

#[test]
fn automatic_exclusions_and_quality_cannot_receive_prices() {
    for exclusion in ["outside", "missing", "dc", "ambiguous", "quality"] {
        let runtime = SyntheticRuntime::new();
        let mut config = manifest().config().clone();
        if exclusion == "outside" {
            config.owner_chargers[0].geofence.center =
                NormalizedLocation::from_coordinates(0.0, 0.0);
        }
        if exclusion == "dc" || exclusion == "ambiguous" {
            for annotation in &mut config.annotations {
                if annotation.device == "device-1" {
                    annotation.charge_type = if exclusion == "dc" {
                        NormalizedChargeType::Dc
                    } else {
                        NormalizedChargeType::Ambiguous
                    };
                }
            }
        }
        let manifest = Manifest::new(config).unwrap();
        let mut store = Store::open(&runtime.database(), &manifest, &source()).unwrap();
        let fixture: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../dev/fixtures/complete.json")).unwrap();
        for (offset, record) in fixture["expected"].as_array().unwrap().iter().enumerate() {
            let mut payload = record["payload"].clone();
            if record["key"] == "device-1" {
                if exclusion == "missing" {
                    payload["data"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|field| field["key"] != "Location");
                }
                if exclusion == "quality" {
                    for field in payload["data"].as_array_mut().unwrap() {
                        if field["key"] == "ACChargingEnergyIn" {
                            field["value"]["stringValue"] = serde_json::json!("invalid");
                        }
                    }
                }
            }
            store
                .ingest(
                    &manifest,
                    DeliveryCoordinate {
                        partition: 0,
                        offset: offset as i64,
                    },
                    offset as i64,
                    Some(record["key"].as_str().unwrap().as_bytes()),
                    &serde_json::to_vec(&payload).unwrap(),
                )
                .unwrap();
        }
        let result = results(&store, "a");
        assert_eq!(result.sessions[0].pricing.state, "ineligible");
        assert_eq!(result.sessions[0].pricing.quote, None);
        assert_eq!(result.pricing.exact_subtotal, None);
    }
}

#[test]
fn invalid_rates_unknown_selection_duplicate_versions_and_overlap_are_rejected_at_startup() {
    let original = manifest().config().clone();
    let mut changed = original.clone();
    changed.rate_models[0].periods[0].usd_per_kwh = "NaN".to_owned();
    assert!(Manifest::new(changed).is_err());
    let mut changed = original.clone();
    changed.owner_chargers[0].rate_model_versions = vec!["neighbor-only".to_owned()];
    assert!(Manifest::new(changed).is_err());
    let mut changed = original.clone();
    changed.rate_models.push(changed.rate_models[0].clone());
    assert!(Manifest::new(changed).is_err());
    let mut changed = original.clone();
    changed.rate_models[0].timezone_rules_version = "unknown".to_owned();
    assert!(Manifest::new(changed).is_err());
    let mut changed = original.clone();
    changed.rate_models[0].periods[0].end_second = 55000;
    assert!(Manifest::new(changed).is_err());
    let mut changed = original.clone();
    let mut second = changed.rate_models[0].clone();
    second.id = "overlapping-v2".to_owned();
    changed.rate_models.push(second);
    for charger in &mut changed.owner_chargers {
        charger
            .rate_model_versions
            .push("overlapping-v2".to_owned());
    }
    assert!(Manifest::new(changed).is_err());
    let mut changed = original;
    changed.owner_chargers.pop();
    assert!(Manifest::new(changed).is_err());
}
