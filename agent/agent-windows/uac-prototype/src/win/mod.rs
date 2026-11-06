//! The Win32 boundary of the UAC proof: the only module where unsafe code is allowed.
#![allow(unsafe_code)]

pub mod desktop;
pub mod inject;
pub mod launch;
pub mod screen;
pub mod service;
pub mod window;

use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Capture and absolute pointer input must use the same physical pixels.
pub fn dpi_aware() {
    // SAFETY: called before any window exists; failure only leaves the process DPI-virtualized.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}
