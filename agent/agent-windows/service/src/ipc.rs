use std::path::{Path, PathBuf};

pub const PIPE: &str = r"\\.\pipe\midas-agent";

/// Pipe access: network logons denied; SYSTEM and Administrators full; interactive users
/// may only connect, read and write. The per-client checks below still apply to everyone.
pub const PIPE_SDDL: &str = "D:P(D;;GA;;;NU)(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionRule {
    /// The session currently attached to the physical console.
    ActiveConsole,
    Exactly(u32),
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub session: SessionRule,
    /// The only executables allowed to read status: the agent itself and its tray program next to
    /// it, in a folder only administrators can write.
    pub images: Vec<PathBuf>,
}

/// The tray program's file name; it sits next to the agent executable.
pub const TRAY: &str = "midas-tray.exe";

impl Policy {
    pub fn for_program(session: SessionRule, program: PathBuf) -> Self {
        // Split by hand: both separators count on Windows, and the tests also run elsewhere.
        let text = program.to_string_lossy();
        let tray = match text.rfind(['\\', '/']) {
            Some(end) => PathBuf::from(format!("{}{TRAY}", &text[..=end])),
            None => PathBuf::from(TRAY),
        };
        Self {
            session,
            images: vec![program, tray],
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    OtherSession,
    ForeignProcess,
}

fn same_file(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
        text.strip_prefix(r"\\?\")
            .map(str::to_owned)
            .unwrap_or(text)
    };
    normalize(left) == normalize(right)
}

pub fn admitted(policy: &Policy, console: u32, session: u32, image: &Path) -> Result<(), Refusal> {
    let allowed = match policy.session {
        SessionRule::ActiveConsole => console,
        SessionRule::Exactly(session) => session,
    };
    if session != allowed || session == u32::MAX {
        return Err(Refusal::OtherSession);
    }
    if !policy
        .images
        .iter()
        .any(|allowed| same_file(image, allowed))
    {
        return Err(Refusal::ForeignProcess);
    }
    Ok(())
}

#[cfg(windows)]
pub use server::{query, serve};

#[cfg(windows)]
mod server {
    use std::{io, os::windows::io::AsRawHandle, time::Duration};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions},
        sync::watch,
    };

    use super::{PIPE_SDDL, Policy, admitted};
    use crate::{runner::State, win};

    fn create(name: &str, first: bool) -> io::Result<NamedPipeServer> {
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(16)
            .out_buffer_size(4096)
            .in_buffer_size(512);
        win::create_pipe(&options, name, PIPE_SDDL)
    }

    /// Serves the read-only status to admitted local clients until `shutdown`.
    pub async fn serve(
        name: String,
        policy: Policy,
        state: watch::Receiver<State>,
        mut shutdown: watch::Receiver<bool>,
    ) -> io::Result<()> {
        // Creating the first instance fails if another process already owns the name.
        let mut next = create(&name, true)?;
        loop {
            tokio::select! {
                connected = next.connect() => {
                    connected?;
                    let client = next;
                    next = create(&name, false)?;
                    let (policy, state) = (policy.clone(), state.clone());
                    tokio::spawn(async move { answer(client, &policy, &state).await });
                }
                _ = shutdown.wait_for(|stop| *stop) => return Ok(()),
            }
        }
    }

    async fn answer(mut client: NamedPipeServer, policy: &Policy, state: &watch::Receiver<State>) {
        let verdict = win::pipe_client(client.as_raw_handle()).map(|(session, image)| {
            (
                admitted(policy, win::console_session(), session, &image),
                session,
                image,
            )
        });
        match verdict {
            Ok((Ok(()), _, _)) => {
                let Ok(mut line) = serde_json::to_vec(&*state.borrow()) else {
                    return;
                };
                line.push(b'\n');
                // Closing (not disconnecting) the instance keeps the written line readable.
                let _ = tokio::time::timeout(Duration::from_secs(5), client.write_all(&line)).await;
                return;
            }
            Ok((Err(refusal), session, image)) => {
                tracing::warn!(?refusal, session, image = %image.display(), "status client refused");
            }
            Err(error) => tracing::warn!(%error, "status client could not be identified"),
        }
        let _ = client.disconnect();
    }

    /// Reads the status, accepting only a server that runs as a service (session 0).
    pub async fn query(name: &str) -> io::Result<String> {
        let mut pipe = ClientOptions::new().read(true).write(false).open(name)?;
        if win::pipe_server_session(pipe.as_raw_handle())? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the status pipe is not served by a Windows service",
            ));
        }
        let mut text = String::new();
        tokio::time::timeout(Duration::from_secs(5), pipe.read_to_string(&mut text))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no status received"))??;
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_installed_program_in_the_allowed_session_is_admitted() {
        let policy = Policy::for_program(
            SessionRule::ActiveConsole,
            PathBuf::from(r"C:\Program Files\MIDAS\agent\midas-agent.exe"),
        );
        let installed = Path::new(r"c:\program files\midas\agent\MIDAS-AGENT.EXE");
        assert_eq!(admitted(&policy, 2, 2, installed), Ok(()));
        assert_eq!(
            admitted(
                &policy,
                2,
                2,
                Path::new(r"\\?\C:\Program Files\MIDAS\agent\midas-agent.exe")
            ),
            Ok(())
        );
        assert_eq!(
            admitted(&policy, 2, 3, installed),
            Err(Refusal::OtherSession)
        );
        assert_eq!(
            admitted(&policy, u32::MAX, u32::MAX, installed),
            Err(Refusal::OtherSession)
        );
        assert_eq!(
            admitted(
                &policy,
                2,
                2,
                Path::new(r"C:\Users\x\Downloads\midas-agent.exe")
            ),
            Err(Refusal::ForeignProcess)
        );
        assert_eq!(
            admitted(
                &policy,
                2,
                2,
                Path::new(r"C:\Program Files\MIDAS\agent\midas-tray.exe")
            ),
            Ok(()),
            "the tray program next to the agent may read the status"
        );
        assert_eq!(
            admitted(
                &policy,
                2,
                2,
                Path::new(r"C:\Users\x\Downloads\midas-tray.exe")
            ),
            Err(Refusal::ForeignProcess)
        );
        let fixed = Policy {
            session: SessionRule::Exactly(5),
            ..policy
        };
        assert_eq!(admitted(&fixed, 2, 5, installed), Ok(()));
        assert_eq!(
            admitted(&fixed, 5, 2, installed),
            Err(Refusal::OtherSession)
        );
    }
}
