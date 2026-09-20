use orbit_research_core::{Error, Research as Corpus, Result};
use serde::Deserialize;
use serde_json::json;
use std::io::Read;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const MAX_BODY: u64 = 64 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    request_key: String,
    title: String,
    body: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    derived_from: Vec<String>,
}
pub fn serve(corpus: Corpus, port: u16) -> Result<()> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| Error::Invalid(e.to_string()))?;
    let address = server.server_addr().to_string();
    let origin = format!("http://{address}");
    let mut random = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let token = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    eprintln!("Orbit Research: {origin}");
    for mut request in server.incoming_requests() {
        let host = header(&request, "Host");
        if host != Some(address.as_str()) {
            reply(
                request,
                403,
                "application/json",
                "{\"error\":\"Unrecognized host\"}",
            )?;
            continue;
        }
        if let Some(supplied) = header(&request, "Origin") {
            if supplied != origin {
                reply(
                    request,
                    403,
                    "application/json",
                    "{\"error\":\"Foreign origin\"}",
                )?;
                continue;
            }
        }
        let path = request.url().to_owned();
        if request.method() == &Method::Get {
            match path.as_str() {
                "/" => reply(
                    request,
                    200,
                    "text/html; charset=utf-8",
                    include_str!("../web/index.html"),
                )?,
                "/app.css" => reply(request, 200, "text/css", include_str!("../web/app.css"))?,
                "/app.js" => reply(
                    request,
                    200,
                    "text/javascript",
                    include_str!("../web/app.js"),
                )?,
                "/api/session" => reply(
                    request,
                    200,
                    "application/json",
                    &json!({"token":token}).to_string(),
                )?,
                "/api/corpus" => match corpus.snapshot() {
                    Ok(snapshot) => reply(
                        request,
                        200,
                        "application/json",
                        &serde_json::to_string(&snapshot)?,
                    )?,
                    Err(error) => error_reply(request, error)?,
                },
                _ => reply(
                    request,
                    404,
                    "application/json",
                    "{\"error\":\"Not found\"}",
                )?,
            }
        } else if request.method() == &Method::Post && path == "/api/questions" {
            if header(&request, "X-Research-Token") != Some(token.as_str())
                || header(&request, "Content-Type") != Some("application/json")
            {
                reply(
                    request,
                    403,
                    "application/json",
                    "{\"error\":\"Missing session token or JSON content type\"}",
                )?;
                continue;
            }
            let mut body = Vec::new();
            request
                .as_reader()
                .take(MAX_BODY + 1)
                .read_to_end(&mut body)?;
            if body.len() as u64 > MAX_BODY {
                reply(
                    request,
                    413,
                    "application/json",
                    "{\"error\":\"Request too large\"}",
                )?;
                continue;
            }
            let outcome = serde_json::from_slice::<Capture>(&body)
                .map_err(Error::from)
                .and_then(|capture| {
                    corpus.reserve(
                        &capture.request_key,
                        "Q",
                        &capture.title,
                        &capture.body,
                        capture.tags,
                        capture.derived_from,
                    )
                });
            match outcome {
                Ok(reservation) => reply(
                    request,
                    201,
                    "application/json",
                    &serde_json::to_string(&reservation)?,
                )?,
                Err(error) => error_reply(request, error)?,
            }
        } else {
            reply(
                request,
                405,
                "application/json",
                "{\"error\":\"Method not allowed\"}",
            )?;
        }
    }
    Ok(())
}
fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}
fn error_reply(request: Request, error: Error) -> Result<()> {
    reply(
        request,
        409,
        "application/json",
        &json!({"error":error.to_string()}).to_string(),
    )
}
fn reply(request: Request, status: u16, content_type: &str, body: &str) -> Result<()> {
    let mut response = Response::from_string(body).with_status_code(StatusCode(status));
    for (name, value) in [
        ("Content-Type", content_type),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        (
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
    ] {
        response.add_header(
            Header::from_bytes(name, value)
                .map_err(|_| Error::Invalid("Invalid response header".into()))?,
        );
    }
    request.respond(response)?;
    Ok(())
}
