//! MIDAS-017 early WebRTC and encoder proof: real desktop capture to a local browser over WebRTC.
//! Development prototype only: it binds to loopback and has no authentication, consent or audit.

mod adapt;
mod capture;
mod encode;
mod network;
mod session;

use std::{net::UdpSocket, path::PathBuf, time::Instant};

use axum::{
    Json, Router,
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use str0m::{Candidate, RtcConfig, change::SdpOffer};

const PAGE: &str = include_str!("page.html");

fn directory() -> PathBuf {
    std::env::var_os("MIDAS_MEASUREMENTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".local/media-prototype"))
}

async fn offer(Json(offer): Json<SdpOffer>) -> Result<Json<serde_json::Value>, StatusCode> {
    let socket = UdpSocket::bind("127.0.0.1:0").map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let address = socket
        .local_addr()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut rtc = RtcConfig::new()
        .clear_codecs()
        .enable_h264(true)
        .enable_bwe(Some(str0m::bwe::Bitrate::kbps(2_500)))
        .build(Instant::now());
    let candidate = Candidate::host(address, "udp").map_err(|_| StatusCode::BAD_REQUEST)?;
    rtc.add_local_candidate(candidate)
        .ok_or(StatusCode::BAD_REQUEST)?;
    let answer = rtc
        .sdp_api()
        .accept_offer(offer)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let session = session::Session::new(rtc, socket, directory())
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    std::thread::spawn(move || session.run());
    Ok(Json(serde_json::json!({
        "answer": answer,
        "ladder": session::ladder_table(),
        "profiles": network::PROFILES,
    })))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();
    str0m::crypto::from_feature_flags().install_process_default();
    let port: u16 = std::env::var("MIDAS_PROTOTYPE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7700);
    let app = Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/offer", post(offer));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("open http://127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
