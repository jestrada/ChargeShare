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
        let scopes: Vec<_> = store
            .manifest
            .config()
            .mappings
            .iter()
            .map(|mapping| serde_json::json!({"owner": mapping.owner, "vehicle": mapping.vehicle}))
            .collect();
        return (
            200,
            serde_json::json!({"synthetic_only": !store.manifest.is_live(), "scopes": scopes})
                .to_string(),
        );
    }
    let path: Vec<_> = parts[1].split('/').collect();
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
    match store
        .results(&owner, &vehicle)
        .and_then(|result| serde_json::to_string(&result).map_err(|_| IngestionError::DomainReplay))
    {
        Ok(body) => (200, body),
        Err(_) => safe_error(503, "results_unavailable"),
    }
}

fn safe_error(status: u16, reason: &str) -> (u16, String) {
    (status, format!("{{\"error\":\"{reason}\"}}"))
}
