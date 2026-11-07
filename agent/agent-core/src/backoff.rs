use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackoffPolicy {
    pub base: Duration,
    pub cap: Duration,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(1),
            cap: Duration::from_secs(300),
        }
    }
}

#[derive(Debug)]
pub struct Backoff {
    policy: BackoffPolicy,
    attempt: u32,
}

impl Backoff {
    pub fn new(policy: BackoffPolicy) -> Self {
        Self { policy, attempt: 0 }
    }

    /// Doubling delay with jitter in its upper half, so a fleet that lost the
    /// server at the same moment does not reconnect in lockstep.
    pub fn next_delay(&mut self, random: u64) -> Duration {
        let factor = 1u32.checked_shl(self.attempt).unwrap_or(u32::MAX);
        let ceiling = self.policy.base.saturating_mul(factor).min(self.policy.cap);
        self.attempt = self.attempt.saturating_add(1);
        jittered(ceiling, random)
    }

    /// Delay used when the server refused this agent's protocol versions.
    pub fn longest_delay(&self, random: u64) -> Duration {
        jittered(self.policy.cap, random)
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

fn jittered(ceiling: Duration, random: u64) -> Duration {
    let half = ceiling / 2;
    let span = u64::try_from((ceiling - half).as_nanos()).unwrap_or(u64::MAX);
    half + Duration::from_nanos(random % span.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: BackoffPolicy = BackoffPolicy {
        base: Duration::from_secs(1),
        cap: Duration::from_secs(60),
    };

    #[test]
    fn delay_doubles_until_the_cap() {
        let mut backoff = Backoff::new(POLICY);
        let highest: Vec<_> = (0..9).map(|_| backoff.next_delay(u64::MAX)).collect();
        let lowest: Vec<_> = {
            let mut backoff = Backoff::new(POLICY);
            (0..9).map(|_| backoff.next_delay(0)).collect()
        };
        let ceilings = [1, 2, 4, 8, 16, 32, 60, 60, 60].map(Duration::from_secs);
        for ((high, low), ceiling) in highest.iter().zip(&lowest).zip(ceilings) {
            assert!(
                *high <= ceiling && *low == ceiling / 2,
                "{high:?} {low:?} {ceiling:?}"
            );
        }
    }

    #[test]
    fn jitter_stays_within_the_upper_half() {
        for random in [1, 12_345, 987_654_321, u64::MAX / 3] {
            let delay = Backoff {
                policy: POLICY,
                attempt: 4,
            }
            .next_delay(random);
            assert!((Duration::from_secs(8)..=Duration::from_secs(16)).contains(&delay));
        }
    }

    #[test]
    fn reset_and_long_attempt_counts_are_safe() {
        let mut backoff = Backoff {
            policy: POLICY,
            attempt: u32::MAX,
        };
        assert!(backoff.next_delay(u64::MAX) <= POLICY.cap);
        backoff.reset();
        assert!(backoff.next_delay(u64::MAX) <= POLICY.base);
        assert!(backoff.longest_delay(0) == POLICY.cap / 2);
    }
}
