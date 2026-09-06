//! Hard time limit for a bot move.
//!
//! The bot has 20 seconds per move by default. Every search loop
//! (hypothesis regeneration, PDF) cooperatively aborts once the deadline
//! expires.
//!
//! ## WebAssembly
//!
//! `Instant::now()` is not implemented on `wasm32-unknown-unknown`, so on
//! WASM every `Deadline` constructor returns `Deadline::none()`. The engine
//! then falls back to being *work-limited* (bounded by
//! `EngineConfig::hypothesis_soft_target`) instead of time-limited, which
//! keeps moves fast and deterministic in the browser. The public field type
//! stays `Option<Instant>` on both platforms — on WASM it is simply always
//! `None`, and no `Instant::now()` call is ever executed.

use std::time::{Duration, Instant};

/// A deadline — the point in time by which a move must be complete.
#[derive(Clone, Copy, Debug)]
pub struct Deadline {
    /// `None` = no limit (tests, fast benchmarks, WASM builds).
    pub limit: Option<Instant>,
    /// Cooperative check period: the deadline is only re-read every
    /// `check_interval` iterations, to avoid hammering the clock in hot
    /// loops. Must be a power of two.
    pub check_interval: u32,
}

impl Deadline {
    /// No time limit. Search is bounded only by hypothesis counts.
    pub fn none() -> Self {
        Self {
            limit: None,
            check_interval: 1024,
        }
    }

    /// A limit of `secs` seconds from now (disabled on WASM — see module docs).
    pub fn from_secs(secs: u64) -> Self {
        Self::from_duration(Duration::from_secs(secs))
    }

    /// A limit of `dur` from now (disabled on WASM — see module docs).
    pub fn from_duration(dur: Duration) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                limit: Some(Instant::now() + dur),
                check_interval: 256,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // WASM has no wall clock: search is work-limited instead.
            let _ = dur;
            Self::none()
        }
    }

    /// The default competitive deadline: 20 seconds.
    pub fn default_20s() -> Self {
        Self::from_secs(20)
    }

    /// Has the deadline already passed? (Always `false` for `Deadline::none()`
    /// and on WASM.)
    #[inline(always)]
    pub fn expired(self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.limit {
                Some(t) => Instant::now() >= t,
                None => false,
            }
        }
    }

    /// Remaining time before expiry (`None` when there is no limit).
    #[inline]
    pub fn remaining(self) -> Option<Duration> {
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.limit.map(|t| {
                let now = Instant::now();
                if t > now {
                    t.duration_since(now)
                } else {
                    Duration::ZERO
                }
            })
        }
    }

    /// Should the caller check the clock on iteration `iter`?
    /// Used to amortise clock reads in tight loops.
    #[inline(always)]
    pub fn should_check(self, iter: u32) -> bool {
        iter & (self.check_interval - 1) == 0
    }

    /// Check the deadline (amortised) and return `true` if it expired.
    #[inline(always)]
    pub fn check_expired(self, iter: u32) -> bool {
        if !self.should_check(iter) {
            return false;
        }
        self.expired()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deadline_none_never_expires() {
        let d = Deadline::none();
        for i in 0..10000 {
            assert!(!d.check_expired(i));
        }
        assert!(!d.expired());
        assert!(d.remaining().is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_deadline_from_duration() {
        let d = Deadline::from_duration(Duration::from_millis(50));
        assert!(!d.expired());
        std::thread::sleep(Duration::from_millis(60));
        assert!(d.expired());
    }

    #[test]
    fn test_check_interval_is_power_of_two() {
        for d in [Deadline::none(), Deadline::from_secs(1)] {
            assert_eq!(d.check_interval.count_ones(), 1);
        }
    }
}
