//! Remote-support media: what both the encoder and the transport side need.

pub mod adapt;
pub mod codec;
pub mod frame;
pub mod link;
#[cfg(feature = "webrtc")]
pub mod peer;
#[cfg(feature = "webrtc")]
pub mod session;
#[cfg(all(test, feature = "webrtc"))]
mod testing;
#[cfg(feature = "webrtc")]
pub mod turn;
