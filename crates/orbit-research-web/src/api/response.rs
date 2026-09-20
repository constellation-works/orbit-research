//! Consistent JSON responses and browser security headers.
use orbit_research_core::{Error, Result};
use serde_json::{Value, json};
use tiny_http::{Header, Request, Response, StatusCode};

pub(super) fn error_reply(request: Request, error: Error) -> Result<()> {
    reply(
        request,
        409,
        "application/json",
        &json!({"error":error.to_string()}).to_string(),
    )
}
pub(super) fn reply_json(request: Request, status: u16, value: &Value) -> Result<()> {
    reply(request, status, "application/json", &value.to_string())
}
pub(super) fn reply(request: Request, status: u16, content_type: &str, body: &str) -> Result<()> {
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
