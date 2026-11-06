use std::{
    fs::{File, OpenOptions},
    net::IpAddr,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Mutex, atomic::AtomicBool},
    thread,
};

use clap::{Args, Parser, Subcommand};
use midas_media_prototype::{
    capture::Source,
    server::{self, Options},
    session::{Hooks, InputSink},
};

use crate::win;

const PAGE: &str = include_str!("page.html");

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run under the service manager as LocalSystem and start the session helper in the console session.
    Service(Listen),
    /// Capture and control the input desktop of the current session.
    Session {
        #[command(flatten)]
        listen: Listen,
        /// Log and measurement directory; set by the service. Without it the helper logs to the console.
        #[arg(long, hide = true)]
        logs: Option<PathBuf>,
    },
}

#[derive(Args, Clone)]
pub struct Listen {
    /// Address on the isolated test network; loopback by default.
    #[arg(long, default_value = "127.0.0.1")]
    pub address: IpAddr,
    #[arg(long, default_value_t = 7701)]
    pub port: u16,
}

pub fn log_file(directory: &Path, name: &str) -> std::io::Result<File> {
    std::fs::create_dir_all(directory)?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(name))
}

fn log_to(file: File) {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .init();
}

fn pairing_code() -> Result<String, getrandom::Error> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes)?;
    let n = u64::from_le_bytes(bytes) % 100_000_000;
    Ok(format!("{:04}-{:04}", n / 10_000, n % 10_000))
}

fn session(listen: Listen, logs: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    match &logs {
        Some(directory) => log_to(log_file(directory, "session.log")?),
        None => tracing_subscriber::fmt().init(),
    }
    win::dpi_aware();
    let code = pairing_code()?;
    if logs.is_none() {
        println!("pairing code: {code}");
    }
    let active = Arc::new(AtomicBool::new(false));
    let tracker = win::desktop::Tracker::start();
    {
        let (code, active) = (code.clone(), active.clone());
        thread::spawn(move || win::window::run(&code, &active));
    }
    let (capture, control) = (tracker.clone(), tracker);
    let hooks = Hooks {
        source: Arc::new(move || {
            Box::new(win::screen::Screen::new(capture.clone())) as Box<dyn Source>
        }),
        input: Some(Arc::new(move || {
            Box::new(win::inject::Injector::start(control.clone())) as Box<dyn InputSink>
        })),
    };
    tracing::info!(address = %listen.address, port = listen.port, "session helper started");
    tokio::runtime::Runtime::new()?.block_on(server::serve(Options {
        address: listen.address,
        port: listen.port,
        page: PAGE,
        measurements: logs.unwrap_or_else(|| PathBuf::from(".local/uac-prototype")),
        hooks,
        pairing: Some(code),
        active: Some(active),
    }))?;
    Ok(())
}

pub fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Service(listen) => win::service::run(listen).map_err(Into::into),
        Command::Session { listen, logs } => session(listen, logs),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "stopped");
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
