use std::net::SocketAddr;

use axum::{Json, Router, routing::get};
use serde::Serialize;

pub mod config;
pub mod transport;

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

pub fn router() -> Router {
    Router::new().route(
        "/health/live",
        get(|| async { Json(Health { status: "ok" }) }),
    )
}

pub fn validate_development_address(address: SocketAddr) -> Result<SocketAddr, &'static str> {
    if !address.ip().is_loopback() {
        return Err("the development server must bind to a loopback address");
    }
    Ok(address)
}

pub fn web_router() -> Router {
    router()
        .route("/agent/enroll", axum::routing::post(unavailable))
        .layer(axum::middleware::from_fn(observe))
}

pub fn agent_router() -> Router {
    Router::new()
        .route("/agent/connect", get(unavailable))
        .layer(axum::middleware::from_fn(observe))
}

async fn unavailable() -> axum::http::StatusCode {
    axum::http::StatusCode::SERVICE_UNAVAILABLE
}

async fn observe(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let started = std::time::Instant::now();
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map_or("unmatched", axum::extract::MatchedPath::as_str)
        .to_owned();
    let response = next.run(request).await;
    tracing::info!(
        route,
        status = response.status().as_u16(),
        duration_ms = started.elapsed().as_millis() as u64,
        "request completed"
    );
    response
}
