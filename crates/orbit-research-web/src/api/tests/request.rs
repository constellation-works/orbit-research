use super::super::request::{HttpRequest, Reply};
use serde_json::Value;
use tiny_http::{Header, Method, StatusCode, TestRequest};

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).expect("fixture header")
}

#[test]
fn all_reply_types_share_security_headers() {
    for reply in [
        Reply::text(200, "text/html", "dashboard"),
        Reply::json(200, serde_json::json!({"token":"fixture"})),
        Reply::error(403, "Foreign origin"),
    ] {
        let response = reply.into_response().expect("response");
        for (name, value) in [
            ("Cache-Control", "no-store"),
            ("X-Content-Type-Options", "nosniff"),
            ("Referrer-Policy", "no-referrer"),
        ] {
            assert!(
                response
                    .headers()
                    .iter()
                    .any(|header| { header.field.equiv(name) && header.value.as_str() == value })
            );
        }
        assert!(response.headers().iter().any(|header| {
            header.field.equiv("Content-Security-Policy")
                && header.value.as_str().contains("frame-ancestors 'none'")
        }));
    }
}

#[test]
fn request_boundary_rejects_foreign_origins_and_unauthenticated_posts() {
    let base = || TestRequest::new().with_header(header("Host", "127.0.0.1:4318"));
    for raw in [
        TestRequest::new(),
        base().with_header(header("Origin", "https://foreign.invalid")),
        base()
            .with_method(Method::Post)
            .with_header(header("Content-Type", "application/json")),
        base()
            .with_method(Method::Post)
            .with_header(header("X-Research-Token", "token")),
    ] {
        let request = HttpRequest::new(raw.into());
        let Err(reply) = request.validate("127.0.0.1:4318", "http://127.0.0.1:4318", "token")
        else {
            panic!("unsafe request accepted");
        };
        assert_eq!(
            reply.into_response().expect("response").status_code(),
            StatusCode(403)
        );
    }
    let request = HttpRequest::new(
        base()
            .with_method(Method::Post)
            .with_header(header("Content-Type", "application/json"))
            .with_header(header("X-Research-Token", "token"))
            .into(),
    );
    assert!(
        request
            .validate("127.0.0.1:4318", "http://127.0.0.1:4318", "token")
            .is_ok()
    );
}

#[test]
fn malformed_json_is_rejected_by_the_shared_reader() {
    let mut request = HttpRequest::new(TestRequest::new().with_body("not JSON").into());
    let Err(reply) = request.json::<Value>() else {
        panic!("invalid JSON accepted");
    };
    assert_eq!(
        reply.into_response().expect("response").status_code(),
        StatusCode(400)
    );
}

#[test]
fn oversized_json_is_rejected_before_deserialization() {
    let body = Box::leak(" ".repeat(64 * 1024 + 1).into_boxed_str());
    let mut request = HttpRequest::new(TestRequest::new().with_body(body).into());
    let Err(reply) = request.json::<Value>() else {
        panic!("oversized JSON accepted");
    };
    assert_eq!(
        reply.into_response().expect("response").status_code(),
        StatusCode(413)
    );
}
