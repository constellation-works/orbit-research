//! HTTP request shapes and bounded query/header extraction.
use serde::Deserialize;
use serde_json::Value;
use tiny_http::Request;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Capture {
    pub(crate) request_key: String,
    pub(crate) title: String,
    pub(crate) body: String,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) derived_from: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationRequest {
    pub(crate) operation: String,
    pub(crate) arguments: Value,
}

pub(crate) fn query_value<'a>(query: &'a str, wanted: &str) -> Option<&'a str> {
    query.split('&').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key == wanted && !value.is_empty() && value.len() <= 256).then_some(value)
    })
}

pub(crate) fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}
