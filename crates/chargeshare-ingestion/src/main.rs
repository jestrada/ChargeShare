mod acceptance;

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chargeshare_core::{OwnerId, VehicleId};
use chargeshare_ingestion::{
    DeliveryCoordinate, IngestionError, KafkaConfig, MAX_MANIFEST_BYTES, Manifest, PartitionStart,
    RereadRange, SYNTHETIC_TOPIC, SourceBinding, Store, consume_bounded, read_source_marker,
    reread_bounded,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeOwnership {
    project_name: String,
    synthetic_only: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), IngestionError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("car") => car_command(&args[1..]),
        Some("registry") => registry_command(&args[1..]),
        Some("rebuild-live") if args.len() == 3 => {
            let original = private_runtime_settings(&args[1])?;
            let replacement = private_runtime_settings(&args[2])?;
            chargeshare_ingestion::rebuild_live(&original, &replacement)?;
            println!("Live rebuild completed in the separate dataset; original preserved.");
            Ok(())
        }
        Some("live") if args.len() == 2 => {
            chargeshare_ingestion::run_live(&private_runtime_settings(&args[1])?)
        }
        Some("serve") if args.len() == 4 => {
            let runtime = owned_runtime(&args[1])?;
            let (manifest, source) = runtime_contract(&runtime)?;
            let address = args[3]
                .parse::<std::net::SocketAddr>()
                .map_err(|_| IngestionError::InvalidCommand)?;
            let private_container = args[1] == "/run/.local-runtime"
                && args[2] == "kafka:9092"
                && address == "0.0.0.0:8788".parse().unwrap();
            if !(address.ip() == std::net::Ipv4Addr::LOCALHOST && address.port() != 0)
                && !private_container
            {
                return Err(IngestionError::InvalidCommand);
            }
            let config = KafkaConfig {
                bootstrap_servers: args[2].clone(),
                source_epoch_path: runtime.join("source-epoch.json"),
                max_records: 1,
                deadline: Duration::from_secs(2),
            };
            config.validate()?;
            let store = Store::open(&runtime.join("ingestion.sqlite3"), &manifest, &source)?;
            let listener = std::net::TcpListener::bind(address)
                .map_err(|_| IngestionError::StorageUnavailable)?;
            chargeshare_ingestion::serve_results(
                listener,
                store,
                config,
                source,
                &AtomicBool::new(false),
            )
        }
        Some("consume") if args.len() == 5 => {
            let max_records = args[3]
                .parse::<usize>()
                .map_err(|_| IngestionError::InvalidCommand)?;
            let seconds = args[4]
                .parse::<u64>()
                .map_err(|_| IngestionError::InvalidCommand)?;
            if !(1..=120).contains(&seconds) {
                return Err(IngestionError::InvalidCommand);
            }
            with_deadline(Duration::from_secs(seconds), |start| {
                consume_runtime(&args[1], &args[2], max_records, seconds, start)
            })
        }
        Some("replay") if args.len() == 4 => with_deadline(Duration::from_secs(10), |_| {
            replay_runtime(&args[1], &args[2], &args[3])
        }),
        Some("reread") if args.len() == 7 => {
            let partition = args[3]
                .parse::<i32>()
                .map_err(|_| IngestionError::InvalidCommand)?;
            let start_offset = args[4]
                .parse::<i64>()
                .map_err(|_| IngestionError::InvalidCommand)?;
            let end_offset = args[5]
                .parse::<i64>()
                .map_err(|_| IngestionError::InvalidCommand)?;
            let seconds = args[6]
                .parse::<u64>()
                .map_err(|_| IngestionError::InvalidCommand)?;
            if !(1..=120).contains(&seconds) || start_offset < 0 || end_offset <= start_offset {
                return Err(IngestionError::InvalidCommand);
            }
            with_deadline(Duration::from_secs(seconds), |start| {
                reread_runtime(
                    &args[1],
                    &args[2],
                    RereadRange {
                        partition,
                        start_offset,
                        end_offset,
                    },
                    seconds,
                    start,
                )
            })
        }
        Some("acceptance-status") if args.len() == 2 => {
            with_deadline(Duration::from_secs(10), |_| acceptance::status(&args[1]))
        }
        Some("verify-decoded") if args.len() == 2 => with_deadline(Duration::from_secs(10), |_| {
            acceptance::verify_decoded(&args[1])
        }),
        Some("consume-probe") if args.len() == 4 => {
            with_deadline(Duration::from_secs(30), |start| {
                acceptance::consume_probe(&args[1], &args[2], &args[3], start)
            })
        }
        Some("candidate-demo") if args.len() == 1 => {
            with_deadline(Duration::from_secs(10), |_| candidate_demo())
        }
        _ => Err(IngestionError::InvalidCommand),
    }
}

fn private_runtime_settings(input: &str) -> Result<Vec<u8>, IngestionError> {
    let path = Path::new(input);
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or(IngestionError::InvalidConfiguration)?;
    if !path.is_absolute() || path.starts_with(checkout) {
        return Err(IngestionError::InvalidConfiguration);
    }
    reject_symlink_ancestors(path)?;
    read_private_file(path, MAX_MANIFEST_BYTES)
}

fn reread_runtime(
    path: &str,
    bootstrap_servers: &str,
    range: RereadRange,
    seconds: u64,
    start: Instant,
) -> Result<(), IngestionError> {
    let runtime = owned_runtime(path)?;
    let (manifest, source) = runtime_contract(&runtime)?;
    let mut config = KafkaConfig {
        bootstrap_servers: bootstrap_servers.to_owned(),
        source_epoch_path: runtime.join("source-epoch.json"),
        max_records: usize::try_from(range.end_offset - range.start_offset)
            .map_err(|_| IngestionError::InvalidCommand)?,
        deadline: Duration::from_secs(seconds),
    };
    config.validate()?;
    if !runtime.join("ingestion.sqlite3").is_file() {
        return Err(IngestionError::StorageUnavailable);
    }
    let store = Store::open(&runtime.join("ingestion.sqlite3"), &manifest, &source)?;
    config.deadline = config
        .deadline
        .checked_sub(start.elapsed())
        .filter(|time| !time.is_zero())
        .ok_or(IngestionError::Timeout)?;
    let report = reread_bounded(&config, &source, &manifest, &store, &[range])?;
    println!(
        "synthetic bounded reread: verified={} progress=unchanged evidence=unchanged",
        report.verified_records
    );
    Ok(())
}

fn consume_runtime(
    path: &str,
    bootstrap_servers: &str,
    max_records: usize,
    seconds: u64,
    start: Instant,
) -> Result<(), IngestionError> {
    let runtime = owned_runtime(path)?;
    let (manifest, source) = runtime_contract(&runtime)?;
    let mut config = KafkaConfig {
        bootstrap_servers: bootstrap_servers.to_owned(),
        source_epoch_path: runtime.join("source-epoch.json"),
        max_records,
        deadline: Duration::from_secs(seconds),
    };
    config.validate()?;
    let mut store = Store::open(&runtime.join("ingestion.sqlite3"), &manifest, &source)?;
    config.deadline = config
        .deadline
        .checked_sub(start.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(IngestionError::Timeout)?;
    let report = consume_bounded(&config, &source, &manifest, &mut store)?;
    let statistics = store.statistics()?;
    println!(
        "synthetic bounded ingestion: committed={} duplicates={} variants={} accepted={} rejected={}; receiver acceptance unverified",
        report.committed_records,
        report.duplicate_deliveries,
        report.inserted_variants,
        statistics.accepted_deliveries,
        statistics.rejected_deliveries,
    );
    Ok(())
}

fn replay_runtime(path: &str, owner: &str, vehicle: &str) -> Result<(), IngestionError> {
    let runtime = owned_runtime(path)?;
    let (manifest, source) = runtime_contract(&runtime)?;
    let owner = OwnerId::new(owner).map_err(|_| IngestionError::InvalidCommand)?;
    let vehicle = VehicleId::new(vehicle).map_err(|_| IngestionError::InvalidCommand)?;
    if !manifest
        .config()
        .mappings
        .iter()
        .any(|mapping| mapping.owner == owner.as_str() && mapping.vehicle == vehicle.as_str())
    {
        return Err(IngestionError::DomainReplay);
    }
    let database = runtime.join("ingestion.sqlite3");
    if !database.is_file() {
        return Err(IngestionError::StorageUnavailable);
    }
    let store = Store::open(&database, &manifest, &source)?;
    print_scoped_summary(&store, &owner, &vehicle)
}

fn print_scoped_summary(
    store: &Store,
    owner: &OwnerId,
    vehicle: &VehicleId,
) -> Result<(), IngestionError> {
    let summary = store.summary(owner, vehicle)?;
    let sessions = store.sessions(owner, vehicle)?;
    println!(
        "synthetic scoped replay: sessions={} observed_ac={} eligible_shared_charger={} held_or_excluded={}; physical accuracy unvalidated",
        sessions.len(),
        summary.observed_ac,
        summary.eligible_shared_charger,
        summary.held_or_excluded.len(),
    );
    Ok(())
}

fn runtime_contract(runtime: &Path) -> Result<(Manifest, SourceBinding), IngestionError> {
    let source = read_source_marker(&runtime.join("source-epoch.json"))?;
    let manifest = Manifest::from_json(&read_private_file(
        &runtime.join("manifest.json"),
        MAX_MANIFEST_BYTES,
    )?)
    .map_err(|_| IngestionError::InvalidConfiguration)?;
    Ok((manifest, source))
}

fn owned_runtime(input: &str) -> Result<PathBuf, IngestionError> {
    let path = Path::new(input);
    if path.file_name().and_then(|name| name.to_str()) != Some(".local-runtime")
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(IngestionError::InvalidConfiguration);
    }
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| IngestionError::InvalidConfiguration)?
            .join(path)
    };
    reject_symlink_ancestors(&path)?;
    validate_private_directory(&path)?;
    let ownership: RuntimeOwnership = serde_json::from_slice(&read_private_file(
        &path.join("synthetic-runtime.json"),
        1_024,
    )?)
    .map_err(|_| IngestionError::InvalidConfiguration)?;
    let suffix = ownership
        .project_name
        .strip_prefix("chargeshare-local-")
        .ok_or(IngestionError::InvalidConfiguration)?;
    if !ownership.synthetic_only
        || suffix.len() != 12
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(IngestionError::InvalidConfiguration);
    }
    Ok(path)
}

fn reject_symlink_ancestors(path: &Path) -> Result<(), IngestionError> {
    for ancestor in path.ancestors() {
        let metadata =
            fs::symlink_metadata(ancestor).map_err(|_| IngestionError::InvalidConfiguration)?;
        if metadata.file_type().is_symlink() {
            return Err(IngestionError::InvalidConfiguration);
        }
    }
    Ok(())
}

fn validate_private_directory(path: &Path) -> Result<(), IngestionError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| IngestionError::InvalidConfiguration)?;
    if !metadata.is_dir() {
        return Err(IngestionError::InvalidConfiguration);
    }
    validate_private_metadata(&metadata)
}

#[cfg(target_os = "linux")]
fn validate_private_metadata(metadata: &fs::Metadata) -> Result<(), IngestionError> {
    use std::os::unix::fs::MetadataExt;

    let process_owner = rustix::process::geteuid().as_raw();
    if metadata.mode() & 0o077 != 0 || metadata.uid() != process_owner {
        return Err(IngestionError::InvalidConfiguration);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn validate_private_metadata(_: &fs::Metadata) -> Result<(), IngestionError> {
    Err(IngestionError::InvalidConfiguration)
}

fn read_private_file(path: &Path, maximum: usize) -> Result<Vec<u8>, IngestionError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| IngestionError::InvalidConfiguration)?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(IngestionError::InvalidConfiguration);
    }
    validate_private_metadata(&metadata)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(IngestionError::InvalidConfiguration);
        }
    }
    let mut contents = Vec::new();
    let file = open_private_file(path)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| IngestionError::InvalidConfiguration)?;
    if !opened_metadata.is_file() || opened_metadata.len() > maximum as u64 {
        return Err(IngestionError::InvalidConfiguration);
    }
    validate_private_metadata(&opened_metadata)?;
    verify_private_file_identity(&metadata, &opened_metadata)?;
    file.take((maximum + 1) as u64)
        .read_to_end(&mut contents)
        .map_err(|_| IngestionError::InvalidConfiguration)?;
    if contents.len() > maximum {
        return Err(IngestionError::InvalidConfiguration);
    }
    let final_metadata =
        fs::symlink_metadata(path).map_err(|_| IngestionError::InvalidConfiguration)?;
    validate_private_metadata(&final_metadata)?;
    verify_private_file_identity(&opened_metadata, &final_metadata)?;
    Ok(contents)
}

#[cfg(target_os = "linux")]
fn verify_private_file_identity(
    expected: &fs::Metadata,
    actual: &fs::Metadata,
) -> Result<(), IngestionError> {
    use std::os::unix::fs::MetadataExt;

    if expected.dev() != actual.dev()
        || expected.ino() != actual.ino()
        || actual.nlink() != 1
        || !actual.is_file()
    {
        return Err(IngestionError::InvalidConfiguration);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn verify_private_file_identity(_: &fs::Metadata, _: &fs::Metadata) -> Result<(), IngestionError> {
    Err(IngestionError::InvalidConfiguration)
}

#[cfg(target_os = "linux")]
fn open_private_file(path: &Path) -> Result<fs::File, IngestionError> {
    use rustix::fs::{Mode, OFlags, open, openat};

    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let mut directory = fs::File::from(
        open(
            if path.is_absolute() { "/" } else { "." },
            directory_flags,
            Mode::empty(),
        )
        .map_err(|_| IngestionError::InvalidConfiguration)?,
    );
    for component in path
        .parent()
        .ok_or(IngestionError::InvalidConfiguration)?
        .components()
    {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                directory = fs::File::from(
                    openat(&directory, name, directory_flags, Mode::empty())
                        .map_err(|_| IngestionError::InvalidConfiguration)?,
                );
            }
            _ => return Err(IngestionError::InvalidConfiguration),
        }
    }
    validate_private_metadata(
        &directory
            .metadata()
            .map_err(|_| IngestionError::InvalidConfiguration)?,
    )?;
    Ok(fs::File::from(
        openat(
            &directory,
            path.file_name()
                .ok_or(IngestionError::InvalidConfiguration)?,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|_| IngestionError::InvalidConfiguration)?,
    ))
}

#[cfg(not(target_os = "linux"))]
fn open_private_file(_: &Path) -> Result<fs::File, IngestionError> {
    Err(IngestionError::InvalidConfiguration)
}

fn with_deadline(
    duration: Duration,
    operation: impl FnOnce(Instant) -> Result<(), IngestionError>,
) -> Result<(), IngestionError> {
    let active = Arc::new(AtomicBool::new(true));
    let watchdog_active = Arc::clone(&active);
    let start = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(duration);
        if watchdog_active.load(Ordering::SeqCst) {
            eprintln!("{}", IngestionError::Timeout);
            std::process::exit(2);
        }
    });
    let result = operation(start);
    active.store(false, Ordering::SeqCst);
    result
}

fn candidate_demo() -> Result<(), IngestionError> {
    let root = std::env::current_dir().map_err(|_| IngestionError::InvalidConfiguration)?;
    if !root
        .join("crates/chargeshare-ingestion/Cargo.toml")
        .is_file()
        || !root.join(".gitignore").is_file()
    {
        return Err(IngestionError::InvalidConfiguration);
    }
    let target = root.join("target");
    if !target.exists() {
        fs::create_dir(&target).map_err(|_| IngestionError::StorageUnavailable)?;
    }
    reject_symlink_ancestors(&target)?;
    let runtime = target.join("durable-ingestion-fixture");
    if !runtime.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&runtime)
                .map_err(|_| IngestionError::StorageUnavailable)?;
        }
        #[cfg(not(unix))]
        return Err(IngestionError::InvalidConfiguration);
    }
    reject_symlink_ancestors(&runtime)?;
    validate_private_directory(&runtime)?;
    let manifest_bytes = include_bytes!("../tests/fixtures/manifest-v1.json");
    let manifest =
        Manifest::from_json(manifest_bytes).map_err(|_| IngestionError::InvalidConfiguration)?;
    let source = SourceBinding {
        epoch: "synthetic-candidate-fixture-v1".to_owned(),
        topic: SYNTHETIC_TOPIC.to_owned(),
        cluster_id: None,
        topic_id: None,
        partitions: vec![PartitionStart {
            partition: 0,
            initial_offset: 0,
        }],
    };
    write_unchanged_fixture(&runtime.join("manifest.json"), manifest_bytes)?;
    write_unchanged_fixture(
        &runtime.join("source-epoch.json"),
        &serde_json::to_vec(&source).map_err(|_| IngestionError::InvalidConfiguration)?,
    )?;
    let fixtures: serde_json::Value =
        serde_json::from_slice(include_bytes!("../tests/fixtures/candidate-v.json"))
            .map_err(|_| IngestionError::InvalidConfiguration)?;
    let mut store = Store::open(&runtime.join("ingestion.sqlite3"), &manifest, &source)?;
    for (offset, record) in fixtures["records"]
        .as_array()
        .ok_or(IngestionError::InvalidConfiguration)?
        .iter()
        .enumerate()
    {
        let key = record["key"]
            .as_str()
            .ok_or(IngestionError::InvalidConfiguration)?;
        let payload = serde_json::to_vec(&record["payload"])
            .map_err(|_| IngestionError::InvalidConfiguration)?;
        let position = store.positions()?[0].next_offset;
        store.ingest(
            &manifest,
            DeliveryCoordinate {
                partition: 0,
                offset: i64::try_from(offset).map_err(|_| IngestionError::InvalidConfiguration)?,
            },
            position,
            Some(key.as_bytes()),
            &payload,
        )?;
    }
    println!("normalization/storage fixture; receiver acceptance unverified");
    for mapping in &manifest.config().mappings {
        let owner =
            OwnerId::new(&mapping.owner).map_err(|_| IngestionError::InvalidConfiguration)?;
        let vehicle =
            VehicleId::new(&mapping.vehicle).map_err(|_| IngestionError::InvalidConfiguration)?;
        print_scoped_summary(&store, &owner, &vehicle)?;
    }
    Ok(())
}

fn write_unchanged_fixture(path: &Path, contents: &[u8]) -> Result<(), IngestionError> {
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        if read_private_file(path, MAX_MANIFEST_BYTES)? != contents {
            return Err(IngestionError::ConfigurationMismatch);
        }
        return Ok(());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| IngestionError::StorageUnavailable)?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|_| IngestionError::StorageUnavailable)
}

fn command_options(
    args: &[String],
    allowed: &[&str],
    repeated: &[&str],
) -> Result<std::collections::BTreeMap<String, Vec<String>>, IngestionError> {
    if !args.len().is_multiple_of(2) {
        return Err(IngestionError::InvalidCommand);
    }
    let mut options = std::collections::BTreeMap::<String, Vec<String>>::new();
    for pair in args.as_chunks::<2>().0 {
        if !allowed.contains(&pair[0].as_str()) {
            return Err(IngestionError::InvalidCommand);
        }
        let values = options.entry(pair[0].clone()).or_default();
        if !values.is_empty() && !repeated.contains(&pair[0].as_str()) {
            return Err(IngestionError::InvalidCommand);
        }
        values.push(pair[1].clone());
    }
    Ok(options)
}

fn required_option(
    options: &std::collections::BTreeMap<String, Vec<String>>,
    name: &str,
) -> Result<String, IngestionError> {
    options
        .get(name)
        .and_then(|values| values.first())
        .cloned()
        .ok_or(IngestionError::InvalidCommand)
}

fn car_command(args: &[String]) -> Result<(), IngestionError> {
    let command = args.first().ok_or(IngestionError::InvalidCommand)?;
    let allowed: &[&str] = match command.as_str() {
        "add" => &[
            "--database",
            "--vin",
            "--vehicle",
            "--owner",
            "--name",
            "--charger",
            "--make",
            "--model",
            "--year",
        ],
        "rename" => &["--database", "--vehicle", "--name"],
        "set-color" => &["--database", "--vehicle", "--color"],
        "remove" => &["--database", "--vehicle"],
        _ => return Err(IngestionError::InvalidCommand),
    };
    let options = command_options(&args[1..], allowed, &["--charger"])?;
    let database = PathBuf::from(required_option(&options, "--database")?);
    let vehicle = required_option(&options, "--vehicle")?;
    let outcome = match command.as_str() {
        "add" => Store::add_car(
            &database,
            chargeshare_ingestion::CarRegistration {
                device: required_option(&options, "--vin")?,
                vehicle,
                owner: required_option(&options, "--owner")?,
                name: required_option(&options, "--name")?,
                chargers: options
                    .get("--charger")
                    .cloned()
                    .ok_or(IngestionError::InvalidCommand)?,
                make: options
                    .get("--make")
                    .and_then(|values| values.first())
                    .cloned(),
                model: options
                    .get("--model")
                    .and_then(|values| values.first())
                    .cloned(),
                year: options
                    .get("--year")
                    .and_then(|values| values.first())
                    .map(|value| value.parse().map_err(|_| IngestionError::InvalidCommand))
                    .transpose()?,
            },
        )?,
        "rename" => Store::rename_car(&database, &vehicle, &required_option(&options, "--name")?)?,
        "set-color" => {
            Store::set_car_color(&database, &vehicle, &required_option(&options, "--color")?)?
        }
        "remove" => Store::remove_car(&database, &vehicle)?,
        _ => return Err(IngestionError::InvalidCommand),
    };
    println!(
        "{}",
        match outcome {
            chargeshare_ingestion::RegistryWrite::RecoveryPending =>
                "Car stored; retained recovery pending.",
            chargeshare_ingestion::RegistryWrite::RecoveryComplete =>
                "Car already stored; retained recovery complete.",
            chargeshare_ingestion::RegistryWrite::Updated => "Car registry updated.",
        }
    );
    Ok(())
}

fn registry_command(args: &[String]) -> Result<(), IngestionError> {
    if args.first().map(String::as_str) == Some("set-rate") {
        let options = command_options(
            &args[1..],
            &["--database", "--charger", "--active-from", "--definition"],
            &[],
        )?;
        let database = PathBuf::from(required_option(&options, "--database")?);
        let definition_path = PathBuf::from(required_option(&options, "--definition")?);
        let contents =
            fs::read(definition_path).map_err(|_| IngestionError::InvalidConfiguration)?;
        if contents.len() > MAX_MANIFEST_BYTES {
            return Err(IngestionError::InvalidConfiguration);
        }
        let definition =
            serde_json::from_slice(&contents).map_err(|_| IngestionError::InvalidConfiguration)?;
        let active_from = required_option(&options, "--active-from")?
            .parse::<i64>()
            .map_err(|_| IngestionError::InvalidCommand)?;
        Store::assign_charger_rate(
            &database,
            &required_option(&options, "--charger")?,
            active_from,
            &definition,
        )?;
        println!("Charger rate assignment saved; earlier history preserved.");
        return Ok(());
    }
    if args.first().map(String::as_str) != Some("migrate") {
        return Err(IngestionError::InvalidCommand);
    }
    let options = command_options(
        &args[1..],
        &[
            "--database",
            "--bootstrap",
            "--car-name",
            "--charger-name",
            "--charger-owner",
        ],
        &["--car-name"],
    )?;
    let database = PathBuf::from(required_option(&options, "--database")?);
    let bootstrap = chargeshare_ingestion::RegistryBootstrap::from_json(
        &private_runtime_settings(&required_option(&options, "--bootstrap")?)?,
    )?;
    let mut car_names = std::collections::BTreeMap::new();
    for entry in options
        .get("--car-name")
        .ok_or(IngestionError::InvalidCommand)?
    {
        let (vehicle, name) = entry
            .split_once('=')
            .ok_or(IngestionError::InvalidCommand)?;
        if car_names
            .insert(vehicle.to_owned(), name.to_owned())
            .is_some()
        {
            return Err(IngestionError::InvalidCommand);
        }
    }
    let setup = chargeshare_ingestion::RegistrySetup {
        car_names,
        charger_name: required_option(&options, "--charger-name")?,
        charger_owner: required_option(&options, "--charger-owner")?,
    };
    chargeshare_ingestion::migrate_registry(&database, &bootstrap, &setup)?;
    println!("Registry migration complete; retained history preserved.");
    Ok(())
}
