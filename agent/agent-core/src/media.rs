//! Remote-support media: what both the encoder and the transport side need.

pub mod adapt;
pub mod codec;
pub mod files;
pub mod frame;
pub mod link;
#[cfg(feature = "webrtc")]
pub mod peer;
#[cfg(feature = "webrtc")]
pub mod session;
#[cfg(all(test, feature = "webrtc"))]
mod testing;
pub mod transfer;
#[cfg(feature = "webrtc")]
pub mod turn;
