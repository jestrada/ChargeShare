use crate::Result;
use crate::fixtures::{Frame, decode_hex};
use native_tls::{Certificate, Identity, TlsConnector};
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, Message, client_tls_with_config};

const TRANSPORT_DEADLINE: Duration = Duration::from_secs(10);

fn connector(runtime: &Path, identity_name: Option<&str>) -> Result<TlsConnector> {
    let certificates = runtime.join("certificates");
    let mut builder = TlsConnector::builder();
    builder.disable_built_in_roots(true);
    builder.add_root_certificate(Certificate::from_pem(&fs::read(
        certificates.join("ca.crt"),
    )?)?);
    if let Some(name) = identity_name {
        builder.identity(Identity::from_pkcs8(
            &fs::read(certificates.join(format!("{name}.crt")))?,
            &fs::read(certificates.join(format!("{name}.key")))?,
        )?);
    }
    builder
        .build()
        .map_err(|_| "authentication: cannot construct local-test TLS client".into())
}

fn socket() -> Result<TcpStream> {
    let address: SocketAddr = "127.0.0.1:8443".parse()?;
    let stream = TcpStream::connect_timeout(&address, TRANSPORT_DEADLINE)
        .map_err(|_| "transport: receiver unavailable within 10-second connection deadline")?;
    stream.set_read_timeout(Some(TRANSPORT_DEADLINE))?;
    stream.set_write_timeout(Some(TRANSPORT_DEADLINE))?;
    Ok(stream)
}

pub fn exchange(runtime: &Path, frame: &Frame) -> Result<()> {
    let identity_name = match frame.device.as_str() {
        "device-1" => "client",
        "device-2" => "client-device-2",
        _ => return Err("fixture: unsupported fictional device".into()),
    };
    let mut request = "wss://localhost:8443/".into_client_request()?;
    request.headers_mut().insert("Version", "1.0.0".parse()?);
    request
        .headers_mut()
        .insert("X-Network-Interface", "wifi".parse()?);
    let (mut websocket, _) = client_tls_with_config(
        request,
        socket()?,
        None,
        Some(Connector::NativeTls(connector(
            runtime,
            Some(identity_name),
        )?)),
    )
    .map_err(|_| "authentication: local-test mTLS WebSocket handshake failed")?;
    websocket
        .send(Message::Binary(decode_hex(&frame.message_hex)?.into()))
        .map_err(|_| "transport: synthetic frame send failed")?;
    let expected = decode_hex(&frame.acknowledgment_hex)?;
    let deadline_socket = match websocket.get_ref() {
        MaybeTlsStream::NativeTls(stream) => stream.get_ref().try_clone()?,
        _ => return Err("authentication: expected local-test TLS transport".into()),
    };
    with_socket_deadline(deadline_socket, TRANSPORT_DEADLINE, || {
        receive_acknowledgment(&expected, TRANSPORT_DEADLINE, |remaining| {
            let stream = match websocket.get_ref() {
                MaybeTlsStream::NativeTls(stream) => stream.get_ref(),
                _ => return Err("authentication: expected local-test TLS transport".into()),
            };
            stream.set_read_timeout(Some(remaining))?;
            stream.set_write_timeout(Some(remaining))?;
            websocket.read().map_err(|_| {
                "receiver acknowledgment: missing expected ACK at 10-second deadline".into()
            })
        })?;
        let _ = websocket.close(None);
        Ok(())
    })
}

fn with_socket_deadline(
    stream: TcpStream,
    timeout: Duration,
    operation: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let (cancel, cancellation) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if matches!(
            cancellation.recv_timeout(remaining),
            Err(RecvTimeoutError::Timeout)
        ) {
            let _ = stream.shutdown(Shutdown::Both);
            true
        } else {
            false
        }
    });
    let result = operation();
    drop(cancel);
    let expired = watchdog
        .join()
        .map_err(|_| "receiver acknowledgment: deadline watchdog failed")?;
    if expired || Instant::now() >= deadline {
        return Err("receiver acknowledgment: expected ACK deadline exceeded".into());
    }
    result
}

fn receive_acknowledgment(
    expected: &[u8],
    timeout: Duration,
    mut next_message: impl FnMut(Duration) -> Result<Message>,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or("receiver acknowledgment: expected ACK deadline exceeded")?;
        let message = next_message(remaining)?;
        if Instant::now() >= deadline {
            return Err("receiver acknowledgment: expected ACK deadline exceeded".into());
        }
        match message {
            Message::Binary(observed) if observed.as_ref() == expected => return Ok(()),
            Message::Ping(_) | Message::Pong(_) => {}
            _ => return Err("receiver acknowledgment: unexpected protocol response".into()),
        }
    }
}

pub fn status(runtime: &Path) -> Result<()> {
    let mut stream = connector(runtime, Some("client"))?
        .connect("localhost", socket()?)
        .map_err(|_| "readiness: authenticated receiver TLS handshake failed")?;
    stream.write_all(b"GET /status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream
        .take(4096)
        .read_to_string(&mut response)
        .map_err(|_| "readiness: receiver status response timed out")?;
    if !response.starts_with("HTTP/1.1 200") || !response.ends_with("mtls ok") {
        return Err("readiness: expected receiver authenticated status was not observed".into());
    }
    Ok(())
}

pub fn reject_untrusted(runtime: &Path) -> Result<()> {
    for identity in [None, Some("untrusted-client")] {
        let mut request = "wss://localhost:8443/".into_client_request()?;
        request.headers_mut().insert("Version", "1.0.0".parse()?);
        let attempted = client_tls_with_config(
            request,
            socket()?,
            None,
            Some(Connector::NativeTls(connector(runtime, identity)?)),
        );
        if attempted.is_ok() {
            return Err("authentication: receiver accepted an absent or unrelated synthetic client certificate".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn socket_deadline_interrupts_continuous_partial_input() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        let sender = std::thread::spawn(move || {
            for _ in 0..100 {
                if peer.write_all(b"partial").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let mut received = Vec::new();
        let result = with_socket_deadline(
            stream.try_clone().unwrap(),
            Duration::from_millis(40),
            || {
                stream.read_to_end(&mut received)?;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(received.len() < 700);
        sender.join().unwrap();
    }

    #[test]
    fn keepalives_do_not_extend_the_acknowledgment_deadline() {
        let timeout = Duration::from_millis(40);
        let mut previous_remaining = timeout;
        let mut keepalives = 0;
        let result = receive_acknowledgment(b"expected", timeout, |remaining| {
            assert!(remaining < previous_remaining);
            previous_remaining = remaining;
            keepalives += 1;
            std::thread::sleep(Duration::from_millis(10));
            Ok(Message::Ping(Vec::new().into()))
        });
        assert!(result.is_err());
        assert!(keepalives > 0);
        assert!(keepalives <= 4);
    }

    #[test]
    fn exact_acknowledgment_after_keepalives_succeeds() {
        let mut messages = [
            Message::Ping(Vec::new().into()),
            Message::Pong(Vec::new().into()),
            Message::Binary(b"expected".to_vec().into()),
        ]
        .into_iter();
        assert!(
            receive_acknowledgment(b"expected", Duration::from_secs(1), |_| {
                Ok(messages.next().unwrap())
            })
            .is_ok()
        );
    }

    #[test]
    fn mismatched_acknowledgment_and_transport_failure_are_rejected() {
        assert!(
            receive_acknowledgment(b"expected", Duration::from_secs(1), |_| {
                Ok(Message::Binary(b"different".to_vec().into()))
            })
            .is_err()
        );
        assert!(
            receive_acknowledgment(b"expected", Duration::from_secs(1), |_| {
                Err("controlled transport failure".into())
            })
            .is_err()
        );
    }
}
