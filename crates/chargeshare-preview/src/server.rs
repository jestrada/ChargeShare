use serde_json::{Value, json};
use tiny_http::{Header, Method, Response, Server};

use crate::demo;

fn route(method: &Method, url: &str) -> (u16, Value) {
    if method != &Method::Get {
        return (405, json!({ "error": "Only GET is supported." }));
    }
    let missing = match url {
        "/api/demo" | "/api/demo?scenario=complete" => false,
        "/api/demo?scenario=missing" => true,
        value if value.starts_with("/api/demo?") => {
            return (400, json!({ "error": "Unknown demo scenario." }));
        }
        _ => return (404, json!({ "error": "Not found." })),
    };
    match demo::snapshot(missing) {
        Ok(data) => (200, data),
        Err(_) => (500, json!({ "error": "The demo could not be calculated." })),
    }
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http("127.0.0.1:8787")?;
    println!("ChargeShare synthetic preview API: http://127.0.0.1:8787/api/demo");
    for request in server.incoming_requests() {
        let (status, body) = route(request.method(), request.url());
        let mut response = Response::from_string(body.to_string()).with_status_code(status);
        for (name, value) in [
            ("Content-Type", "application/json; charset=utf-8"),
            ("Cache-Control", "no-store"),
            ("X-Content-Type-Options", "nosniff"),
        ] {
            response.add_header(Header::from_bytes(name, value).expect("valid static header"));
        }
        if request.respond(response).is_err() {
            eprintln!("Preview response connection closed.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::route;
    use tiny_http::Method;

    #[test]
    fn routes_are_get_only_and_unknown_input_is_not_reflected() {
        assert_eq!(route(&Method::Get, "/api/demo").0, 200);
        assert_eq!(route(&Method::Get, "/api/demo?scenario=missing").0, 200);
        assert_eq!(route(&Method::Post, "/api/demo").0, 405);
        assert_eq!(route(&Method::Get, "/private").0, 404);
        for url in [
            "/api/demo?scenario=missing&scenario=complete",
            "/api/demo?unknown=secret",
            "/api/demo?scenario=%3Cscript%3E",
        ] {
            let (status, body) = route(&Method::Get, url);
            assert_eq!(status, 400);
            assert_eq!(body["error"], "Unknown demo scenario.");
        }
    }
}
