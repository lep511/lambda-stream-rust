use http::{HeaderMap, StatusCode, header::HeaderValue};
use stream_rust::{error_response, streaming_response};

// ─── error_response ─────────────────────────────────────────────────────────

#[test]
fn error_response_400_status() {
    let resp = error_response(StatusCode::BAD_REQUEST, "bad input");
    assert_eq!(resp.metadata_prelude.status_code, StatusCode::BAD_REQUEST);
}

#[test]
fn error_response_500_status() {
    let resp = error_response(StatusCode::INTERNAL_SERVER_ERROR, "boom");
    assert_eq!(
        resp.metadata_prelude.status_code,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[test]
fn error_response_has_json_content_type() {
    let resp = error_response(StatusCode::BAD_REQUEST, "test");
    let ct = resp
        .metadata_prelude
        .headers
        .get("content-type")
        .expect("content-type header should exist");
    assert_eq!(ct, "application/json");
}

#[test]
fn error_response_empty_cookies() {
    let resp = error_response(StatusCode::BAD_REQUEST, "test");
    assert!(resp.metadata_prelude.cookies.is_empty());
}

#[test]
fn error_response_not_found() {
    let resp = error_response(StatusCode::NOT_FOUND, "not found");
    assert_eq!(resp.metadata_prelude.status_code, StatusCode::NOT_FOUND);
}

#[test]
fn error_response_unauthorized() {
    let resp = error_response(StatusCode::UNAUTHORIZED, "no auth");
    assert_eq!(resp.metadata_prelude.status_code, StatusCode::UNAUTHORIZED);
}

#[test]
fn error_response_service_unavailable() {
    let resp = error_response(StatusCode::SERVICE_UNAVAILABLE, "down");
    assert_eq!(
        resp.metadata_prelude.status_code,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

// ─── streaming_response ─────────────────────────────────────────────────────

#[test]
fn streaming_response_preserves_status() {
    let headers = HeaderMap::new();
    let body = lambda_runtime::streaming::Body::empty();
    let resp = streaming_response(StatusCode::OK, headers, body);

    assert_eq!(resp.metadata_prelude.status_code, StatusCode::OK);
}

#[test]
fn streaming_response_preserves_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert("x-custom", HeaderValue::from_static("value"));

    let body = lambda_runtime::streaming::Body::empty();
    let resp = streaming_response(StatusCode::OK, headers, body);

    assert_eq!(
        resp.metadata_prelude.headers.get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        resp.metadata_prelude.headers.get("x-custom").unwrap(),
        "value"
    );
}

#[test]
fn streaming_response_empty_headers() {
    let headers = HeaderMap::new();
    let body = lambda_runtime::streaming::Body::empty();
    let resp = streaming_response(StatusCode::OK, headers, body);

    assert!(resp.metadata_prelude.headers.is_empty());
    assert!(resp.metadata_prelude.cookies.is_empty());
}

#[test]
fn streaming_response_multiple_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("cache-control", HeaderValue::from_static("no-cache"));
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));

    let body = lambda_runtime::streaming::Body::empty();
    let resp = streaming_response(StatusCode::OK, headers, body);

    assert_eq!(resp.metadata_prelude.headers.len(), 3);
}

#[test]
fn streaming_response_cors_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("POST, OPTIONS"),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static("Content-Type"),
    );

    let body = lambda_runtime::streaming::Body::empty();
    let resp = streaming_response(StatusCode::OK, headers, body);

    assert_eq!(
        resp.metadata_prelude
            .headers
            .get("access-control-allow-origin")
            .unwrap(),
        "*"
    );
    assert_eq!(
        resp.metadata_prelude
            .headers
            .get("access-control-allow-methods")
            .unwrap(),
        "POST, OPTIONS"
    );
}
