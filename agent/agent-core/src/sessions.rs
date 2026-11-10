use std::sync::Arc;

use midas_protocol::sessions::{OsSession, SessionReport, SessionRequest, valid_sessions};
use tokio::sync::{Semaphore, mpsc};

pub type Collector = Arc<dyn Fn() -> Option<Vec<OsSession>> + Send + Sync>;

#[derive(Clone)]
pub struct Probe {
    collect: Collector,
    slot: Arc<Semaphore>,
}

impl Probe {
    pub fn new(collect: Collector) -> Self {
        Self {
            collect,
            slot: Arc::new(Semaphore::new(1)),
        }
    }

    pub fn request(&self, request: SessionRequest, reply: mpsc::Sender<SessionReport>) {
        let Ok(permit) = self.slot.clone().try_acquire_owned() else {
            let _ = reply.try_send(SessionReport {
                request_id: request.request_id,
                connection_id: request.connection_id,
                available: false,
                sessions: Vec::new(),
            });
            return;
        };
        let collect = self.collect.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let sessions = collect().filter(|list| valid_sessions(list));
            let report = SessionReport {
                request_id: request.request_id,
                connection_id: request.connection_id,
                available: sessions.is_some(),
                sessions: sessions.unwrap_or_default(),
            };
            let _ = reply.try_send(report);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Mutex, time::Duration};

    use super::*;

    fn request() -> SessionRequest {
        SessionRequest {
            request_id: uuid::Uuid::new_v4().as_bytes().to_vec(),
            connection_id: uuid::Uuid::new_v4().as_bytes().to_vec(),
        }
    }

    #[tokio::test]
    async fn reconnects_cannot_start_a_second_native_collection() {
        let started = Arc::new(tokio::sync::Notify::new());
        let (release, wait) = std::sync::mpsc::channel();
        let wait = Mutex::new(wait);
        let signal = started.clone();
        let probe = Probe::new(Arc::new(move || {
            signal.notify_one();
            wait.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Some(Vec::new())
        }));
        let (reply, mut result) = mpsc::channel(1);
        let original = request();
        probe.request(original.clone(), reply);
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        let (reply, mut busy) = mpsc::channel(1);
        let reconnected = request();
        probe.clone().request(reconnected.clone(), reply);
        let report = busy.recv().await.unwrap();
        assert!(!report.available);
        assert_eq!(report.connection_id, reconnected.connection_id);
        assert_eq!(report.request_id, reconnected.request_id);
        release.send(()).unwrap();
        let report = tokio::time::timeout(Duration::from_secs(5), result.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(report.available);
        assert_eq!(report.connection_id, original.connection_id);
        assert_eq!(report.request_id, original.request_id);
    }

    #[tokio::test]
    async fn invalid_or_failed_collection_never_becomes_an_available_empty_list() {
        for sessions in [None, Some(vec![OsSession::default()])] {
            let probe = Probe::new(Arc::new(move || sessions.clone()));
            let (reply, mut result) = mpsc::channel(1);
            probe.request(request(), reply);
            let report = tokio::time::timeout(Duration::from_secs(5), result.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(!report.available);
            assert!(report.sessions.is_empty());
        }
    }
}
