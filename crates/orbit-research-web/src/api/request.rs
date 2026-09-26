//! One HTTP boundary for request guards, bounded JSON bodies and response headers.
use crate::parse::header;
use orbit_research_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::io::{Cursor, Read};
use tiny_http::{Header, Method, Request, Response, StatusCode};

const MAX_BODY: u64 = 64 * 1024;

pub(super) type HttpResult<T> = std::result::Result<T, Reply>;

pub(super) struct HttpRequest {
    inner: Request,
}

impl HttpRequest {
    pub(super) fn new(inner: Request) -> Self {
        Self { inner }
    }

    pub(super) fn method(&self) -> &Method {
        self.inner.method()
    }

    pub(super) fn path(&self) -> &str {
        self.inner.url().split('?').next().unwrap_or_default()
    }

    pub(super) fn validate(&self, address: &str, origin: &str, token: &str) -> HttpResult<()> {
        if header(&self.inner, "Host") != Some(address) {
            return Err(Reply::error(403, "Unrecognized host"));
        }
        if header(&self.inner, "Origin").is_some_and(|supplied| supplied != origin) {
            return Err(Reply::error(403, "Foreign origin"));
        }
        if self.method() == &Method::Post
            && (header(&self.inner, "X-Research-Token") != Some(token)
                || header(&self.inner, "Content-Type") != Some("application/json"))
        {
            return Err(Reply::error(
                403,
                "Missing session token or JSON content type",
            ));
        }
        Ok(())
    }

    pub(super) fn json<T: DeserializeOwned>(&mut self) -> HttpResult<T> {
        let mut body = Vec::new();
        self.inner
            .as_reader()
            .take(MAX_BODY + 1)
            .read_to_end(&mut body)
            .map_err(|error| Reply::error(400, &error.to_string()))?;
        if body.len() as u64 > MAX_BODY {
            return Err(Reply::error(413, "Request too large"));
        }
        serde_json::from_slice(&body).map_err(|error| Reply::error(400, &error.to_string()))
    }

    pub(super) fn reply(self, reply: Reply) -> Result<()> {
        self.inner.respond(reply.into_response()?)?;
        Ok(())
    }
}

pub(super) struct Reply {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl Reply {
    pub(super) fn text(status: u16, content_type: &'static str, body: &str) -> Self {
        Self {
            status,
            content_type,
            body: body.to_owned(),
        }
    }

    pub(super) fn json(status: u16, value: Value) -> Self {
        Self::text(status, "application/json", &value.to_string())
    }

    pub(super) fn error(status: u16, message: &str) -> Self {
        Self::json(status, json!({ "error": message }))
    }

    pub(super) fn into_response(self) -> Result<Response<Cursor<Vec<u8>>>> {
        let mut response =
            Response::from_string(self.body).with_status_code(StatusCode(self.status));
        for (name, value) in [
            ("Content-Type", self.content_type),
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
        Ok(response)
    }
}

impl From<Error> for Reply {
    fn from(error: Error) -> Self {
        Self::error(409, &error.to_string())
    }
}
