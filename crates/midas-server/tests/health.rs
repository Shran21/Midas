use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use midas_server::{router, validate_development_address};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn liveness_returns_only_service_status() {
    let response = router()
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value, json!({ "status": "ok" }));
}

#[tokio::test]
async fn unknown_operations_are_not_available() {
    for uri in ["/api/devices", "/agent/enroll", "/health/ready"] {
        let response = router()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn liveness_does_not_accept_writes() {
    let response = router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/health/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[test]
fn development_listener_rejects_public_and_wildcard_addresses() {
    for address in [
        "0.0.0.0:7443",
        "[::]:7443",
        "192.168.1.2:7443",
        "[::ffff:192.168.1.2]:7443",
    ] {
        assert!(validate_development_address(address.parse().unwrap()).is_err());
    }
}

#[test]
fn development_listener_accepts_ipv4_and_ipv6_loopback() {
    for address in ["127.0.0.1:7443", "[::1]:7443"] {
        assert!(validate_development_address(address.parse().unwrap()).is_ok());
    }
}
