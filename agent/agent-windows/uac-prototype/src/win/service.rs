use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{OnceLock, mpsc},
    time::Duration,
};

use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use super::{
    launch::{Child, active_console_session, spawn_in_session},
    window::ENDED_LOCALLY,
};
use crate::app::{Listen, log_file};

pub const NAME: &str = "MidasUacPrototype";

static LISTEN: OnceLock<Listen> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main);

pub fn run(listen: Listen) -> windows_service::Result<()> {
    let _ = LISTEN.set(listen);
    service_dispatcher::start(NAME, ffi_service_main)
}

/// Logs live next to the binary, which only administrators can write; never in a user-writable folder.
fn logs() -> std::io::Result<PathBuf> {
    let program = std::env::current_exe()?;
    Ok(program
        .parent()
        .ok_or(std::io::ErrorKind::NotFound)?
        .join("logs"))
}

fn service_main(_arguments: Vec<OsString>) {
    if let Ok(directory) = logs()
        && let Ok(file) = log_file(&directory, "service.log")
    {
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init();
    }
    if let Err(error) = supervise() {
        tracing::error!(%error, "service stopped with an error");
    }
}

fn status(state: ServiceState, accepted: ServiceControlAccept) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accepted,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }
}

fn spawn(session: u32) -> std::io::Result<Child> {
    let listen = LISTEN.get().ok_or(std::io::ErrorKind::InvalidInput)?;
    let arguments = format!(
        "session --address {} --port {} --logs \"{}\"",
        listen.address,
        listen.port,
        logs()?.display()
    );
    let child = spawn_in_session(session, &std::env::current_exe()?, &arguments)?;
    tracing::info!(session, "session helper started");
    Ok(child)
}

/// Keeps one helper in the active console session. A helper that exits, for example after the
/// local End, is not restarted: a new remote session needs a service restart and a new code.
fn supervise() -> Result<(), Box<dyn std::error::Error>> {
    let (stop, stopped) = mpsc::channel();
    let handle = service_control_handler::register(NAME, move |control| match control {
        ServiceControl::Stop => {
            let _ = stop.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    handle.set_service_status(status(ServiceState::Running, ServiceControlAccept::STOP))?;
    let mut child: Option<Child> = None;
    let mut finished = false;
    while stopped.recv_timeout(Duration::from_secs(1)).is_err() {
        if let Some(code) = child.as_ref().and_then(Child::exited) {
            if code == ENDED_LOCALLY as u32 {
                tracing::info!("session ended locally; restart the service for a new session");
            } else {
                tracing::warn!(code, "session helper exited");
            }
            child = None;
            finished = true;
        }
        if finished {
            continue;
        }
        let console = active_console_session();
        if child.as_ref().map(|c| c.session) != console {
            if let Some(previous) = child.take() {
                tracing::info!(session = previous.session, "console session changed");
                previous.terminate();
            }
            if let Some(session) = console {
                match spawn(session) {
                    Ok(started) => child = Some(started),
                    Err(error) => {
                        tracing::error!(%error, session, "cannot start the session helper");
                        finished = true;
                    }
                }
            }
        }
    }
    if let Some(running) = child.take() {
        running.terminate();
    }
    tracing::info!("service stopped");
    handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()))?;
    Ok(())
}
