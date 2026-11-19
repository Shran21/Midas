use std::{io, os::windows::io::AsRawHandle};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite},
    time::Instant,
};

#[cfg(test)]
use super::IO_TIMEOUT;
use super::{Connection, STARTUP_TIMEOUT, bootstrap::delivery, denied};

const WORKER_HELLO: &[u8] = b"worker-ready/1";
const BROKER_HELLO: &[u8] = b"broker-ready/1";

pub enum Participant {
    Worker,
    Broker,
}

impl Participant {
    pub fn run(self) -> io::Result<()> {
        let image = std::env::current_exe()?;
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()?
            .block_on(async {
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
                        standby(connection).await
                    }
                    Self::Broker => {
                        let (listener, launcher) =
                            delivery::receive_companion(&image, deadline).await?;
                        let mut connection = listener.accept(deadline).await?;
                        expect(&mut connection, WORKER_HELLO, deadline).await?;
                        connection.send(BROKER_HELLO, deadline).await?;
                        launcher.connected(deadline).await?;
                        standby(connection).await
                    }
                }
            })
    }
}

async fn expect<S: AsyncRead + AsyncWrite + AsRawHandle + Unpin>(
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
