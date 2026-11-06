use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Params {
    pub bitrate_bps: u64,
    /// Fraction of the captured resolution, 0.25..=1.0.
    pub scale: f64,
    pub fps: u32,
}

impl Params {
    pub fn clamped(self) -> Self {
        Self {
            bitrate_bps: self.bitrate_bps.clamp(MIN_BITRATE, MAX_BITRATE),
            scale: self.scale.clamp(0.25, 1.0),
            fps: self.fps.clamp(5, 60),
        }
    }
}

pub const MIN_BITRATE: u64 = 150_000;
pub const MAX_BITRATE: u64 = 8_000_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct Observation {
    pub estimate_bps: Option<u64>,
    pub loss: Option<f64>,
    pub rtt: Option<Duration>,
}

/// Shared adaptive controller: lowers quickly on congestion, raises in small steps after a stable period.
pub struct Controller {
    current: Params,
    last_change: Instant,
    stable_since: Instant,
}

const RAISE_AFTER: Duration = Duration::from_secs(2);
const RAISE_STEP: f64 = 1.25;

pub fn ladder(bitrate_bps: u64) -> (f64, u32) {
    match bitrate_bps {
        b if b >= 2_500_000 => (1.0, 30),
        b if b >= 1_200_000 => (0.75, 30),
        b if b >= 600_000 => (0.5, 24),
        _ => (0.5, 15),
    }
}

impl Controller {
    pub fn new(start: Params, now: Instant) -> Self {
        Self {
            current: start.clamped(),
            last_change: now,
            stable_since: now,
        }
    }

    #[cfg(test)]
    pub fn current(&self) -> Params {
        self.current
    }

    /// Returns new parameters and the reason when they change.
    pub fn update(
        &mut self,
        now: Instant,
        observed: Observation,
    ) -> Option<(Params, &'static str)> {
        let Some(estimate) = observed.estimate_bps else {
            // Missing measurements never count as a healthy link.
            self.stable_since = now;
            return None;
        };
        let mut target = (estimate as f64 * 0.9) as u64;
        let lossy = observed.loss.is_some_and(|l| l > 0.05);
        if lossy {
            target = (target as f64 * 0.7) as u64;
        }
        if observed.rtt.is_some_and(|r| r > Duration::from_millis(400)) {
            target = (target as f64 * 0.8) as u64;
        }
        let target = target.clamp(MIN_BITRATE, MAX_BITRATE);
        let current = self.current.bitrate_bps;
        let (bitrate, reason) = if (target as f64) < current as f64 * 0.9 {
            self.stable_since = now;
            (target, if lossy { "loss" } else { "estimate_down" })
        } else if target as f64 > current as f64 * 1.1
            && now.duration_since(self.stable_since) >= RAISE_AFTER
            && now.duration_since(self.last_change) >= RAISE_AFTER
        {
            (
                ((current as f64 * RAISE_STEP) as u64).min(target),
                "recover",
            )
        } else {
            if target < current {
                self.stable_since = now;
            }
            return None;
        };
        let (scale, fps) = ladder(bitrate);
        let next = Params {
            bitrate_bps: bitrate,
            scale,
            fps,
        }
        .clamped();
        if next == self.current {
            return None;
        }
        self.current = next;
        self.last_change = now;
        Some((next, reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(bps: u64, loss: f64) -> Observation {
        Observation {
            estimate_bps: Some(bps),
            loss: Some(loss),
            rtt: Some(Duration::from_millis(20)),
        }
    }

    #[test]
    fn drops_fast_and_recovers_gradually_without_oscillation() {
        let start = Instant::now();
        let mut control = Controller::new(
            Params {
                bitrate_bps: 4_000_000,
                scale: 1.0,
                fps: 30,
            },
            start,
        );
        let (down, reason) = control
            .update(start + Duration::from_millis(500), seen(600_000, 0.08))
            .unwrap();
        assert_eq!(reason, "loss");
        assert!(down.bitrate_bps < 600_000 && down.scale <= 0.5 && down.fps <= 15);
        let mut changes = 0;
        let mut previous = down.bitrate_bps;
        for step in 1..=60 {
            let now = start + Duration::from_millis(500 + step * 500);
            if let Some((next, reason)) = control.update(now, seen(5_000_000, 0.0)) {
                assert_eq!(reason, "recover");
                assert!(next.bitrate_bps <= (previous as f64 * RAISE_STEP) as u64 + 1);
                previous = next.bitrate_bps;
                changes += 1;
            }
        }
        assert!(changes >= 3, "recovery is gradual: {changes}");
        assert!(control.current().bitrate_bps > 3_000_000);
        assert_eq!(control.current().scale, 1.0);
    }

    #[test]
    fn missing_measurement_never_raises_quality() {
        let start = Instant::now();
        let mut control = Controller::new(
            Params {
                bitrate_bps: 300_000,
                scale: 0.5,
                fps: 15,
            },
            start,
        );
        for step in 1..20 {
            assert!(
                control
                    .update(start + Duration::from_secs(step), Observation::default())
                    .is_none()
            );
        }
        assert_eq!(control.current().bitrate_bps, 300_000);
    }
}
