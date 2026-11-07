use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use midas_protocol::{MAX_BATCH_SAMPLES, MetricsBatch, Sample};
use prost::Message;
use tokio::sync::watch;

/// One day of minute samples survives an outage; older ones are dropped and counted.
pub const CAPACITY: usize = 24 * 60;
pub const INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct Buffer {
    samples: VecDeque<Sample>,
    capacity: usize,
    dropped: u64,
}

pub type Shared = Arc<Mutex<Buffer>>;

impl Buffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity.min(CAPACITY)),
            capacity: capacity.max(1),
            dropped: 0,
        }
    }

    pub fn shared(capacity: usize) -> Shared {
        Arc::new(Mutex::new(Self::new(capacity)))
    }

    pub fn push(&mut self, sample: Sample) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.samples.push_back(sample);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The oldest samples to send next, with the count of drops not yet reported.
    pub fn peek(&self) -> (Vec<Sample>, u64) {
        let batch = self
            .samples
            .iter()
            .take(MAX_BATCH_SAMPLES)
            .cloned()
            .collect();
        (batch, self.dropped)
    }

    /// Forgets samples the server stored and the drops it was told about.
    pub fn acknowledge(&mut self, count: usize, reported: u64) {
        for _ in 0..count.min(self.samples.len()) {
            self.samples.pop_front();
        }
        self.dropped = self.dropped.saturating_sub(reported);
    }
}

pub fn lock(buffer: &Shared) -> std::sync::MutexGuard<'_, Buffer> {
    buffer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Restores samples kept across an agent restart; an unreadable file is ignored.
pub fn load(path: &std::path::Path, buffer: &Shared) {
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let Ok(saved) = MetricsBatch::decode(bytes.as_slice()) else {
        tracing::warn!("saved metrics are unreadable and were discarded");
        return;
    };
    let mut buffer = lock(buffer);
    for sample in saved.samples {
        buffer.push(sample);
    }
    buffer.dropped = buffer.dropped.saturating_add(saved.dropped);
}

/// Persists the buffer through a temporary file so a crash never leaves half a file.
pub fn save(path: &std::path::Path, buffer: &Shared) -> std::io::Result<()> {
    let saved = {
        let buffer = lock(buffer);
        MetricsBatch {
            sequence: 0,
            samples: buffer.samples.iter().cloned().collect(),
            dropped: buffer.dropped,
        }
    };
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, saved.encode_to_vec())?;
    std::fs::rename(&temporary, path)
}

/// Reads CPU, memory, storage and network figures from the operating system.
pub struct Sampler {
    system: sysinfo::System,
    disks: sysinfo::Disks,
    networks: sysinfo::Networks,
}

impl Sampler {
    /// `None` when the operating system does not expose the figures.
    pub fn new() -> Option<Self> {
        let mut system = sysinfo::System::new().ok()?;
        system.refresh_cpu_usage();
        Some(Self {
            system,
            disks: sysinfo::Disks::new_with_refreshed_list().ok()?,
            networks: sysinfo::Networks::new_with_refreshed_list().ok()?,
        })
    }

    pub fn sample(&mut self, at: i64) -> Sample {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.disks.refresh(true);
        self.networks.refresh(true);
        let (disk_total, disk_free) = self.disks.iter().filter(|disk| !disk.is_removable()).fold(
            (0u64, 0u64),
            |(total, free), disk| {
                (
                    total.saturating_add(disk.total_space()),
                    free.saturating_add(disk.available_space()),
                )
            },
        );
        let (net_rx, net_tx) = self
            .networks
            .iter()
            .filter(|(name, _)| !is_loopback(name))
            .fold((0u64, 0u64), |(rx, tx), (_, data)| {
                (
                    rx.saturating_add(data.received()),
                    tx.saturating_add(data.transmitted()),
                )
            });
        Sample {
            at: at - at.rem_euclid(60),
            cpu_permille: (self.system.global_cpu_usage().clamp(0.0, 100.0) * 10.0).round() as u32,
            memory_used: self.system.used_memory(),
            memory_total: self.system.total_memory(),
            disk_used: disk_total.saturating_sub(disk_free),
            disk_total,
            net_rx,
            net_tx,
        }
    }
}

fn is_loopback(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_lowercase();
    name == "lo" || name.contains("loopback")
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// Samples once a minute into `buffer`, whether or not the agent is connected, and keeps
/// a copy in `store` so an outage survives an agent restart.
pub async fn collect(
    buffer: Shared,
    store: Option<std::path::PathBuf>,
    mut shutdown: watch::Receiver<bool>,
) {
    if let Some(store) = &store {
        load(store, &buffer);
    }
    let Some(mut sampler) = Sampler::new() else {
        tracing::error!("system metrics are unavailable; no samples are collected");
        return;
    };
    let mut tick = tokio::time::interval(INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let sample = sampler.sample(now());
                lock(&buffer).push(sample);
                if let Some(store) = &store
                    && let Err(error) = save(store, &buffer)
                {
                    tracing::warn!(%error, "metrics could not be saved locally");
                }
            }
            _ = shutdown.wait_for(|stop| *stop) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: i64) -> Sample {
        Sample {
            at,
            ..Sample::default()
        }
    }

    #[test]
    fn the_buffer_is_bounded_and_counts_drops() {
        let mut buffer = Buffer::new(3);
        for at in 0..5 {
            buffer.push(sample(at * 60));
        }
        let (batch, dropped) = buffer.peek();
        assert_eq!(
            batch.iter().map(|s| s.at).collect::<Vec<_>>(),
            [120, 180, 240]
        );
        assert_eq!(dropped, 2);
        buffer.acknowledge(2, dropped);
        assert_eq!(buffer.peek(), (vec![sample(240)], 0));
    }

    #[test]
    fn batches_are_limited_and_unacknowledged_samples_stay() {
        let mut buffer = Buffer::new(CAPACITY);
        for at in 0..130 {
            buffer.push(sample(at * 60));
        }
        let (first, _) = buffer.peek();
        assert_eq!(first.len(), MAX_BATCH_SAMPLES);
        assert_eq!(
            buffer.peek().0,
            first,
            "nothing is lost before an acknowledgement"
        );
        buffer.acknowledge(first.len(), 0);
        assert_eq!(buffer.len(), 70);
        assert_eq!(buffer.peek().0[0].at, 60 * 60);
    }

    #[test]
    fn the_buffer_survives_a_restart() {
        let path = std::env::temp_dir().join(format!("midas-metrics-{}.pb", std::process::id()));
        let buffer = Buffer::shared(5);
        for at in 0..7 {
            lock(&buffer).push(sample(at * 60));
        }
        save(&path, &buffer).unwrap();
        let restored = Buffer::shared(5);
        load(&path, &restored);
        assert_eq!(lock(&restored).peek(), lock(&buffer).peek());
        assert_eq!(lock(&restored).peek().1, 2);
        std::fs::remove_file(&path).unwrap();
        load(&path, &restored);
        assert_eq!(lock(&restored).len(), 5, "a missing file changes nothing");
    }

    #[test]
    fn samples_are_minute_aligned_and_sane() {
        let mut sampler = Sampler::new().unwrap();
        let taken = sampler.sample(1_700_000_059);
        assert_eq!(taken.at, 1_700_000_040);
        assert!(taken.cpu_permille <= 1000);
        assert!(taken.memory_total > 0 && taken.memory_used <= taken.memory_total);
        assert!(taken.disk_used <= taken.disk_total);
    }
}
