//! Flat route dispatch; every endpoint shares the same HTTP boundary.
use super::request::{HttpRequest, HttpResult, Reply};
use crate::parse::{Capture, OperationRequest, query_value};
use orbit_research_core::{Application, Result, application::Operation};
use serde_json::{Value, json};
use tiny_http::{Method, Request};

pub(crate) fn handle_request(
    request: Request,
    application: &Application,
    address: &str,
    origin: &str,
    token: &str,
) -> Result<()> {
    let mut request = HttpRequest::new(request);
    let result = request
        .validate(address, origin, token)
        .and_then(|()| route(&mut request, application, token));
    request.reply(result.unwrap_or_else(|error| error))
}

fn route(request: &mut HttpRequest, application: &Application, token: &str) -> HttpResult<Reply> {
    match (request.method(), request.path()) {
        (Method::Get, "/") => Ok(Reply::text(
            200,
            "text/html; charset=utf-8",
            include_str!("../../assets/dashboard/index.html"),
        )),
        (Method::Get, "/app.css") => Ok(Reply::text(
            200,
            "text/css",
            include_str!("../../assets/dashboard/app.css"),
        )),
        (Method::Get, "/app.js") => Ok(Reply::text(
            200,
            "text/javascript",
            include_str!("../../assets/dashboard/app.js"),
        )),
        (Method::Get, "/api/session") => Ok(Reply::json(200, json!({ "token": token }))),
        (Method::Get, "/api/corpus") => execute(application, Operation::List, json!({})),
        (Method::Get, "/api/work-links") => execute(application, Operation::WorkLinks, json!({})),
        (Method::Get, "/api/backend") => execute(application, Operation::Backend, json!({})),
        (Method::Get, "/api/work-status") => work_status(request, application),
        (Method::Post, "/api/questions") => capture(request, application),
        (Method::Post, "/api/operations") => operate(request, application),
        (Method::Get | Method::Post, _) => Err(Reply::error(404, "Not found")),
        _ => Err(Reply::error(405, "Method not allowed")),
    }
}

fn work_status(request: &HttpRequest, application: &Application) -> HttpResult<Reply> {
    let request_key = query_value(request.query(), "request_key")
        .ok_or_else(|| Reply::error(400, "request_key is required"))?;
    execute(
        application,
        Operation::WorkStatus,
        json!({ "request_key": request_key }),
    )
}

fn capture(request: &mut HttpRequest, application: &Application) -> HttpResult<Reply> {
    let capture: Capture = request.json()?;
    execute(
        application,
        Operation::Create,
        json!({
            "request_key": capture.request_key,
            "kind": "Q",
            "title": capture.title,
            "body": capture.body,
            "tags": capture.tags,
            "derived_from": capture.derived_from,
        }),
    )
}

fn operate(request: &mut HttpRequest, application: &Application) -> HttpResult<Reply> {
    let envelope: OperationRequest = request.json()?;
    if envelope.operation.len() > 128 || !envelope.arguments.is_object() {
        return Err(Reply::error(
            400,
            "operation must be <=128 bytes with object arguments",
        ));
    }
    execute(application, envelope.operation.parse()?, envelope.arguments)
}

fn execute(application: &Application, operation: Operation, arguments: Value) -> HttpResult<Reply> {
    let status = match operation {
        Operation::Create => 201,
        _ => 200,
    };
    Ok(Reply::json(
        status,
        application.execute(operation, arguments)?,
    ))
}
