//! The user's tray icon for the MIDAS agent; it runs with the signed-in user's rights.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(u8::try_from(midas_agent_windows::win::tray::run()).unwrap_or(1))
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("midas-tray runs on Windows only");
    std::process::ExitCode::from(2)
}
