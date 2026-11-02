use std::net::SocketAddr;

use axum::{Json, Router, routing::get};
use serde::Serialize;

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

compile_error!("MIDAS CI rejection probe");
