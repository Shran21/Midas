//! MIDAS-018 proof: a browser sees and controls the real secure-desktop UAC prompt of a standard user.
//! Lab prototype for an isolated test machine and network only; see docs/meresek/uac-prototipus.md.

#[cfg(windows)]
mod app;
#[cfg_attr(not(windows), allow(dead_code))]
mod input;
#[cfg(windows)]
mod win;

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    return app::main();
    #[cfg(not(windows))]
    {
        eprintln!("midas-uac-prototype runs on Windows only");
        std::process::ExitCode::from(2)
    }
}
