use std::{
    io::{BufRead, Write},
    os::windows::{io::AsHandle, process::CommandExt},
    process::{Child, Command, Stdio},
};

use super::*;
use identity::tests::{local, peer, process};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

async fn pair() -> (Connection<NamedPipeServer>, Connection<NamedPipeClient>) {
    let listener = Listener::create(local()).unwrap();
    let client = open(listener.name(), local()).unwrap();
    (listener.accept(deadline()).await.unwrap(), client)
}

#[tokio::test]
async fn control_frames_roundtrip_and_timeout_closes_the_channel() {
    let (mut server, mut client) = pair().await;
    client.send(b"prepare", deadline()).await.unwrap();
    let frame = server.receive(deadline()).await.unwrap();
    assert_eq!(format!("{frame:?}"), "Frame { .. }");
    assert_eq!(&*frame, b"prepare");
    server.send(b"observed", deadline()).await.unwrap();
    assert_eq!(&*client.receive(deadline()).await.unwrap(), b"observed");
    assert_eq!(
        server
            .receive(Instant::now() + Duration::from_millis(25))
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    assert!(server.send(b"late", deadline()).await.is_err());
    assert!(client.receive(deadline()).await.is_err());
}

#[tokio::test]
async fn cancellation_closes_incomplete_frames_and_refuses_reuse() {
    let (mut server, mut client) = pair().await;
    client
        .pipe
        .as_mut()
        .unwrap()
        .write_all(b"MR")
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), server.receive(deadline()))
            .await
            .is_err()
    );
    assert!(server.receive(deadline()).await.is_err());
    assert!(client.receive(deadline()).await.is_err());
}

#[tokio::test]
async fn invalid_or_oversized_frames_never_reach_the_caller() {
    for header in [*b"BAD!\x01\0\0\0", *b"MRC1\0\0\0\0", *b"MRC1\x01\0\x01\0"] {
        let (mut server, mut client) = pair().await;
        client
            .pipe
            .as_mut()
            .unwrap()
            .write_all(&header)
            .await
            .unwrap();
        assert!(server.receive(deadline()).await.is_err());
        assert!(server.send(b"late", deadline()).await.is_err());
    }
    let (mut server, mut client) = pair().await;
    assert!(
        client
            .send(&vec![1; MAX_FRAME + 1], deadline())
            .await
            .is_err()
    );
    assert!(server.receive(deadline()).await.is_err());
}

#[tokio::test]
async fn endpoints_refuse_role_confusion_remote_names_and_another_owner() {
    assert!(Listener::bind(&local(), local()).is_err());
    let listener = Listener::create(local()).unwrap();
    assert!(connect(listener.name(), local()).is_err());
    assert!(open(r"\\host\pipe\midas-remote-invalid", local()).is_err());
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .max_instances(1);
    assert!(super::super::create_pipe(&options, listener.name(), &local().acl().unwrap()).is_err());
}

struct TestChild(Child);

impl TestChild {
    fn spawn() -> Self {
        Self(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "win::remote_ipc::tests::child_endpoint",
                    "--nocapture",
                ])
                .env("MIDAS_REMOTE_IPC_CHILD", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .creation_flags(0x0800_0000)
                .spawn()
                .unwrap(),
        )
    }

    fn start(&mut self, name: &str, expected: u32) {
        writeln!(self.0.stdin.as_mut().unwrap(), "{name}\n{expected}").unwrap();
    }

    async fn finish(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(status) = self.0.try_wait().unwrap() {
                    assert!(status.success());
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn child_endpoint() {
    if std::env::var_os("MIDAS_REMOTE_IPC_CHILD").is_none() {
        return;
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let name = lines.next().unwrap().unwrap();
    let pid = lines.next().unwrap().unwrap().parse().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let Ok(mut client) = open(&name, peer(process(pid))) else {
            return;
        };
        while let Ok(frame) = client.receive(deadline()).await {
            if client.send(&frame, deadline()).await.is_err() {
                break;
            }
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn separate_process_exchange_is_bound_to_the_spawned_child() {
    let mut child = TestChild::spawn();
    let pinned = peer(child.0.as_handle().try_clone_to_owned().unwrap());
    let listener = Listener::create(pinned).unwrap();
    child.start(listener.name(), std::process::id());
    let mut connection = listener.accept(deadline()).await.unwrap();
    connection
        .send(b"target-session", deadline())
        .await
        .unwrap();
    assert_eq!(
        &*connection.receive(deadline()).await.unwrap(),
        b"target-session"
    );
    drop(connection);
    child.finish().await;
}

#[tokio::test]
async fn another_process_with_the_same_image_and_user_cannot_replace_the_expected_child() {
    let child = TestChild::spawn();
    let pinned = peer(child.0.as_handle().try_clone_to_owned().unwrap());
    let listener = Listener::create(pinned).unwrap();
    let _wrong_client = open(listener.name(), local()).unwrap();
    assert!(listener.accept(deadline()).await.is_err());
}

#[tokio::test]
async fn wrong_server_identity_and_process_exit_are_refused() {
    let mut child = TestChild::spawn();
    let listener = Listener::create(local()).unwrap();
    assert!(
        open(
            listener.name(),
            peer(child.0.as_handle().try_clone_to_owned().unwrap())
        )
        .is_err()
    );
    let pinned = peer(child.0.as_handle().try_clone_to_owned().unwrap());
    let pid = child.0.id();
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(pinned.check(pid).is_err());
}
