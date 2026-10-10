mod broker;
mod fixtures;
mod transport;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

const RECEIVER_REVISION: &str = "bd076fe1494841707528449560c4a19d0d426da4";
const BOUNDARY: &str = "synthetic receiver-to-Kafka only; adapter, SQLite and receiver-backed dashboard remain unimplemented";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Deserialize)]
struct RuntimeIdentity {
    project_name: String,
    runtime_dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct RetainedRun {
    scenario: String,
    starting_offsets: std::collections::BTreeMap<u32, u64>,
    expected: Vec<fixtures::ExpectedRecord>,
}

fn main() {
    if let Err(error) = execute() {
        eprintln!("telemetry-harness failed: {error}");
        std::process::exit(1);
    }
}

fn execute() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mode = arguments.first().map(String::as_str).unwrap_or("smoke");
    broker::bounded_command(
        "python3",
        &["scripts/dev/runtime.py", "validate-live-config"],
        60,
        "synthetic local runtime validation",
    )?;
    let identity_output = broker::bounded_command(
        "python3",
        &["scripts/dev/runtime.py", "identity"],
        10,
        "runtime identity",
    )?;
    let runtime: RuntimeIdentity = serde_json::from_slice(&identity_output)?;
    if !runtime.runtime_dir.is_dir() {
        return Err("runtime: start the prepared synthetic environment with tilt up before running the harness".into());
    }
    let lock = exclusive_run(&runtime)?;
    let broker = broker::Broker::new(&runtime.project_name);
    match mode {
        "ready" => {
            broker.ready()?;
            transport::status(&runtime.runtime_dir)?;
            println!("ready: Kafka protocol metadata and receiver authenticated status");
        }
        "replay" => {
            let run: RetainedRun = serde_json::from_slice(&fs::read(runtime.runtime_dir.join("last-run.json"))?)?;
            let observed = broker.observe(&run.starting_offsets, run.expected.len())?;
            fixtures::verify(&run.expected, &observed)?;
            println!("retained replay passed: {}; local broker retention, no application crash-recovery claim", run.scenario);
        }
        "invalid-auth" => {
            let starting_offsets = broker.offsets()?;
            transport::reject_untrusted(&runtime.runtime_dir)?;
            if broker.offsets()? != starting_offsets {
                return Err("authentication: rejected client produced unexpected telemetry".into());
            }
            println!("invalid-auth passed: unrelated synthetic CA rejected; no telemetry record");
        }
        "smoke" | "all" | "complete" | "missing" | "adverse" => {
            let scenario = fixtures::load(mode)?;
            let starting_offsets = broker.offsets()?;
            if !arguments.iter().any(|argument| argument == "--no-send") {
                for frame in &scenario.frames {
                    transport::exchange(&runtime.runtime_dir, frame)?;
                }
            }
            let mut expected = scenario.expected;
            if arguments.iter().any(|argument| argument == "--inject-missing-output") {
                expected.push(fixtures::missing_expectation());
            }
            let current_offsets = broker.offsets()?;
            if current_offsets.iter().any(|(partition, end)| end.saturating_sub(starting_offsets.get(partition).copied().unwrap_or(*end)) > expected.len() as u64) {
                return Err("record matching: unexpected extra synthetic output after run-start offsets".into());
            }
            let observed = broker.observe(&starting_offsets, expected.len())?;
            fixtures::verify(&expected, &observed)?;
            let run = RetainedRun { scenario: mode.to_owned(), starting_offsets, expected };
            write_private(runtime.runtime_dir.join("last-run.json"), serde_json::to_vec_pretty(&run)?)?;
            write_private(runtime.runtime_dir.join("normalized.json"), serde_json::to_vec_pretty(&fixtures::normalized(&observed))?)?;
            println!("{mode} passed: {} expected ACKs and {} matching decoded records; receiver {RECEIVER_REVISION}; {BOUNDARY}", scenario.frames.len(), observed.len());
        }
        _ => return Err("usage: chargeshare-harness ready|smoke|all|complete|missing|adverse|invalid-auth|replay [--no-send|--inject-missing-output]".into()),
    }
    FileExt::unlock(&lock)?;
    Ok(())
}

fn exclusive_run(runtime: &RuntimeIdentity) -> Result<File> {
    let lock = OpenOptions::new()
        .create(true)
        .mode(0o600)
        .truncate(false)
        .read(true)
        .write(true)
        .open(
            runtime
                .runtime_dir
                .with_file_name(".local-runtime-run.lock"),
        )?;
    lock.try_lock_exclusive()
        .map_err(|_| "run isolation: another harness is active; wait for it to finish")?;
    Ok(lock)
}

fn write_private(path: PathBuf, content: Vec<u8>) -> Result<()> {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(&content)?;
    Ok(())
}
