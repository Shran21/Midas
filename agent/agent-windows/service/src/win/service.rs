use std::{ffi::OsString, path::PathBuf, process::Command, time::Duration};

use tokio::sync::watch;
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
        ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};

use crate::{
    config::{Config, Paths, write_atomic},
    ipc::{PIPE, SessionRule},
};

pub const NAME: &str = "MidasAgent";

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into());
    PathBuf::from(base).join("MIDAS").join("agent")
}

define_windows_service!(ffi_service_main, service_main);

pub fn dispatch() -> windows_service::Result<()> {
    service_dispatcher::start(NAME, ffi_service_main)
}

fn status(state: ServiceState, accepted: ServiceControlAccept) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accepted,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    }
}

fn service_main(_arguments: Vec<OsString>) {
    let paths = Paths::new(data_dir());
    if std::fs::create_dir_all(paths.logs()).is_ok()
        && let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(paths.logs().join("agent.log"))
    {
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init();
    }
    let (stop, stopped) = watch::channel(false);
    let handler = move |event| match event {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stop.send_replace(true);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let Ok(control) = service_control_handler::register(NAME, handler) else {
        return;
    };
    let _ = control.set_service_status(status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    ));
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(crate::run_agent(
            paths,
            stopped,
            SessionRule::ActiveConsole,
            PIPE.into(),
        )),
        Err(error) => tracing::error!(%error, "runtime could not start"),
    }
    let _ =
        control.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()));
}

/// Restricts the data directory to SYSTEM and Administrators, removing inherited access.
fn protect(directory: &std::path::Path) -> Result<(), String> {
    let result = Command::new("icacls")
        .arg(directory)
        .args([
            "/inheritance:r",
            "/grant:r",
            "*S-1-5-18:(OI)(CI)F",
            "*S-1-5-32-544:(OI)(CI)F",
        ])
        .output()
        .map_err(|error| format!("icacls failed to start: {error}"))?;
    if !result.status.success() {
        return Err("icacls could not protect the data directory".into());
    }
    Ok(())
}

pub fn install(config: &Config) -> Result<(), String> {
    config.validate()?;
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    let program_files = std::env::var_os("ProgramFiles").ok_or("ProgramFiles is not set")?;
    // The status pipe trusts this exact file, so it must sit where only administrators write.
    if !program.to_string_lossy().to_lowercase().starts_with(
        &PathBuf::from(program_files)
            .to_string_lossy()
            .to_lowercase(),
    ) {
        return Err("install from a folder under %ProgramFiles%".into());
    }
    let paths = Paths::new(data_dir());
    std::fs::create_dir_all(&paths.root).map_err(|e| e.to_string())?;
    protect(&paths.root)?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    write_atomic(&paths.config(), &bytes).map_err(|e| e.to_string())?;
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| format!("service manager: {e}"))?;
    let info = ServiceInfo {
        name: NAME.into(),
        display_name: "MIDAS agent".into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: program,
        launch_arguments: vec!["service".into()],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let service = manager
        .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
        .map_err(|e| format!("create service: {e}"))?;
    let _ = service.set_description("MIDAS monitoring and remote support agent");
    let restart = |seconds| ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(seconds),
    };
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86_400)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![restart(5), restart(30), restart(60)]),
        })
        .map_err(|e| format!("failure actions: {e}"))?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(|e| format!("failure actions: {e}"))?;
    service
        .start::<&str>(&[])
        .map_err(|e| format!("start service: {e}"))?;
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("service manager: {e}"))?;
    let service = manager
        .open_service(
            NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        )
        .map_err(|e| format!("open service: {e}"))?;
    if service
        .query_status()
        .is_ok_and(|status| status.current_state != ServiceState::Stopped)
    {
        let _ = service.stop();
    }
    service.delete().map_err(|e| format!("delete service: {e}"))
}
