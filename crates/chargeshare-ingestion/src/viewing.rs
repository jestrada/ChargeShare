use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rustix::fs::{Mode, OFlags, open, openat};

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
    serve_with_driver(listener, store, driver, stop, None)
}

pub(crate) fn serve_with_driver(
    listener: TcpListener,
    mut store: Store,
    mut driver: LiveIngestion,
    stop: &AtomicBool,
    private_car_images_dir: Option<&Path>,
) -> Result<(), IngestionError> {
    let images = private_car_images_dir.and_then(open_image_directory);
    listener
        .set_nonblocking(true)
        .map_err(|_| IngestionError::StorageUnavailable)?;
    let mut failure = None;
    while !stop.load(Ordering::SeqCst) {
        if failure.is_none() {
            failure = driver.poll(&mut store).err();
        }
        match listener.accept() {
            Ok((stream, _)) => respond(stream, &store, failure, images.as_ref()),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return Err(IngestionError::StorageUnavailable),
        }
        if failure.is_some() || store.validate_live_storage().is_err() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

fn respond(
    mut stream: TcpStream,
    store: &Store,
    failure: Option<IngestionError>,
    images: Option<&File>,
) {
    let timeout = Some(Duration::from_millis(250));
    if stream.set_read_timeout(timeout).is_err() || stream.set_write_timeout(timeout).is_err() {
        return;
    }
    let request = read_request(&mut stream);
    let response = match request {
        Ok(request) => route(&request, store, failure, images),
        Err(()) => safe_error(400, "invalid_request"),
    };
    let status = response.status;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Service Unavailable",
    };
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        response.content_type,
        response.body.len()
    );
    if stream.write_all(headers.as_bytes()).is_ok() {
        let _ = stream.write_all(&response.body);
    }
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

fn route(
    request: &str,
    store: &Store,
    failure: Option<IngestionError>,
    images: Option<&File>,
) -> Response {
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
    if let Some(name) = parts[1].strip_prefix("/api/poc/car-images/") {
        return images
            .and_then(|directory| private_car_image(directory, name))
            .unwrap_or_else(|| safe_error(404, "image_unavailable"));
    }
    if parts[1] == "/api/poc/ready" {
        return if failure.is_none() && store.validate_live_storage().is_ok() {
            json_response(
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
        return json_response(
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
        Ok(body) => json_response(200, body),
        Err(_) => safe_error(503, "results_unavailable"),
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

fn json_response(status: u16, body: String) -> Response {
    Response {
        status,
        content_type: "application/json",
        body: body.into_bytes(),
    }
}

fn safe_error(status: u16, reason: &str) -> Response {
    json_response(status, format!("{{\"error\":\"{reason}\"}}"))
}

fn open_image_directory(path: &Path) -> Option<File> {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
    if path.starts_with(checkout) {
        return None;
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open("/", flags, Mode::empty()).ok()?;
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    for component in components {
        let Component::Normal(name) = component else {
            return None;
        };
        directory = openat(&directory, name, flags, Mode::empty()).ok()?;
    }
    Some(directory.into())
}

const MAX_CAR_IMAGE_BYTES: u64 = 2 * 1024 * 1024;

fn private_car_image(directory: &File, name: &str) -> Option<Response> {
    let color = name.strip_suffix(".png")?;
    if color.len() != 6
        || !color
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .ok()?;
    let file = File::from(descriptor);
    let metadata = file.metadata().ok()?;
    use std::os::unix::fs::PermissionsExt;
    if !metadata.is_file()
        || metadata.len() > MAX_CAR_IMAGE_BYTES
        || metadata.permissions().mode() & 0o444 == 0
    {
        return None;
    }
    let mut body = Vec::new();
    file.take(MAX_CAR_IMAGE_BYTES + 1)
        .read_to_end(&mut body)
        .ok()?;
    if body.len() > MAX_CAR_IMAGE_BYTES as usize
        || body.len() < 45
        || !body.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        || !body.ends_with(b"\0\0\0\0IEND\xaeB`\x82")
    {
        return None;
    }
    let width = u32::from_be_bytes(body[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(body[20..24].try_into().ok()?);
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }
    Some(Response {
        status: 200,
        content_type: "image/png",
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 216, 177,
            247, 240, 127, 0, 7, 161, 3, 56, 60, 220, 185, 249, 0, 0, 0, 0, 73, 69, 78, 68, 174,
            66, 96, 130,
        ]
    }

    #[test]
    fn private_images_are_bounded_pngs_and_never_follow_symlinks_or_request_paths() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let path =
            Path::new("/tmp").join(format!("chargeshare-private-images-{}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        let images = path.join("images");
        std::fs::create_dir(&images).unwrap();
        let png = synthetic_png();
        std::fs::write(images.join("b8bdc3.png"), &png).unwrap();
        let directory = open_image_directory(&images).unwrap();
        let response = private_car_image(&directory, "b8bdc3.png").unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "image/png");
        assert_eq!(response.body, png);
        for name in [
            "",
            "../images/b8bdc3.png",
            "b8bdc3.png/..",
            "%2e%2e/b8bdc3.png",
            "/b8bdc3.png",
            "B8BDC3.png",
            "b8bdc3.jpg",
            "b8bdc3.svg",
            "000000.png",
            "b8bdc3.png?path=../",
            "b8bdc3.png%00",
        ] {
            assert!(private_car_image(&directory, name).is_none(), "{name}");
        }
        assert!(open_image_directory(Path::new("relative")).is_none());
        assert!(open_image_directory(&images.join("..")).is_none());
        assert!(open_image_directory(&path.join("missing")).is_none());
        assert!(open_image_directory(Path::new(env!("CARGO_MANIFEST_DIR"))).is_none());
        symlink(&images, path.join("linked-directory")).unwrap();
        assert!(open_image_directory(&path.join("linked-directory")).is_none());
        assert!(open_image_directory(&path.join("linked-directory/child")).is_none());
        std::fs::write(path.join("outside.png"), &png).unwrap();
        symlink(path.join("outside.png"), images.join("16181c.png")).unwrap();
        assert!(private_car_image(&directory, "16181c.png").is_none());
        symlink(images.join("b8bdc3.png"), images.join("808080.png")).unwrap();
        assert!(private_car_image(&directory, "808080.png").is_none());
        std::fs::set_permissions(
            images.join("b8bdc3.png"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        assert!(private_car_image(&directory, "b8bdc3.png").is_none());
        std::fs::set_permissions(
            images.join("b8bdc3.png"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        std::fs::write(images.join("b8bdc3.png"), b"unsupported synthetic image").unwrap();
        assert!(private_car_image(&directory, "b8bdc3.png").is_none());
        std::fs::write(images.join("b8bdc3.png"), &png[..png.len() - 1]).unwrap();
        assert!(private_car_image(&directory, "b8bdc3.png").is_none());
        let mut huge_dimensions = png.clone();
        huge_dimensions[16..20].copy_from_slice(&4097u32.to_be_bytes());
        std::fs::write(images.join("b8bdc3.png"), huge_dimensions).unwrap();
        assert!(private_car_image(&directory, "b8bdc3.png").is_none());
        File::create(images.join("b8bdc3.png"))
            .unwrap()
            .set_len(MAX_CAR_IMAGE_BYTES + 1)
            .unwrap();
        assert!(private_car_image(&directory, "b8bdc3.png").is_none());
        std::fs::remove_file(images.join("b8bdc3.png")).unwrap();
        std::fs::create_dir(images.join("b8bdc3.png")).unwrap();
        assert!(private_car_image(&directory, "b8bdc3.png").is_none());
        std::fs::remove_dir(images.join("b8bdc3.png")).unwrap();
        rustix::fs::mknodat(
            &directory,
            "b8bdc3.png",
            rustix::fs::FileType::Fifo,
            Mode::RUSR | Mode::WUSR,
            0,
        )
        .unwrap();
        assert!(private_car_image(&directory, "b8bdc3.png").is_none());
        std::fs::remove_file(images.join("b8bdc3.png")).unwrap();
        std::fs::write(images.join("b8bdc3.png"), &png).unwrap();
        std::fs::rename(&images, path.join("original")).unwrap();
        symlink(&path, &images).unwrap();
        assert_eq!(
            private_car_image(&directory, "b8bdc3.png").unwrap().body,
            png
        );
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn synthetic_dashboard_charger_choice_preserves_the_existing_result_contract() {
        let directory =
            Path::new("/tmp").join(format!("chargeshare-view-charger-{}", std::process::id()));
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
        let all = route(&format!("GET {path} HTTP/1.1\r\n\r\n"), &store, None, None);
        let selected = route(
            &format!("GET {path}?charger=fictional-shared-charger HTTP/1.1\r\n\r\n"),
            &store,
            None,
            None,
        );
        let images = open_image_directory(&directory).unwrap();
        let png = synthetic_png();
        std::fs::write(directory.join("b8bdc3.png"), &png).unwrap();
        assert_eq!(
            route(
                "GET /api/poc/car-images/b8bdc3.png HTTP/1.1\r\n\r\n",
                &store,
                None,
                Some(&images)
            )
            .body,
            png
        );
        assert_eq!(
            route(
                "GET /api/poc/car-images/b8bdc3.png HTTP/1.1\r\n\r\n",
                &store,
                None,
                None
            ),
            safe_error(404, "image_unavailable")
        );
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::scope(|threads| {
            let client = threads.spawn(move || {
                let mut stream = TcpStream::connect(address).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .write_all(
                        b"GET /api/poc/car-images/b8bdc3.png HTTP/1.1\r\nHost: localhost\r\n\r\n",
                    )
                    .unwrap();
                let mut received = Vec::new();
                stream.read_to_end(&mut received).unwrap();
                received
            });
            let (stream, _) = listener.accept().unwrap();
            respond(stream, &store, None, Some(&images));
            let received = client.join().unwrap();
            let boundary = received
                .windows(4)
                .position(|bytes| bytes == b"\r\n\r\n")
                .unwrap()
                + 4;
            let headers = std::str::from_utf8(&received[..boundary]).unwrap();
            assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
            assert!(headers.contains("Content-Type: image/png\r\n"));
            assert!(headers.contains(&format!("Content-Length: {}\r\n", png.len())));
            assert!(headers.contains("Cache-Control: no-store\r\n"));
            assert!(headers.contains("X-Content-Type-Options: nosniff\r\n"));
            assert_eq!(&received[boundary..], png);
        });
        for target in [
            "/api/poc/car-images/",
            "/api/poc/car-images",
            "/api/poc/car-images/../outside.png",
            "/api/poc/car-images/000000.png",
        ] {
            let response = route(
                &format!("GET {target} HTTP/1.1\r\n\r\n"),
                &store,
                None,
                Some(&images),
            );
            assert_eq!(response.status, 404);
            assert!(
                !String::from_utf8(response.body)
                    .unwrap()
                    .contains(directory.to_str().unwrap())
            );
        }
        assert_eq!(
            route(
                "POST /api/poc/car-images/808080.png HTTP/1.1\r\n\r\n",
                &store,
                None,
                Some(&images)
            ),
            safe_error(405, "method_refused")
        );
        assert_eq!(all.status, 200);
        assert_eq!(all, selected);
        assert_eq!(
            route(
                &format!("GET {path}?charger=unknown HTTP/1.1\r\n\r\n"),
                &store,
                None,
                None
            ),
            safe_error(404, "charger_refused")
        );
        drop(store);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
