use std::{
    net::{IpAddr, SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Instant,
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
};
use serde_json::{Value, json};
use str0m::{Candidate, RtcConfig, change::SdpOffer};

use crate::{
    network,
    session::{self, Hooks, Session},
};

/// Failed pairing attempts after which the process refuses every further offer.
const PAIRING_ATTEMPTS: u32 = 5;

pub struct Options {
    pub address: IpAddr,
    pub port: u16,
    pub page: &'static str,
    pub measurements: PathBuf,
    pub hooks: Hooks,
    /// When set, `/offer` requires this code in the `x-midas-pairing` header.
    pub pairing: Option<String>,
    /// When set, only one session runs at a time and the flag shows whether one is active.
    pub active: Option<Arc<AtomicBool>>,
}

struct Shared {
    options: Options,
    failures: AtomicU32,
}

fn same(expected: &str, given: &str) -> bool {
    expected.len() == given.len()
        && expected
            .bytes()
            .zip(given.bytes())
            .fold(0, |acc, (a, b)| acc | (a ^ b))
            == 0
}

async fn offer(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Json(offer): Json<SdpOffer>,
) -> Result<Json<Value>, StatusCode> {
    let options = &shared.options;
    if let Some(code) = &options.pairing {
        if shared.failures.load(Ordering::Acquire) >= PAIRING_ATTEMPTS {
            return Err(StatusCode::FORBIDDEN);
        }
        let given = headers
            .get("x-midas-pairing")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if !same(code, given) {
            shared.failures.fetch_add(1, Ordering::AcqRel);
            tracing::warn!("rejected offer with a wrong pairing code");
            return Err(StatusCode::FORBIDDEN);
        }
    }
    if let Some(active) = &options.active
        && active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return Err(StatusCode::CONFLICT);
    }
    let started = start(options, offer);
    if started.is_err()
        && let Some(active) = &options.active
    {
        active.store(false, Ordering::Release);
    }
    started.map(Json)
}

fn start(options: &Options, offer: SdpOffer) -> Result<Value, StatusCode> {
    let socket = UdpSocket::bind(SocketAddr::new(options.address, 0))
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
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
    let session = Session::new(
        rtc,
        socket,
        options.measurements.clone(),
        options.hooks.clone(),
    )
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let active = options.active.clone();
    std::thread::spawn(move || {
        session.run();
        if let Some(active) = active {
            active.store(false, Ordering::Release);
        }
    });
    Ok(json!({
        "answer": answer,
        "ladder": session::ladder_table(),
        "profiles": network::PROFILES,
    }))
}

pub async fn serve(options: Options) -> std::io::Result<()> {
    str0m::crypto::from_feature_flags().install_process_default();
    let (address, page) = (SocketAddr::new(options.address, options.port), options.page);
    let app = Router::new()
        .route("/", get(move || async move { Html(page) }))
        .route("/offer", post(offer))
        .with_state(Arc::new(Shared {
            options,
            failures: AtomicU32::new(0),
        }));
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("open http://{address}");
    axum::serve(listener, app).await
}

#[cfg(test)]
mod tests {
    use super::same;

    #[test]
    fn pairing_codes_compare_whole_values() {
        assert!(same("1234-5678", "1234-5678"));
        assert!(!same("1234-5678", "1234-5679"));
        assert!(!same("1234-5678", "1234-567"));
        assert!(!same("1234-5678", ""));
    }
}
