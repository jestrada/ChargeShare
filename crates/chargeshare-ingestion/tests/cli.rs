#![cfg(target_os = "linux")]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use chargeshare_ingestion::{PartitionStart, SYNTHETIC_TOPIC, SourceBinding};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    checkout: PathBuf,
    runtime: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let crate_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        Self::in_checkout(crate_directory.parent().unwrap().parent().unwrap())
    }

    fn in_checkout(checkout_root: &Path) -> Self {
        let fixture_parent = checkout_root.join("target");
        match fs::create_dir(&fixture_parent) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => panic!("fixture parent could not be created"),
        }
        assert!(fs::symlink_metadata(&fixture_parent).unwrap().is_dir());
        let checkout = fixture_parent.join(format!(
            "cli-fixture-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&checkout).unwrap();
        let checkout = checkout.canonicalize().unwrap();
        let runtime = checkout.join(".local-runtime");
        fs::DirBuilder::new().mode(0o700).create(&runtime).unwrap();
        private_file(
            &runtime.join("synthetic-runtime.json"),
            br#"{"project_name":"chargeshare-local-0123456789ab","synthetic_only":true}"#,
        );
        private_file(
            &runtime.join("manifest.json"),
            include_bytes!("fixtures/manifest-v1.json"),
        );
        let source = SourceBinding {
            epoch: "synthetic-cli-fixture-v1".to_owned(),
            topic: SYNTHETIC_TOPIC.to_owned(),
            cluster_id: None,
            topic_id: None,
            partitions: vec![PartitionStart {
                partition: 0,
                initial_offset: 0,
            }],
        };
        private_file(
            &runtime.join("source-epoch.json"),
            &serde_json::to_vec(&source).unwrap(),
        );
        Self { checkout, runtime }
    }

    fn command(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_chargeshare-ingestion"))
            .current_dir(&self.checkout)
            .args(arguments)
            .output()
            .unwrap()
    }

    fn consume(&self, records: &str, seconds: &str) -> Output {
        self.command(&[
            "consume",
            self.runtime.to_str().unwrap(),
            "127.0.0.1:1",
            records,
            seconds,
        ])
    }

    fn prepare_demo_checkout(&self) {
        let crate_directory = self.checkout.join("crates/chargeshare-ingestion");
        fs::create_dir_all(&crate_directory).unwrap();
        private_file(
            &crate_directory.join("Cargo.toml"),
            b"synthetic fixture checkout",
        );
        private_file(&self.checkout.join(".gitignore"), b"target/\n");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.checkout).unwrap();
    }
}

fn private_file(path: &Path, contents: &[u8]) {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(contents).unwrap();
    file.sync_all().unwrap();
}

fn assert_safe_failure(output: &Output, reason: &str) {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), reason);
}

#[test]
fn fixture_setup_creates_ignored_parent_without_cargo_build_outputs() {
    let outer = Fixture::new();
    let fresh_checkout = outer.checkout.join("fresh-checkout");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&fresh_checkout)
        .unwrap();
    private_file(&fresh_checkout.join(".gitignore"), b"target/\n");
    assert!(!fresh_checkout.join("target").exists());
    let fresh = Fixture::in_checkout(&fresh_checkout);
    assert!(fresh_checkout.join("target").is_dir());
    assert!(fresh.runtime.is_dir());
    assert_safe_failure(
        &fresh.command(&[
            "replay",
            fresh.runtime.to_str().unwrap(),
            "owner-a",
            "vehicle-a",
        ]),
        "storage: unavailable or bounded write failed",
    );
    assert!(!fresh.runtime.join("ingestion.sqlite3").exists());
}

#[test]
fn fixture_setup_refuses_symlinked_ignored_parent() {
    let outer = Fixture::new();
    let fresh_checkout = outer.checkout.join("fresh-checkout");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&fresh_checkout)
        .unwrap();
    symlink(&outer.runtime, fresh_checkout.join("target")).unwrap();
    assert!(std::panic::catch_unwind(|| Fixture::in_checkout(&fresh_checkout)).is_err());
    assert_eq!(fs::read_dir(&outer.runtime).unwrap().count(), 3);
}

#[test]
fn missing_epoch_marker_blocks_without_fabricating_source_or_database() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.runtime.join("source-epoch.json")).unwrap();
    let output = fixture.consume("1", "1");
    assert_safe_failure(&output, "source: broker runtime identity mismatch");
    assert!(!fixture.runtime.join("source-epoch.json").exists());
    assert!(!fixture.runtime.join("ingestion.sqlite3").exists());
}

#[test]
fn invalid_record_deadline_and_external_broker_limits_fail_without_storage() {
    let fixture = Fixture::new();
    for (records, seconds) in [
        ("0", "1"),
        ("10001", "1"),
        ("1", "0"),
        ("1", "121"),
        ("-1", "1"),
    ] {
        let output = fixture.consume(records, seconds);
        assert_eq!(output.status.code(), Some(2));
        assert!(!fixture.runtime.join("ingestion.sqlite3").exists());
    }
    let output = fixture.command(&[
        "consume",
        fixture.runtime.to_str().unwrap(),
        "example.invalid:9092",
        "1",
        "1",
    ]);
    assert_safe_failure(&output, "configuration: invalid synthetic contract");
    assert!(!fixture.runtime.join("ingestion.sqlite3").exists());
}

#[test]
fn bounded_unavailable_broker_uses_fixed_errors_and_preserves_initialized_database() {
    let fixture = Fixture::new();
    let start = Instant::now();
    let output = fixture.consume("1", "1");
    assert_eq!(output.status.code(), Some(2));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(output.stdout.is_empty());
    let reason = String::from_utf8_lossy(&output.stderr);
    assert!(matches!(
        reason.trim(),
        "ingestion: bounded deadline exceeded" | "source: broker operation failed"
    ));
    assert!(!reason.contains("127.0.0.1"));
    assert!(!reason.contains("device-"));
    assert!(fixture.runtime.join("ingestion.sqlite3").is_file());
}

#[test]
fn replay_rejects_unknown_or_mismatched_scope_before_creating_state() {
    let fixture = Fixture::new();
    for (owner, vehicle) in [
        ("owner-a", "vehicle-b"),
        ("unknown-owner", "vehicle-a"),
        ("owner-a", "unknown-vehicle"),
    ] {
        let output =
            fixture.command(&["replay", fixture.runtime.to_str().unwrap(), owner, vehicle]);
        assert_safe_failure(
            &output,
            "replay: scoped evidence could not be reconstructed",
        );
        assert!(!fixture.runtime.join("ingestion.sqlite3").exists());
        assert!(!fixture.runtime.join("ingestion.sqlite3.lock").exists());
    }
}

#[test]
fn runtime_and_marker_privacy_symlinks_and_hardlinks_are_rejected() {
    let fixture = Fixture::new();
    fs::set_permissions(&fixture.runtime, fs::Permissions::from_mode(0o755)).unwrap();
    assert_safe_failure(
        &fixture.consume("1", "1"),
        "configuration: invalid synthetic contract",
    );
    fs::set_permissions(&fixture.runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let marker = fixture.runtime.join("source-epoch.json");
    let alternate = fixture.runtime.join("alternate.json");
    fs::rename(&marker, &alternate).unwrap();
    symlink(&alternate, &marker).unwrap();
    assert_safe_failure(
        &fixture.consume("1", "1"),
        "source: broker runtime identity mismatch",
    );
    fs::remove_file(&marker).unwrap();
    fs::hard_link(&alternate, &marker).unwrap();
    assert_safe_failure(
        &fixture.consume("1", "1"),
        "source: broker runtime identity mismatch",
    );
    assert!(!fixture.runtime.join("ingestion.sqlite3").exists());
}

#[test]
fn arbitrary_runtime_path_and_unrecognized_ownership_cannot_read_candidate_data() {
    let fixture = Fixture::new();
    let output = fixture.command(&[
        "consume",
        fixture.checkout.to_str().unwrap(),
        "127.0.0.1:1",
        "1",
        "1",
    ]);
    assert_safe_failure(&output, "configuration: invalid synthetic contract");
    fs::write(
        fixture.runtime.join("synthetic-runtime.json"),
        br#"{"project_name":"another-project","synthetic_only":true}"#,
    )
    .unwrap();
    assert_safe_failure(
        &fixture.consume("1", "1"),
        "configuration: invalid synthetic contract",
    );
}

#[test]
fn candidate_demo_is_labeled_storage_only_and_idempotent_after_restart() {
    let fixture = Fixture::new();
    fixture.prepare_demo_checkout();
    let first = fixture.command(&["candidate-demo"]);
    assert!(first.status.success());
    let second = fixture.command(&["candidate-demo"]);
    assert!(second.status.success());
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());
    let summary = String::from_utf8_lossy(&first.stdout);
    assert!(summary.contains("normalization/storage fixture; receiver acceptance unverified"));
    assert!(summary.contains("observed_ac=0.500000 kWh"));
    assert!(summary.contains("observed_ac=0.250000 kWh"));
    assert!(summary.contains("eligible_shared_charger=0.000000 kWh"));
    assert!(!summary.contains("device-"));
    assert!(!summary.contains("vin"));
    assert!(!fixture.runtime.join("ingestion.sqlite3").exists());
}

#[test]
fn candidate_demo_refuses_differing_existing_fixture_without_overwrite() {
    let fixture = Fixture::new();
    fixture.prepare_demo_checkout();
    assert!(fixture.command(&["candidate-demo"]).status.success());
    let marker = fixture
        .checkout
        .join("target/durable-ingestion-fixture/source-epoch.json");
    let differing = b"synthetic different fixture marker";
    fs::write(&marker, differing).unwrap();
    assert_safe_failure(
        &fixture.command(&["candidate-demo"]),
        "storage: frozen configuration mismatch",
    );
    assert_eq!(fs::read(marker).unwrap(), differing);
}
