use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use midas_protocol::{
    InventoryItem, InventoryKey, InventoryUpdate, MAX_INVENTORY_ITEMS, MAX_INVENTORY_TEXT,
    MAX_MESSAGE_BYTES,
};
use prost::Message;
use tokio::sync::{Notify, watch};

pub type Snapshot = BTreeMap<InventoryKey, String>;
/// Platform-specific collection; it may block, so it runs on a blocking thread.
pub type Collector = Arc<dyn Fn() -> Snapshot + Send + Sync>;

pub const FULL_EVERY_SECONDS: i64 = 24 * 3600;
const GENERATION_MASK: u64 = i64::MAX as u64;
pub const CHECK_EVERY: Duration = Duration::from_secs(15 * 60);
/// Encoded bytes per chunk; long multi-byte texts would otherwise exceed the message limit.
const CHUNK_BYTES: usize = MAX_MESSAGE_BYTES / 2;

/// PostgreSQL text cannot hold NUL, so it never leaves the agent.
fn bounded(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| *c != '\0')
        .take(MAX_INVENTORY_TEXT)
        .collect()
}

pub fn key(kind: &str, key: &str) -> InventoryKey {
    InventoryKey {
        kind: bounded(kind),
        key: bounded(key),
    }
}

pub fn insert(snapshot: &mut Snapshot, kind: &str, name: &str, value: impl AsRef<str>) {
    if !name.trim().is_empty() {
        snapshot.insert(key(kind, name), bounded(value.as_ref()));
    }
}

/// Host, operating system, processor, memory and disks, common to every platform.
pub fn common() -> Snapshot {
    let mut snapshot = Snapshot::new();
    if let Ok(name) = sysinfo::System::host_name() {
        insert(&mut snapshot, "hardware", "host_name", name);
    }
    for (field, value) in [
        ("name", sysinfo::System::name()),
        ("version", sysinfo::System::os_version()),
        ("long_version", sysinfo::System::long_os_version()),
        ("kernel", sysinfo::System::kernel_version()),
    ] {
        if let Ok(value) = value {
            insert(&mut snapshot, "os", field, value);
        }
    }
    if let Ok(mut system) = sysinfo::System::new() {
        system.refresh_memory();
        system.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
        insert(
            &mut snapshot,
            "hardware",
            "memory_bytes",
            system.total_memory().to_string(),
        );
        if let Some(cpu) = system.cpus().first() {
            insert(&mut snapshot, "hardware", "cpu", cpu.brand());
        }
        insert(
            &mut snapshot,
            "hardware",
            "logical_cpus",
            system.cpus().len().to_string(),
        );
    }
    if let Ok(disks) = sysinfo::Disks::new_with_refreshed_list() {
        for disk in disks.iter().filter(|disk| !disk.is_removable()) {
            insert(
                &mut snapshot,
                "disk",
                &disk.mount_point().to_string_lossy(),
                format!(
                    "{} {}",
                    disk.total_space(),
                    disk.file_system().to_string_lossy()
                ),
            );
        }
    }
    snapshot
}

#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub upserts: Vec<InventoryItem>,
    pub removals: Vec<InventoryKey>,
}

pub fn diff(old: &Snapshot, new: &Snapshot) -> Changes {
    let mut changes = Changes::default();
    for (key, value) in new {
        if old.get(key) != Some(value) {
            changes.upserts.push(InventoryItem {
                key: Some(key.clone()),
                value: value.clone(),
            });
        }
    }
    for key in old.keys() {
        if !new.contains_key(key) {
            changes.removals.push(key.clone());
        }
    }
    changes
}

/// The last inventory the server acknowledged in full.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub snapshot: Snapshot,
    pub generation: u64,
    /// Unix seconds of the last completed full reconciliation; 0 forces a full one.
    pub last_full: i64,
}

#[derive(Clone, PartialEq, Message)]
struct Saved {
    #[prost(uint64, tag = "1")]
    generation: u64,
    #[prost(int64, tag = "2")]
    last_full: i64,
    #[prost(message, repeated, tag = "3")]
    items: Vec<InventoryItem>,
}

pub fn load(path: &Path) -> Option<State> {
    let saved = Saved::decode(std::fs::read(path).ok()?.as_slice()).ok()?;
    let snapshot = saved
        .items
        .into_iter()
        .filter_map(|item| Some((item.key?, item.value)))
        .collect();
    Some(State {
        snapshot,
        generation: saved.generation,
        last_full: saved.last_full,
    })
}

pub fn save(path: &Path, state: &State) -> std::io::Result<()> {
    let saved = Saved {
        generation: state.generation,
        last_full: state.last_full,
        items: state
            .snapshot
            .iter()
            .map(|(key, value)| InventoryItem {
                key: Some(key.clone()),
                value: value.clone(),
            })
            .collect(),
    };
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, saved.encode_to_vec())?;
    std::fs::rename(&temporary, path)
}

/// Chunks still to send for one reconciliation or change set.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub full: bool,
    pub generation: u64,
    pub chunks: Vec<(Vec<InventoryItem>, Vec<InventoryKey>)>,
    /// Becomes the saved state once the last chunk is acknowledged.
    pub result: State,
}

impl Plan {
    pub fn update(&self, index: usize, sequence: u64) -> Option<InventoryUpdate> {
        let (upserts, removals) = self.chunks.get(index)?.clone();
        Some(InventoryUpdate {
            sequence,
            generation: self.generation,
            full: self.full,
            last: index + 1 == self.chunks.len(),
            upserts,
            removals,
        })
    }
}

enum Entry {
    Upsert(InventoryItem),
    Removal(InventoryKey),
}

/// At most `MAX_INVENTORY_ITEMS` entries and `CHUNK_BYTES` encoded bytes per chunk.
fn chunked(changes: Changes) -> Vec<(Vec<InventoryItem>, Vec<InventoryKey>)> {
    let mut chunks = Vec::new();
    let mut current: (Vec<InventoryItem>, Vec<InventoryKey>) = (Vec::new(), Vec::new());
    let mut bytes = 0;
    let entries = changes
        .upserts
        .into_iter()
        .map(Entry::Upsert)
        .chain(changes.removals.into_iter().map(Entry::Removal));
    for entry in entries {
        let length = match &entry {
            Entry::Upsert(item) => item.encoded_len(),
            Entry::Removal(key) => key.encoded_len(),
        };
        let size = 1 + prost::length_delimiter_len(length) + length;
        let full = current.0.len() + current.1.len() == MAX_INVENTORY_ITEMS;
        if full || (bytes > 0 && bytes + size > CHUNK_BYTES) {
            chunks.push(std::mem::take(&mut current));
            bytes = 0;
        }
        bytes += size;
        match entry {
            Entry::Upsert(item) => current.0.push(item),
            Entry::Removal(key) => current.1.push(key),
        }
    }
    if !current.0.is_empty() || !current.1.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Full reconciliation when there is no state or it is a day old, otherwise only changes.
pub fn plan(
    state: Option<&State>,
    current: Snapshot,
    now: i64,
    fresh_generation: u64,
) -> Option<Plan> {
    let due = state.is_none_or(|state| now - state.last_full >= FULL_EVERY_SECONDS);
    if due {
        // Generations stay within the server's signed 64-bit column.
        let generation = match state {
            Some(state) => state.generation.wrapping_add(1) & GENERATION_MASK,
            None => fresh_generation & GENERATION_MASK,
        }
        .max(1);
        let mut chunks = chunked(diff(&Snapshot::new(), &current));
        if chunks.is_empty() {
            chunks.push((Vec::new(), Vec::new()));
        }
        return Some(Plan {
            full: true,
            generation,
            chunks,
            result: State {
                snapshot: current,
                generation,
                last_full: now,
            },
        });
    }
    let state = state?;
    let chunks = chunked(diff(&state.snapshot, &current));
    (!chunks.is_empty()).then(|| Plan {
        full: false,
        generation: state.generation,
        chunks,
        result: State {
            snapshot: current,
            ..state.clone()
        },
    })
}

/// The plan being uploaded and where the agent keeps its state.
pub struct Pending {
    pub plan: Option<Plan>,
    pub next: usize,
    pub store: PathBuf,
    pub state: Option<State>,
    /// Wakes the scheduler, for example after the server asked for a full reconciliation.
    pub wake: Arc<Notify>,
}

pub type Shared = Arc<Mutex<Pending>>;

pub fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Pending> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn shared(store: PathBuf) -> Shared {
    let state = load(&store);
    Arc::new(Mutex::new(Pending {
        plan: None,
        next: 0,
        store,
        state,
        wake: Arc::new(Notify::new()),
    }))
}

impl Pending {
    /// Records an acknowledged chunk; after the last one the result becomes the saved state.
    pub fn acknowledged(&mut self, applied: bool, now: i64) {
        let Some(plan) = &self.plan else {
            return;
        };
        if !applied {
            if let Some(state) = &mut self.state {
                state.last_full = 0;
            }
            self.plan = None;
            self.next = 0;
            self.wake.notify_one();
            return;
        }
        self.next += 1;
        if self.next < plan.chunks.len() {
            return;
        }
        let mut result = plan.result.clone();
        if !plan.full {
            result.last_full = self.state.as_ref().map_or(now, |state| state.last_full);
        }
        if let Err(error) = save(&self.store, &result) {
            tracing::warn!(%error, "inventory state could not be saved");
        }
        self.state = Some(result);
        self.plan = None;
        self.next = 0;
    }
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// Collects after a random initial delay and every 15 minutes, planning the next upload.
pub async fn schedule(
    collector: Collector,
    shared: Shared,
    first_delay: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let wake = lock(&shared).wake.clone();
    let mut wait = first_delay;
    loop {
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = wake.notified() => {}
            _ = shutdown.wait_for(|stop| *stop) => return,
        }
        wait = CHECK_EVERY;
        if lock(&shared).plan.is_some() {
            continue;
        }
        let collect = collector.clone();
        let Ok(current) = tokio::task::spawn_blocking(move || collect()).await else {
            continue;
        };
        let mut generation = [0u8; 8];
        let _ = getrandom::fill(&mut generation);
        let mut pending = lock(&shared);
        if pending.plan.is_none() {
            pending.plan = plan(
                pending.state.as_ref(),
                current,
                unix_now(),
                u64::from_le_bytes(generation),
            );
            pending.next = 0;
        }
    }
}

/// Random start within the first minute, so a fleet does not collect in step.
pub fn staggered() -> Duration {
    let mut bytes = [0u8; 2];
    let _ = getrandom::fill(&mut bytes);
    Duration::from_millis(u64::from(u16::from_le_bytes(bytes)) % 60_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(entries: &[(&str, &str, &str)]) -> Snapshot {
        let mut snapshot = Snapshot::new();
        for (kind, name, value) in entries {
            insert(&mut snapshot, kind, name, value);
        }
        snapshot
    }

    #[test]
    fn changes_cover_new_changed_and_removed_items() {
        let old = snapshot(&[
            ("software", "A", "1"),
            ("software", "B", "1"),
            ("os", "version", "11"),
        ]);
        let new = snapshot(&[
            ("software", "A", "2"),
            ("os", "version", "11"),
            ("admin", "alice", ""),
        ]);
        let changes = diff(&old, &new);
        let upserted: Vec<_> = changes
            .upserts
            .iter()
            .map(|item| item.key.clone().unwrap().key)
            .collect();
        assert_eq!(upserted, ["alice", "A"]);
        assert_eq!(changes.removals, vec![key("software", "B")]);
        assert_eq!(diff(&new, &new), Changes::default());
    }

    #[test]
    fn the_first_and_daily_runs_are_full_and_others_send_changes() {
        let current = snapshot(&[("software", "A", "1")]);
        let first = plan(None, current.clone(), 1_000_000, 7).unwrap();
        assert!(first.full && first.generation == 7 && first.chunks.len() == 1);
        let state = first.result.clone();
        assert!(
            plan(Some(&state), current.clone(), 1_000_100, 9).is_none(),
            "nothing changed"
        );
        let changed = snapshot(&[("software", "A", "2")]);
        let delta = plan(Some(&state), changed.clone(), 1_000_100, 9).unwrap();
        assert!(!delta.full && delta.generation == 7);
        let daily = plan(Some(&state), changed, 1_000_000 + FULL_EVERY_SECONDS, 9).unwrap();
        assert!(daily.full && daily.generation == 8);
    }

    #[test]
    fn generations_fit_a_signed_column() {
        let current = snapshot(&[("software", "A", "1")]);
        let first = plan(None, current.clone(), 0, u64::MAX).unwrap();
        assert!(i64::try_from(first.generation).is_ok());
        let wrapped = State {
            generation: GENERATION_MASK,
            ..first.result.clone()
        };
        let next = plan(Some(&wrapped), current, FULL_EVERY_SECONDS, 0).unwrap();
        assert_eq!(next.generation, 1);
    }

    #[test]
    fn large_inventories_are_chunked() {
        let mut big = Snapshot::new();
        for n in 0..450 {
            insert(&mut big, "software", &format!("package-{n:04}"), "1.0");
        }
        let full = plan(None, big, 0, 1).unwrap();
        let sizes: Vec<_> = full.chunks.iter().map(|(items, _)| items.len()).collect();
        assert_eq!(sizes, [200, 200, 50]);
        let last = full.update(2, 3).unwrap();
        assert!(last.last && last.full && last.sequence == 3);
        assert!(!full.update(0, 1).unwrap().last);
        let encoded = midas_protocol::encode(midas_protocol::Body::InventoryUpdate(
            full.update(0, 1).unwrap(),
        ));
        assert!(encoded.len() < midas_protocol::MAX_MESSAGE_BYTES);
    }

    #[test]
    fn long_multi_byte_texts_stay_below_the_message_limit() {
        let mut big = Snapshot::new();
        for n in 0..300 {
            let name = format!("{n:04}{}", "ő".repeat(300));
            insert(&mut big, "software", &name, "日".repeat(300));
        }
        let full = plan(None, big.clone(), 0, 1).unwrap();
        assert!(full.chunks.len() > 2);
        let mut sent = 0;
        for index in 0..full.chunks.len() {
            let update = full.update(index, 1).unwrap();
            sent += update.upserts.len();
            let encoded = midas_protocol::encode(midas_protocol::Body::InventoryUpdate(update));
            assert!(encoded.len() < midas_protocol::MAX_MESSAGE_BYTES);
        }
        assert_eq!(sent, 300);
        let mut changed = big.clone();
        let removed: Vec<_> = changed.keys().take(150).cloned().collect();
        for key in &removed {
            changed.remove(key);
        }
        let state = State {
            snapshot: big,
            generation: 1,
            last_full: 0,
        };
        let delta = plan(Some(&state), changed, 10, 2).unwrap();
        let removals: usize = delta.chunks.iter().map(|(_, keys)| keys.len()).sum();
        assert_eq!(removals, 150);
    }

    #[test]
    fn nul_characters_never_leave_the_agent() {
        let mut snapshot = Snapshot::new();
        insert(&mut snapshot, "software", "bad\0name", "1\0.0");
        let (key, value) = snapshot.into_iter().next().unwrap();
        assert_eq!((key.key.as_str(), value.as_str()), ("badname", "1.0"));
    }

    #[test]
    fn state_is_saved_only_after_the_last_acknowledgement_and_survives_restart() {
        let store = std::env::temp_dir().join(format!("midas-inventory-{}.pb", std::process::id()));
        let _ = std::fs::remove_file(&store);
        let shared = shared(store.clone());
        let mut big = Snapshot::new();
        for n in 0..250 {
            insert(&mut big, "software", &format!("p{n}"), "1");
        }
        {
            let mut pending = lock(&shared);
            pending.plan = plan(None, big.clone(), 500, 4);
            pending.acknowledged(true, 600);
            assert!(
                pending.state.is_none() && !store.exists(),
                "one of two chunks only"
            );
            pending.acknowledged(true, 600);
            assert!(pending.plan.is_none());
        }
        let restored = load(&store).unwrap();
        assert_eq!((restored.generation, restored.last_full), (4, 500));
        assert_eq!(restored.snapshot, big);
        {
            let mut pending = lock(&shared);
            pending.plan = plan(pending.state.as_ref(), Snapshot::new(), 700, 0);
            pending.acknowledged(false, 700);
            assert_eq!(
                pending.state.as_ref().unwrap().last_full,
                0,
                "refused changes force a full run"
            );
        }
        std::fs::remove_file(&store).unwrap();
    }

    #[test]
    fn common_inventory_names_the_host_and_operating_system() {
        let common = common();
        assert!(common.contains_key(&key("os", "name")), "{common:?}");
        assert!(common.contains_key(&key("hardware", "memory_bytes")));
    }
}
