mod inventory;

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand};
use midas_agent_core::host::{
    config::Paths,
    runner::{self, State},
};
use midas_protocol::enrollment::Platform;

const CONFIG: &str = "/etc/midas-agent/config.json";
const STATE: &str = "/var/lib/midas-agent";

#[derive(Parser)]
#[command(version, about = "MIDAS Linux agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Enrolls with a token read from standard input.
    Enroll {
        #[arg(long, default_value = CONFIG)]
        config: PathBuf,
        #[arg(long, default_value = STATE)]
        state: PathBuf,
    },
    /// Runs the agent in the foreground; systemd starts it this way.
    Run {
        #[arg(long, default_value = CONFIG)]
        config: PathBuf,
        #[arg(long, default_value = STATE)]
        state: PathBuf,
    },
}

fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
}

async fn terminated() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = terminate.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    match cli.command {
        Command::Enroll { config, state } => {
            let mut token = String::new();
            if std::io::stdin().read_line(&mut token).is_err() || token.trim().is_empty() {
                eprintln!("error: provide the enrollment token on standard input");
                return ExitCode::FAILURE;
            }
            let paths = Paths::with_config(state, config);
            match runtime.block_on(runner::enroll_with(&paths, Platform::Linux, &token)) {
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
        Command::Run { config, state } => {
            tracing_subscriber::fmt().with_ansi(false).init();
            let paths = Paths::with_config(state, config);
            let (stop, stopped) = tokio::sync::watch::channel(false);
            let (state, mut observed) = tokio::sync::watch::channel(State::Starting);
            runtime.spawn(async move {
                terminated().await;
                stop.send_replace(true);
            });
            runtime.spawn(async move {
                while observed.changed().await.is_ok() {
                    tracing::info!(state = ?*observed.borrow_and_update(), "agent state");
                }
            });
            runtime.block_on(runner::serve(
                paths,
                Platform::Linux,
                Some(Arc::new(inventory::collect)),
                state,
                stopped,
            ));
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_package_layout() {
        let cli = Cli::try_parse_from(["midas-agent", "run"]).unwrap();
        let Command::Run { config, state } = cli.command else {
            panic!("run was not parsed");
        };
        assert_eq!(
            (config, state),
            (PathBuf::from(CONFIG), PathBuf::from(STATE))
        );
    }

    #[test]
    fn the_unit_runs_the_installed_binary_with_least_privilege() {
        let unit = include_str!("../packaging/midas-agent.service");
        let start =
            format!("ExecStart=/usr/bin/midas-agent run --config {CONFIG} --state {STATE}\n");
        assert!(unit.contains(&start), "{unit}");
        for setting in [
            "User=midas-agent\n",
            "NoNewPrivileges=yes\n",
            "CapabilityBoundingSet=\n",
            "ProtectSystem=strict\n",
            "ProtectHome=yes\n",
            "RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX\n",
            "SystemCallFilter=~@privileged @resources\n",
            "ConditionPathExists=/etc/midas-agent/config.json\n",
        ] {
            assert!(unit.contains(setting), "missing {setting}");
        }
    }
}
