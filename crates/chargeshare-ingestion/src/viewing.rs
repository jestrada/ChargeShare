use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chargeshare_core::{OwnerId, VehicleId};

use crate::kafka::LiveIngestion;
use crate::{IngestionError, KafkaConfig, SourceBinding, Store};

pub fn serve_results(
    listener: TcpListener,
    store: Store,
    config: KafkaConfig,
    source: SourceBinding,
    stop: &AtomicBool,
) -> Result<(), IngestionError> {
    let driver = LiveIngestion::connect(config, source, &store)?;
    serve_with_driver(listener, store, driver, stop)
}

pub(crate) fn serve_with_driver(
    listener: TcpListener,
    mut store: Store,
    mut driver: LiveIngestion,
    stop: &AtomicBool,
) -> Result<(), IngestionError> {
    listener
        .set_nonblocking(true)
        .map_err(|_| IngestionError::StorageUnavailable)?;
    let mut failure = None;
    while !stop.load(Ordering::SeqCst) {
        if failure.is_none() {
            failure = driver.poll(&mut store).err();
        }
        match listener.accept() {
            Ok((stream, _)) => respond(stream, &store, failure),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return Err(IngestionError::StorageUnavailable),
        }
        if failure.is_some() || store.validate_live_storage().is_err() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

fn respond(mut stream: TcpStream, store: &Store, failure: Option<IngestionError>) {
    let timeout = Some(Duration::from_millis(250));
    if stream.set_read_timeout(timeout).is_err() || stream.set_write_timeout(timeout).is_err() {
        return;
    }
    let request = read_request(&mut stream);
    let (status, body) = match request {
        Ok(request) => route(&request, store, failure),
        Err(()) => safe_error(400, "invalid_request"),
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Service Unavailable",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn read_request(stream: &mut TcpStream) -> Result<String, ()> {
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while request.len() < 4096 && Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(());
        }
        stream.set_read_timeout(Some(remaining)).map_err(|_| ())?;
        let count = stream.read(&mut buffer).map_err(|_| ())?;
        if count == 0 {
            return Err(());
        }
        request.extend_from_slice(&buffer[..count]);
        if request.len() > 4096 {
            return Err(());
        }
        if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            return String::from_utf8(request).map_err(|_| ());
        }
    }
    Err(())
}

fn route(request: &str, store: &Store, failure: Option<IngestionError>) -> (u16, String) {
    let parts: Vec<_> = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    if parts.len() != 3 || !matches!(parts[2], "HTTP/1.1" | "HTTP/1.0") {
        return safe_error(400, "invalid_request");
    }
    if parts[0] != "GET" {
        return safe_error(405, "method_refused");
    }
    if parts[1] == "/api/poc/ready" {
        return if failure.is_none() && store.validate_live_storage().is_ok() {
            (
                200,
                format!(
                    "{{\"state\":\"ready\",\"synthetic_only\":{}}}",
                    !store.manifest.is_live()
                ),
            )
        } else {
            safe_error(503, "results_unavailable")
        };
    }
    if parts[1] == "/api/poc/scopes" {
        if failure.is_some() {
            return safe_error(503, "results_unavailable");
        }
        let Ok(scopes) = store.active_scopes() else {
            return safe_error(503, "results_unavailable");
        };
        return (
            200,
            serde_json::json!({"synthetic_only": !store.manifest.is_live(), "scopes": scopes})
                .to_string(),
        );
    }
    let (target, query) = parts[1].split_once('?').unwrap_or((parts[1], ""));
    let charger = if query.is_empty() {
        None
    } else {
        let Some(id) = query
            .strip_prefix("charger=")
            .filter(|id| VehicleId::new(id).is_ok())
        else {
            return safe_error(400, "charger_refused");
        };
        let requested_vehicle = target.split('/').nth(6).unwrap_or_default();
        let linked = if let Some(registry) = &store.registry {
            registry.cars.iter().any(|car| {
                car.registration.vehicle == requested_vehicle
                    && car
                        .registration
                        .chargers
                        .iter()
                        .any(|charger| charger == id)
            })
        } else {
            store
                .manifest
                .config()
                .mappings
                .iter()
                .find(|mapping| mapping.vehicle == requested_vehicle)
                .is_some_and(|mapping| {
                    store
                        .manifest
                        .config()
                        .owner_chargers
                        .iter()
                        .any(|charger| {
                            charger.owner_id == mapping.owner && charger.charger_id == id
                        })
                })
        };
        if !linked {
            return safe_error(404, "charger_refused");
        }
        Some(id)
    };
    let path: Vec<_> = target.split('/').collect();
    if path.len() != 8
        || path[..4] != ["", "api", "poc", "owners"]
        || path[5] != "vehicles"
        || path[7] != "results"
    {
        return safe_error(404, "route_refused");
    }
    let (Ok(owner), Ok(vehicle)) = (OwnerId::new(path[4]), VehicleId::new(path[6])) else {
        return safe_error(400, "scope_refused");
    };
    if !store
        .manifest
        .config()
        .mappings
        .iter()
        .any(|mapping| mapping.owner == owner.as_str() && mapping.vehicle == vehicle.as_str())
    {
        return safe_error(404, "scope_refused");
    }
    if failure.is_some() {
        return safe_error(503, "results_unavailable");
    }
    let result = if let Some(charger) = charger {
        store.results_for_charger(&owner, &vehicle, charger)
    } else {
        store.results(&owner, &vehicle)
    };
    match result
        .and_then(|result| serde_json::to_string(&result).map_err(|_| IngestionError::DomainReplay))
    {
        Ok(body) => (200, body),
        Err(_) => safe_error(503, "results_unavailable"),
    }
}

fn safe_error(status: u16, reason: &str) -> (u16, String) {
    (status, format!("{{\"error\":\"{reason}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_dashboard_charger_choice_preserves_the_existing_result_contract() {
        let directory =
            std::env::temp_dir().join(format!("chargeshare-view-charger-{}", std::process::id()));
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let manifest =
            crate::Manifest::from_json(include_bytes!("../../../dev/ingestion-manifest.json"))
                .unwrap();
        let source = crate::SourceBinding {
            epoch: "synthetic-view-charger-v1".to_owned(),
            topic: crate::SYNTHETIC_TOPIC.to_owned(),
            cluster_id: None,
            topic_id: None,
            partitions: vec![crate::PartitionStart {
                partition: 0,
                initial_offset: 0,
            }],
        };
        let store = Store::open(&directory.join("ingestion.sqlite3"), &manifest, &source).unwrap();
        let path = "/api/poc/owners/owner-a/vehicles/vehicle-a/results";
        let all = route(&format!("GET {path} HTTP/1.1\r\n\r\n"), &store, None);
        let selected = route(
            &format!("GET {path}?charger=fictional-shared-charger HTTP/1.1\r\n\r\n"),
            &store,
            None,
        );
        assert_eq!(all.0, 200);
        assert_eq!(all, selected);
        assert_eq!(
            route(
                &format!("GET {path}?charger=unknown HTTP/1.1\r\n\r\n"),
                &store,
                None
            ),
            safe_error(404, "charger_refused")
        );
        drop(store);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
