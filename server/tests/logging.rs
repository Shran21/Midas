use axum::{body::Body, http::Request};
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn request_logs_exclude_client_supplied_secrets() {
    let log = Log::default();
    let writer = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    for path in ["/agent/enroll?token=private-query", "/private-path"] {
        midas_server::web_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", "private-header")
                    .body(Body::from("private-body"))
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    let output = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("private-"));
    let events: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 2);
    for (event, route, status) in [
        (&events[0], "/agent/enroll", 503),
        (&events[1], "unmatched", 404),
    ] {
        assert_eq!(event["fields"]["route"], route);
        assert_eq!(event["fields"]["status"], status);
        assert!(event["fields"]["duration_ms"].is_u64());
    }
}
