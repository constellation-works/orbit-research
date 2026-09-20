//! Loopback HTTP routing and session guards; research policy stays in Core.
mod response;
use crate::parse::{Capture, OperationRequest, header, query_value};
use orbit_research_core::{Application, Error, Result};
use response::{error_reply, reply, reply_json};
use serde_json::{Value, json};
use std::io::Read;
use tiny_http::{Method, Request};

const MAX_BODY: u64 = 64 * 1024;

pub(crate) fn handle_request(
    request: Request,
    application: &Application,
    address: &str,
    origin: &str,
    token: &str,
) -> Result<()> {
    if header(&request, "Host") != Some(address) {
        return reply(
            request,
            403,
            "application/json",
            "{\"error\":\"Unrecognized host\"}",
        );
    }
    if let Some(supplied) = header(&request, "Origin")
        && supplied != origin
    {
        return reply(
            request,
            403,
            "application/json",
            "{\"error\":\"Foreign origin\"}",
        );
    }
    let url = request.url().to_owned();
    let (path, query) = url
        .split_once('?')
        .map_or((url.as_str(), None), |(path, query)| (path, Some(query)));
    if request.method() == &Method::Get {
        return handle_get(request, application, path, query, token);
    }
    if request.method() == &Method::Post {
        return handle_post(request, application, path, token);
    }
    reply(
        request,
        405,
        "application/json",
        "{\"error\":\"Method not allowed\"}",
    )
}

fn handle_get(
    request: Request,
    application: &Application,
    path: &str,
    query: Option<&str>,
    token: &str,
) -> Result<()> {
    match path {
        "/" => reply(
            request,
            200,
            "text/html; charset=utf-8",
            include_str!("../../assets/dashboard/index.html"),
        ),
        "/app.css" => reply(
            request,
            200,
            "text/css",
            include_str!("../../assets/dashboard/app.css"),
        ),
        "/app.js" => reply(
            request,
            200,
            "text/javascript",
            include_str!("../../assets/dashboard/app.js"),
        ),
        "/api/session" => reply_json(request, 200, &json!({"token":token})),
        "/api/corpus" => call_json(request, application, "research.list", json!({}), 200),
        "/api/work-links" => call_json(request, application, "research.work_links", json!({}), 200),
        "/api/backend" => call_json(request, application, "research.backend", json!({}), 200),
        "/api/work-status" => {
            let Some(request_key) = query_value(query.unwrap_or_default(), "request_key") else {
                return reply_json(request, 400, &json!({"error":"request_key is required"}));
            };
            call_json(
                request,
                application,
                "research.work_status",
                json!({"request_key":request_key}),
                200,
            )
        }
        _ => reply(
            request,
            404,
            "application/json",
            "{\"error\":\"Not found\"}",
        ),
    }
}

fn handle_post(
    mut request: Request,
    application: &Application,
    path: &str,
    token: &str,
) -> Result<()> {
    if header(&request, "X-Research-Token") != Some(token)
        || header(&request, "Content-Type") != Some("application/json")
    {
        return reply(
            request,
            403,
            "application/json",
            "{\"error\":\"Missing session token or JSON content type\"}",
        );
    }
    let mut body = Vec::new();
    request
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)?;
    if body.len() as u64 > MAX_BODY {
        return reply(
            request,
            413,
            "application/json",
            "{\"error\":\"Request too large\"}",
        );
    }
    if path == "/api/questions" {
        let result = serde_json::from_slice::<Capture>(&body)
            .map_err(Error::from)
            .and_then(|capture| {
                application.call("research.create", json!({
                "request_key":capture.request_key, "kind":"Q", "title":capture.title,
                "body":capture.body, "tags":capture.tags, "derived_from":capture.derived_from,
            }))
            });
        return match result {
            Ok(value) => reply_json(request, 201, &value),
            Err(error) => error_reply(request, error),
        };
    }
    if path != "/api/operations" {
        return reply(
            request,
            404,
            "application/json",
            "{\"error\":\"Not found\"}",
        );
    }
    let envelope = match serde_json::from_slice::<OperationRequest>(&body) {
        Ok(envelope) if envelope.operation.len() <= 128 && envelope.arguments.is_object() => {
            envelope
        }
        Ok(_) => {
            return reply_json(
                request,
                400,
                &json!({"error":"operation must be <=128 bytes with object arguments"}),
            );
        }
        Err(error) => return error_reply(request, Error::from(error)),
    };
    let status = if envelope.operation == "research.create" {
        201
    } else {
        200
    };
    match application.call(&envelope.operation, envelope.arguments) {
        Ok(value) => reply_json(request, status, &value),
        Err(error) => error_reply(request, error),
    }
}

fn call_json(
    request: Request,
    application: &Application,
    operation: &str,
    arguments: Value,
    status: u16,
) -> Result<()> {
    match application.call(operation, arguments) {
        Ok(value) => reply_json(request, status, &value),
        Err(error) => error_reply(request, error),
    }
}
