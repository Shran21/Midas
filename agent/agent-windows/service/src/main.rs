#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows::main()
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("midas-agent runs on Windows only");
    std::process::ExitCode::from(2)
}

#[cfg(windows)]
mod windows {
    use std::{path::PathBuf, process::ExitCode};

    use clap::{Parser, Subcommand, ValueEnum};
    use midas_agent_windows::{
        config::{Config, Paths},
        ipc::{self, SessionRule},
        run_agent, runner,
        win::service,
    };
    use midas_protocol::enrollment::{Kind, Platform};

    #[derive(Parser)]
    #[command(version, about = "MIDAS Windows agent")]
    struct Cli {
        #[command(subcommand)]
        command: Command,
    }

    #[derive(Clone, Copy, ValueEnum)]
    enum DeviceKind {
        Workstation,
        Server,
    }

    #[derive(Subcommand)]
    enum Command {
        /// Registers and starts the service (administrator, from %ProgramFiles%).
        Install {
            #[arg(long)]
            server: String,
            #[arg(long, default_value_t = 8444)]
            port: u16,
            #[arg(long, default_value_t = 443)]
            enrollment_port: u16,
            /// PEM file with the server certificate authority.
            #[arg(long)]
            server_ca: PathBuf,
            #[arg(long, value_enum, default_value = "workstation")]
            kind: DeviceKind,
            #[arg(long)]
            name: Option<String>,
        },
        /// Stops and removes the service; the data directory is kept.
        Uninstall,
        /// Enrolls with a token read from standard input.
        Enroll {
            #[arg(long, hide = true)]
            data: Option<PathBuf>,
        },
        /// Prints the agent status from the service.
        Status,
        /// Runs in the console for trials; status is served to this session only.
        Run {
            #[arg(long)]
            data: Option<PathBuf>,
            #[arg(long, default_value = ipc::PIPE)]
            pipe: String,
        },
        /// Entry point used by the service control manager.
        #[command(hide = true)]
        Service,
    }

    fn data(path: Option<PathBuf>) -> Paths {
        Paths::new(path.unwrap_or_else(service::data_dir))
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    }

    fn report(result: Result<String, String>) -> ExitCode {
        match result {
            Ok(message) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        }
    }

    pub fn main() -> ExitCode {
        let cli = Cli::parse();
        match cli.command {
            Command::Service => match service::dispatch() {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => report(Err(error.to_string())),
            },
            Command::Install {
                server,
                port,
                enrollment_port,
                server_ca,
                kind,
                name,
            } => report((|| {
                let config = Config {
                    server,
                    port,
                    enrollment_port,
                    server_ca: std::fs::read_to_string(&server_ca)
                        .map_err(|e| format!("cannot read {}: {e}", server_ca.display()))?,
                    kind: match kind {
                        DeviceKind::Workstation => Kind::Workstation,
                        DeviceKind::Server => Kind::Server,
                    },
                    name: name
                        .or_else(|| std::env::var("COMPUTERNAME").ok())
                        .ok_or("name is required")?,
                };
                service::install(&config)?;
                Ok("service installed and started".to_owned())
            })()),
            Command::Uninstall => report(service::uninstall().map(|()| "service removed".into())),
            Command::Enroll { data: path } => {
                let mut token = String::new();
                if std::io::stdin().read_line(&mut token).is_err() || token.trim().is_empty() {
                    return report(Err("provide the enrollment token on standard input".into()));
                }
                let paths = data(path);
                report(
                    runtime()
                        .block_on(runner::enroll_with(&paths, Platform::Windows, &token))
                        .map(str::to_owned),
                )
            }
            Command::Status => report(
                runtime()
                    .block_on(ipc::query(ipc::PIPE))
                    .map(|text| text.trim().to_owned())
                    .map_err(|e| e.to_string()),
            ),
            Command::Run { data: path, pipe } => {
                tracing_subscriber::fmt().init();
                let session = match midas_agent_windows::win::current_session() {
                    Ok(session) => session,
                    Err(error) => return report(Err(error.to_string())),
                };
                let (stop, stopped) = tokio::sync::watch::channel(false);
                let runtime = runtime();
                runtime.spawn(async move {
                    let _ = tokio::signal::ctrl_c().await;
                    stop.send_replace(true);
                });
                runtime.block_on(run_agent(
                    data(path),
                    stopped,
                    SessionRule::Exactly(session),
                    pipe,
                ));
                ExitCode::SUCCESS
            }
        }
    }
}
