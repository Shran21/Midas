//! Real desktop capture to a local browser over WebRTC; binds to loopback without authentication.

use std::{net::Ipv4Addr, path::PathBuf, sync::Arc};

use midas_media_prototype::{
    capture,
    server::{self, Options},
    session::Hooks,
};

const PAGE: &str = include_str!("page.html");

fn directory() -> PathBuf {
    std::env::var_os("MIDAS_MEASUREMENTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".local/media-prototype"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();
    let port: u16 = std::env::var("MIDAS_PROTOTYPE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7700);
    server::serve(Options {
        address: Ipv4Addr::LOCALHOST.into(),
        port,
        page: PAGE,
        measurements: directory(),
        hooks: Hooks {
            source: Arc::new(capture::open),
            input: None,
        },
        pairing: None,
        active: None,
    })
    .await?;
    Ok(())
}
