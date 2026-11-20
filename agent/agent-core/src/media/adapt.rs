//! The shared adaptive video controller (PERF-07). It turns network and encoder reports into a
//! bitrate, a capture scale and a frame rate. It lowers quality at once on sustained congestion,
//! raises it in small steps only after a stable period, never treats a missing measurement as
//! free bandwidth, and never changes permissions or consent.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// What the operator is doing; text keeps resolution, motion keeps the frame rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Text,
    Motion,
}

/// What the active encoder and source can deliver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub min_bitrate: u64,
    pub max_bitrate: u64,
    pub max_fps: u32,
    /// Largest frame the encoder handles in real time, in pixels.
    pub max_pixels: u64,
}

impl Limits {
    /// A software encoder: real-time up to 1080p at 30 frames per second.
    pub const SOFTWARE: Self = Self {
        min_bitrate: 150_000,
        max_bitrate: 6_000_000,
        max_fps: 30,
        max_pixels: 1920 * 1080,
    };
    /// A hardware encoder: up to 4K at 60 frames per second.
    pub const HARDWARE: Self = Self {
        min_bitrate: 150_000,
        max_bitrate: 20_000_000,
        max_fps: 60,
        max_pixels: 3840 * 2160,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub bitrate: u64,
    /// Fraction of the captured size, before the encoder's pixel limit applies.
    pub scale: f64,
    pub fps: u32,
}

/// One report from the transport and the encoder queue.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    /// Estimated available bandwidth in bits per second.
    pub estimate: Option<u64>,
    pub rtt: Option<Duration>,
    /// Lost fraction of packets since the previous report, 0..=1.
    pub loss: Option<f64>,
    pub jitter: Option<Duration>,
    /// Frames waiting for the encoder or the sender.
    pub queued_frames: usize,
}

/// Measurements for the operator; missing ones stay missing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Network {
    pub rtt: Option<Duration>,
    pub loss: Option<f64>,
    pub degraded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Congestion,
    Estimate,
    Recovery,
    /// No usable measurement for a while.
    Unmeasured,
    /// Frames pile up before the encoder or the sender.
    Backlog,
    Profile,
    Reconnect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change {
    pub settings: Settings,
    pub reason: Reason,
}

/// The most bandwidth assumed while measurements are missing, and where every connection starts.
const UNMEASURED_CAP: u64 = 1_000_000;
const START_BITRATE: u64 = UNMEASURED_CAP;
/// Sustained congestion cuts the current bitrate at most this often.
const CUT_INTERVAL: Duration = Duration::from_secs(1);
const STALE: Duration = Duration::from_secs(3);
/// Increases need this long without any sign of congestion, between each step.
const RAISE_AFTER: Duration = Duration::from_secs(3);
const RAISE_STEP: f64 = 1.2;
const HEADROOM: f64 = 0.85;
const CONGESTION_CUT: f64 = 0.7;
const SMOOTHING: f64 = 0.3;
const LOSSY: f64 = 0.05;
const JITTERY: Duration = Duration::from_millis(60);
const DELAYED: Duration = Duration::from_millis(150);
const BACKLOG: usize = 2;
/// A step up the quality ladder needs this much more bitrate than its threshold.
const LADDER_MARGIN: f64 = 1.15;
/// Increases look at the lowest raw estimate of this window, so a fluctuating link stays put.
const FLOOR_WINDOW: Duration = Duration::from_secs(5);
const RECENT_LIMIT: usize = 128;

/// Bitrate thresholds and what each allows, best first: (minimum bitrate, scale, frame rate).
fn ladder(profile: Profile) -> &'static [(u64, f64, u32)] {
    match profile {
        Profile::Text => &[
            (1_500_000, 1.0, 30),
            (800_000, 1.0, 15),
            (450_000, 1.0, 8),
            (250_000, 0.75, 5),
            (0, 0.5, 5),
        ],
        Profile::Motion => &[
            (2_500_000, 1.0, 30),
            (1_200_000, 0.75, 30),
            (600_000, 0.5, 24),
            (300_000, 0.5, 15),
            (0, 0.375, 10),
        ],
    }
}

fn smooth(previous: Option<f64>, value: f64) -> f64 {
    previous.map_or(value, |old| old + SMOOTHING * (value - old))
}

pub struct Controller {
    profile: Profile,
    limits: Limits,
    current: Settings,
    rung: usize,
    /// Frame-rate steps taken because of encoder backlog.
    backlog_steps: u32,
    estimate: Option<f64>,
    loss: Option<f64>,
    jitter: Option<f64>,
    min_rtt: Option<Duration>,
    rtt: Option<f64>,
    measured: Option<Instant>,
    /// Recent raw estimates; increases never go beyond their minimum.
    recent: VecDeque<(Instant, u64)>,
    calm_since: Instant,
    last_raise: Instant,
    last_cut: Instant,
}

impl Controller {
    pub fn new(profile: Profile, limits: Limits, now: Instant) -> Self {
        let mut controller = Self {
            profile,
            limits,
            current: Settings {
                bitrate: 0,
                scale: 1.0,
                fps: 1,
            },
            rung: 0,
            backlog_steps: 0,
            estimate: None,
            loss: None,
            jitter: None,
            min_rtt: None,
            rtt: None,
            measured: None,
            recent: VecDeque::new(),
            calm_since: now,
            last_raise: now,
            last_cut: now,
        };
        controller.start(now);
        controller
    }

    pub fn current(&self) -> Settings {
        self.current
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    fn start(&mut self, now: Instant) {
        self.estimate = None;
        self.loss = None;
        self.jitter = None;
        self.min_rtt = None;
        self.rtt = None;
        self.measured = None;
        self.recent.clear();
        self.backlog_steps = 0;
        self.unsettle(now);
        self.last_raise = now;
        self.last_cut = now;
        let bitrate = START_BITRATE.clamp(self.limits.min_bitrate, self.limits.max_bitrate);
        self.rung = self.rung_for(bitrate, ladder(self.profile).len() - 1);
        self.current = self.settings(bitrate);
    }

    /// A new connection starts over from conservative settings and forgets old measurements.
    pub fn reconnect(&mut self, now: Instant) -> Change {
        self.start(now);
        Change {
            settings: self.current,
            reason: Reason::Reconnect,
        }
    }

    pub fn set_profile(&mut self, profile: Profile, now: Instant) -> Option<Change> {
        if profile == self.profile {
            return None;
        }
        self.profile = profile;
        let bitrate = self.current.bitrate;
        self.rung = self.rung_for(bitrate, ladder(profile).len() - 1);
        self.unsettle(now);
        self.apply(bitrate, Reason::Profile)
    }

    /// The highest rung the bitrate allows; moving up from `from` needs the extra margin.
    fn rung_for(&self, bitrate: u64, from: usize) -> usize {
        let steps = ladder(self.profile);
        steps
            .iter()
            .enumerate()
            .find(|(index, (threshold, _, _))| {
                let needed = if *index < from {
                    (*threshold as f64 * LADDER_MARGIN) as u64
                } else {
                    *threshold
                };
                bitrate >= needed
            })
            .map_or(steps.len() - 1, |(index, _)| index)
    }

    fn unsettle(&mut self, now: Instant) {
        self.calm_since = now;
    }

    fn settings(&self, bitrate: u64) -> Settings {
        let (_, scale, fps) = ladder(self.profile)[self.rung];
        let mut fps = fps.min(self.limits.max_fps);
        for _ in 0..self.backlog_steps {
            fps = (fps * 2 / 3).max(5);
        }
        Settings {
            bitrate,
            scale,
            fps: fps.max(1),
        }
    }

    fn apply(&mut self, bitrate: u64, reason: Reason) -> Option<Change> {
        let bitrate = bitrate.clamp(self.limits.min_bitrate, self.limits.max_bitrate);
        self.rung = self.rung_for(bitrate, self.rung);
        let next = self.settings(bitrate);
        if next == self.current {
            return None;
        }
        self.current = next;
        Some(Change {
            settings: next,
            reason,
        })
    }

    /// Loss, jitter or delay above their limits in the smoothed measurements.
    fn congested(&self) -> bool {
        let lossy = self.loss.is_some_and(|loss| loss > LOSSY);
        let jittery = self
            .jitter
            .is_some_and(|jitter| jitter > JITTERY.as_secs_f64());
        let delayed = match (self.rtt, self.min_rtt) {
            (Some(rtt), Some(least)) => rtt > (least + DELAYED).as_secs_f64(),
            _ => false,
        };
        lossy || jittery || delayed
    }

    /// The link as the operator sees it: smoothed measurements, and whether it is degraded now.
    pub fn network(&self) -> Network {
        Network {
            rtt: self.rtt.map(Duration::from_secs_f64),
            loss: self.loss,
            degraded: self.congested(),
        }
    }

    pub fn update(&mut self, now: Instant, sample: Sample) -> Option<Change> {
        if let Some(estimate) = sample.estimate {
            self.estimate = Some(smooth(self.estimate, estimate as f64));
            self.measured = Some(now);
            if self.recent.len() == RECENT_LIMIT {
                self.recent.pop_front();
            }
            self.recent.push_back((now, estimate));
        }
        while self
            .recent
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > FLOOR_WINDOW)
        {
            self.recent.pop_front();
        }
        if let Some(loss) = sample.loss.filter(|loss| (0.0..=1.0).contains(loss)) {
            self.loss = Some(smooth(self.loss, loss));
        }
        if let Some(jitter) = sample.jitter {
            self.jitter = Some(smooth(self.jitter, jitter.as_secs_f64()));
        }
        if let Some(rtt) = sample.rtt {
            self.min_rtt = Some(self.min_rtt.map_or(rtt, |least| least.min(rtt)));
            self.rtt = Some(smooth(self.rtt, rtt.as_secs_f64()));
        }
        let backlog = sample.queued_frames >= BACKLOG;
        let congested = self.congested();
        let stale = self
            .measured
            .is_none_or(|at| now.saturating_duration_since(at) >= STALE);
        if congested || backlog || stale {
            self.unsettle(now);
        }

        if backlog && self.current.fps > 5 {
            self.backlog_steps += 1;
            return self.apply(self.current.bitrate, Reason::Backlog);
        }
        if stale {
            // Unknown capacity: hold, and never stay above the conservative cap.
            let capped = self.current.bitrate.min(UNMEASURED_CAP);
            return (capped < self.current.bitrate)
                .then(|| self.apply(capped, Reason::Unmeasured))
                .flatten();
        }
        let target = (self.estimate.unwrap_or(0.0) * HEADROOM) as u64;
        if congested {
            // Loss, delay or jitter mean the estimate is too high: cut below both, once per interval.
            let by_estimate = (target as f64 * CONGESTION_CUT) as u64;
            let by_current = if now.saturating_duration_since(self.last_cut) >= CUT_INTERVAL {
                (self.current.bitrate as f64 * CONGESTION_CUT) as u64
            } else {
                self.current.bitrate
            };
            let cut = by_estimate.min(by_current);
            if cut < self.current.bitrate {
                self.last_cut = now;
                return self.apply(cut, Reason::Congestion);
            }
            return None;
        }
        if (target as f64) < self.current.bitrate as f64 * 0.9 {
            self.unsettle(now);
            return self.apply(target, Reason::Estimate);
        }
        let calm = now.saturating_duration_since(self.calm_since) >= RAISE_AFTER
            && now.saturating_duration_since(self.last_raise) >= RAISE_AFTER;
        if !calm {
            return None;
        }
        if self.backlog_steps > 0 {
            self.backlog_steps -= 1;
            self.last_raise = now;
            return self.apply(self.current.bitrate, Reason::Recovery);
        }
        let floor = self.recent.iter().map(|(_, estimate)| *estimate).min();
        let ceiling = target.min((floor.unwrap_or(0) as f64 * HEADROOM) as u64);
        if ceiling as f64 > self.current.bitrate as f64 * 1.1 {
            self.last_raise = now;
            let raised = ((self.current.bitrate as f64 * RAISE_STEP) as u64).min(ceiling);
            return self.apply(raised, Reason::Recovery);
        }
        None
    }
}

/// The encoded frame size for a source, within the encoder's pixel limit, in even dimensions
/// of at least 160 by 90 pixels.
pub fn frame_size(source: (u32, u32), scale: f64, limits: Limits) -> (u32, u32) {
    let (width, height) = (source.0.max(2) as f64, source.1.max(2) as f64);
    let fit = (limits.max_pixels as f64 / (width * height))
        .sqrt()
        .min(1.0);
    let factor = scale.clamp(0.1, 1.0).min(fit);
    let even = |value: f64, least: u32| ((value as u32).max(least) / 2 * 2).max(2);
    (
        even(width * factor, 160.min(source.0)),
        even(height * factor, 90.min(source.1)),
    )
}

#[cfg(test)]
mod tests;
