#![cfg(windows)]

use std::time::Duration;

use midas_agent_windows::{
    ipc::{Policy, SessionRule, serve},
    runner::State,
    win,
};
use tokio::{io::AsyncReadExt, net::windows::named_pipe::ClientOptions, sync::watch};

async fn read(name: &str) -> String {
    for _ in 0..100 {
        match ClientOptions::new().write(false).open(name) {
            Ok(mut pipe) => {
                let mut text = String::new();
                let _ =
                    tokio::time::timeout(Duration::from_secs(5), pipe.read_to_string(&mut text))
                        .await
                        .expect("server answered or closed");
                return text;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
    panic!("pipe {name} never became available");
}

struct Server {
    name: String,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

fn start(session: SessionRule) -> Server {
    let name = format!(r"\\.\pipe\midas-test-{}", uuid::Uuid::new_v4());
    let policy = Policy {
        session,
        images: vec![std::env::current_exe().unwrap()],
    };
    let (_, state) = watch::channel(State::Connected { protocol: 1 });
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(serve(name.clone(), policy, state, stopped));
    Server { name, stop, task }
}

impl Server {
    async fn stop(self) {
        self.stop.send_replace(true);
        self.task.await.unwrap().unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_allowed_program_in_the_allowed_session_reads_status() {
    let server = start(SessionRule::Exactly(win::current_session().unwrap()));
    for _ in 0..3 {
        assert_eq!(
            read(&server.name).await.trim(),
            r#"{"state":"connected","protocol":1}"#
        );
    }
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_foreign_program_in_the_same_session_is_refused() {
    let server = start(SessionRule::Exactly(win::current_session().unwrap()));
    read(&server.name).await;
    let short = server.name.trim_start_matches(r"\\.\pipe\");
    let script = format!(
        "$p = New-Object System.IO.Pipes.NamedPipeClientStream('.', '{short}', [System.IO.Pipes.PipeDirection]::In); \
         $p.Connect(5000); $r = New-Object System.IO.StreamReader($p); Write-Output ('got:' + $r.ReadToEnd())"
    );
    let output = tokio::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("got:"),
        "client did not connect: {stdout} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !stdout.contains("state"),
        "foreign program read the status: {stdout}"
    );
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn another_session_is_refused() {
    let other = win::current_session().unwrap().wrapping_add(1);
    let server = start(SessionRule::Exactly(other));
    assert_eq!(read(&server.name).await, "");
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_owner_of_the_pipe_name_is_refused() {
    let server = start(SessionRule::Exactly(win::current_session().unwrap()));
    read(&server.name).await;
    let (_, state) = watch::channel(State::Starting);
    let (_stop, stopped) = watch::channel(false);
    let squatter = serve(
        server.name.clone(),
        Policy {
            session: SessionRule::Exactly(0),
            images: vec![std::env::current_exe().unwrap()],
        },
        state,
        stopped,
    )
    .await;
    assert!(
        squatter.is_err(),
        "the name must not be shared with a second server"
    );
    server.stop().await;
}
