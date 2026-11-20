use std::{
    io,
    os::windows::io::AsRawHandle,
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite},
    time::Instant,
};

#[cfg(test)]
use super::IO_TIMEOUT;
use super::{Connection, STARTUP_TIMEOUT, bootstrap::delivery, denied};
use crate::consent::{Answer, Question, encode_answer};

pub(in crate::win::remote_ipc) const WORKER_HELLO: &[u8] = b"worker-ready/1";
pub(in crate::win::remote_ipc) const BROKER_HELLO: &[u8] = b"broker-ready/1";
/// The companion's greeting to the privileged worker of a session with UAC rights.
pub(in crate::win::remote_ipc) const MEDIA_HELLO: &[u8] = b"media-ready/1";
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

pub enum Participant {
    Worker,
    Broker,
    Privileged,
    /// The consent window in the signed-in user's session.
    Consent,
}

/// The last startup step a participant reached. Participants start without inherited handles, so
/// a failure exit code of `0x20 + step` is how the launcher learns where startup stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Started = 1,
    Runtime = 2,
    Identity = 3,
    Locator = 4,
    Bootstrap = 5,
    Record = 6,
    Prepared = 7,
    Acknowledged = 8,
    Connected = 9,
    Standby = 10,
}

static STAGE: AtomicU8 = AtomicU8::new(0);

pub(in crate::win::remote_ipc) fn reached(stage: Stage) {
    STAGE.store(stage as u8, Ordering::Release);
}

pub fn failure_code() -> u8 {
    0x20 + STAGE.load(Ordering::Acquire)
}

impl Participant {
    pub fn run(self) -> io::Result<()> {
        reached(Stage::Started);
        let image = std::env::current_exe()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()?;
        reached(Stage::Runtime);
        runtime.block_on(async {
            let deadline = Instant::now() + STARTUP_TIMEOUT;
            match self {
                Self::Worker => {
                    let connection =
                        delivery::receive_worker(&image, deadline, async |destination| {
                            let mut connection = destination.connect()?;
                            connection.send(WORKER_HELLO, deadline).await?;
                            expect(&mut connection, BROKER_HELLO, deadline).await?;
                            Ok(connection)
                        })
                        .await?;
                    reached(Stage::Connected);
                    reached(Stage::Standby);
                    super::media::worker(connection).await
                }
                Self::Privileged => {
                    let (connection, media) =
                        delivery::receive_privileged(&image, deadline, async |destination| {
                            let mut connection = destination.connect()?;
                            connection.send(WORKER_HELLO, deadline).await?;
                            let hello = connection.receive(deadline).await?;
                            let media = match &*hello {
                                BROKER_HELLO => false,
                                MEDIA_HELLO => true,
                                _ => return Err(denied()),
                            };
                            Ok((connection, media))
                        })
                        .await?;
                    reached(Stage::Connected);
                    reached(Stage::Standby);
                    // The service itself stands by; the companion of a session with UAC rights
                    // runs the secure desktop and elevated input through this worker.
                    if media {
                        super::media::privileged(connection).await
                    } else {
                        standby(connection).await
                    }
                }
                Self::Consent => consent(deadline, crate::win::consent::ask).await,
                Self::Broker => {
                    let (listener, launcher) =
                        delivery::receive_companion(&image, deadline).await?;
                    let mut connection = listener.accept(deadline).await?;
                    expect(&mut connection, WORKER_HELLO, deadline).await?;
                    connection.send(BROKER_HELLO, deadline).await?;
                    let control = launcher.connected(deadline).await?;
                    reached(Stage::Connected);
                    reached(Stage::Standby);
                    super::media::companion(connection, control, &image).await
                }
            }
        })
    }
}

/// Shows only the question the service delivers, and returns the answer to the service only.
pub(in crate::win::remote_ipc) async fn consent(
    deadline: Instant,
    ask: fn(&Question) -> io::Result<Answer>,
) -> io::Result<()> {
    let mut service = delivery::receive_consent(deadline).await?;
    reached(Stage::Connected);
    let question = super::bounded(deadline, service.next())
        .await?
        .and_then(|message| Question::decode(&message))
        .ok_or_else(denied)?;
    reached(Stage::Standby);
    let request = question.request;
    let answer = tokio::task::spawn_blocking(move || ask(&question))
        .await
        .map_err(|_| denied())??;
    service
        .reply(
            &encode_answer(request, answer),
            Instant::now() + ANSWER_TIMEOUT,
        )
        .await?;
    // No further request exists; the window waits for the service to close the channel.
    match service.next().await? {
        None => Ok(()),
        Some(_) => Err(denied()),
    }
}

pub(in crate::win::remote_ipc) async fn expect<S: AsyncRead + AsyncWrite + AsRawHandle + Unpin>(
    connection: &mut Connection<S>,
    expected: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    if &*connection.receive(deadline).await? != expected {
        return Err(denied());
    }
    Ok(())
}

// No operation has been authorized. Await EOF without a heartbeat and refuse any payload.
async fn standby<S: AsyncRead + Unpin>(connection: Connection<S>) -> io::Result<()> {
    let mut pipe = connection.pipe.ok_or_else(denied)?;
    match pipe.read(&mut [0u8; 1]).await {
        Ok(0) => Ok(()),
        Ok(_) => Err(denied()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::NotConnected
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::pair;
    use super::*;

    #[tokio::test]
    async fn reflected_greeting_is_refused() {
        let (mut broker, mut worker) = pair().await;
        let deadline = Instant::now() + IO_TIMEOUT;
        worker.send(BROKER_HELLO, deadline).await.unwrap();
        assert_eq!(
            expect(&mut broker, WORKER_HELLO, deadline)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[tokio::test]
    async fn standby_exits_on_disconnect_and_rejects_unsolicited_data() {
        let (broker, worker) = pair().await;
        drop(worker);
        super::super::bounded(Instant::now() + IO_TIMEOUT, standby(broker))
            .await
            .unwrap();
        let (broker, mut worker) = pair().await;
        let deadline = Instant::now() + IO_TIMEOUT;
        worker.send(WORKER_HELLO, deadline).await.unwrap();
        assert_eq!(
            super::super::bounded(deadline, standby(broker))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
