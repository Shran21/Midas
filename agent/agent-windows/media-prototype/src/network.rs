use std::{
    collections::VecDeque,
    net::SocketAddr,
    time::{Duration, Instant},
};

use serde::Serialize;

/// Emulated bottleneck on the media send path: capacity, random loss, added delay and a bounded queue.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Profile {
    pub name: &'static str,
    pub bandwidth_bps: Option<u64>,
    pub loss: f64,
    pub delay: Duration,
}

pub const PROFILES: [Profile; 4] = [
    Profile {
        name: "unlimited",
        bandwidth_bps: None,
        loss: 0.0,
        delay: Duration::ZERO,
    },
    Profile {
        name: "good",
        bandwidth_bps: Some(8_000_000),
        loss: 0.0,
        delay: Duration::from_millis(10),
    },
    Profile {
        name: "degraded",
        bandwidth_bps: Some(1_500_000),
        loss: 0.01,
        delay: Duration::from_millis(60),
    },
    Profile {
        name: "bad",
        bandwidth_bps: Some(500_000),
        loss: 0.05,
        delay: Duration::from_millis(150),
    },
];

pub fn profile(name: &str) -> Option<Profile> {
    PROFILES.iter().copied().find(|p| p.name == name)
}

struct Queued {
    release: Instant,
    destination: SocketAddr,
    contents: Vec<u8>,
}

pub struct Impairment {
    profile: Profile,
    queue: VecDeque<Queued>,
    link_free: Instant,
    random: u64,
    pub dropped_loss: u64,
    pub dropped_queue: u64,
}

/// At most this much data may wait for the emulated link; later packets are dropped (drop-tail).
const QUEUE_LIMIT: Duration = Duration::from_millis(250);

impl Impairment {
    pub fn new(profile: Profile, seed: u64) -> Self {
        Self {
            profile,
            queue: VecDeque::new(),
            link_free: Instant::now(),
            random: seed | 1,
            dropped_loss: 0,
            dropped_queue: 0,
        }
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
    }

    fn next_random(&mut self) -> f64 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        (self.random >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn queue_delay(&self, now: Instant) -> Duration {
        self.link_free.saturating_duration_since(now)
    }

    pub fn push(&mut self, now: Instant, destination: SocketAddr, contents: Vec<u8>) {
        if self.profile.loss > 0.0 && self.next_random() < self.profile.loss {
            self.dropped_loss += 1;
            return;
        }
        let start = self.link_free.max(now);
        let serialization = match self.profile.bandwidth_bps {
            Some(bps) => Duration::from_secs_f64(contents.len() as f64 * 8.0 / bps as f64),
            None => Duration::ZERO,
        };
        if start.saturating_duration_since(now) > QUEUE_LIMIT {
            self.dropped_queue += 1;
            return;
        }
        self.link_free = start + serialization;
        self.queue.push_back(Queued {
            release: self.link_free + self.profile.delay,
            destination,
            contents,
        });
    }

    /// Packets whose emulated transfer finished by `now`.
    pub fn pop_ready(&mut self, now: Instant) -> Option<(SocketAddr, Vec<u8>)> {
        if self.queue.front().is_some_and(|q| q.release <= now) {
            let q = self.queue.pop_front()?;
            return Some((q.destination, q.contents));
        }
        None
    }

    pub fn next_release(&self) -> Option<Instant> {
        self.queue.front().map(|q| q.release)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address() -> SocketAddr {
        "127.0.0.1:9".parse().unwrap()
    }

    #[test]
    fn bandwidth_cap_spaces_packets_and_bounds_the_queue() {
        let now = Instant::now();
        let mut link = Impairment::new(profile("bad").unwrap(), 7);
        link.set_profile(Profile {
            loss: 0.0,
            ..profile("bad").unwrap()
        });
        for _ in 0..200 {
            link.push(now, address(), vec![0; 1250]);
        }
        let accepted = link.queue.len();
        assert!(accepted > 0 && accepted < 200, "queue must stay bounded");
        assert!(link.dropped_queue > 0);
        let last = link.queue.back().unwrap().release;
        let span = last.duration_since(now);
        assert!(span <= QUEUE_LIMIT + Duration::from_millis(170), "{span:?}");
        assert!(link.pop_ready(now).is_none(), "added delay applies");
        assert!(link.pop_ready(now + Duration::from_millis(175)).is_some());
    }

    #[test]
    fn loss_rate_follows_the_profile() {
        let now = Instant::now();
        let mut link = Impairment::new(
            Profile {
                name: "loss",
                bandwidth_bps: None,
                loss: 0.05,
                delay: Duration::ZERO,
            },
            42,
        );
        for _ in 0..20_000 {
            link.push(now, address(), vec![0; 100]);
        }
        let rate = link.dropped_loss as f64 / 20_000.0;
        assert!((0.04..0.06).contains(&rate), "{rate}");
    }
}
