#![cfg(windows)]

use std::time::Duration;

use midas_agent_windows::{
    ipc::{Policy, SessionRule, query, serve},
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

fn handle_count() -> i64 {
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("(Get-Process -Id {}).HandleCount", std::process::id()),
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap()
}

/// The tray polls every five seconds; a refused poll must not keep its pipe handle open.
#[test]
fn refused_status_queries_leave_no_handles_behind() {
    let server_runtime = tokio::runtime::Runtime::new().unwrap();
    let server = server_runtime
        .block_on(async { start(SessionRule::Exactly(win::current_session().unwrap())) });
    let client = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    client.block_on(read(&server.name));
    let before = handle_count();
    for _ in 0..200 {
        // This test process is not a service, so the client refuses the server every time. A
        // query faster than the server's next instance finds the pipe busy and tries again.
        let refused = (0..100)
            .map(|_| client.block_on(query(&server.name)).unwrap_err())
            .find(|error| {
                let busy = error.kind() != std::io::ErrorKind::PermissionDenied;
                if busy {
                    std::thread::sleep(Duration::from_millis(10));
                }
                !busy
            });
        assert!(refused.is_some(), "the server never answered");
    }
    std::thread::sleep(Duration::from_millis(500));
    let grown = handle_count() - before;
    assert!(grown < 20, "{grown} handles kept after 200 refused queries");
    server_runtime.block_on(server.stop());
}
