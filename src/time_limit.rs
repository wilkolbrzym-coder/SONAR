//! Twardy limit czasu dla ruchu bota.
//!
//! Bot ma 20 sekund na ruch (domyślnie). Wszystkie operacje szukające
//! rozwiązania (regeneracja hipotez, PDF) są kooperacyjnie przerywane
//! po upływie deadline.

use std::time::{Duration, Instant};

/// Deadline - punkt w czasie, do którego bot musi zakończyć ruch.
#[derive(Clone, Copy, Debug)]
pub struct Deadline {
    /// None = bez limitu (testy, benchmarki szybsze)
    pub limit: Option<Instant>,
    /// Śpij co najmniej tę ilość czasu między sprawdzaniem deadline
    /// (żeby nie marnować CPU na sprawdzanie Instant::now() w każdej iteracji)
    pub check_interval: u32,
}

impl Deadline {
    /// Bez limitu czasu
    pub fn none() -> Self {
        Self { limit: None, check_interval: 1024 }
    }

    /// Limit `secs` sekund od teraz
    pub fn from_secs(secs: u64) -> Self {
        Self {
            limit: Some(Instant::now() + Duration::from_secs(secs)),
            check_interval: 256,
        }
    }

    /// Limit `dur` od teraz
    pub fn from_duration(dur: Duration) -> Self {
        Self {
            limit: Some(Instant::now() + dur),
            check_interval: 256,
        }
    }

    /// Domyślny: 20 sekund
    pub fn default_20s() -> Self {
        Self::from_secs(20)
    }

    /// Czy deadline już minął
    #[inline(always)]
    pub fn expired(self) -> bool {
        match self.limit {
            Some(t) => Instant::now() >= t,
            None => false,
        }
    }

    /// Pozostały czas
    #[inline]
    pub fn remaining(self) -> Option<Duration> {
        self.limit.map(|t| {
            let now = Instant::now();
            if t > now { t.duration_since(now) } else { Duration::ZERO }
        })
    }

    /// Sprawdź czy trzeba zakończyć po N iteracji (kooperacyjne)
    /// `iter` jest zwiększany przez wywołującego
    #[inline(always)]
    pub fn should_check(self, iter: u32) -> bool {
        iter & (self.check_interval - 1) == 0
    }

    /// Sprawdź + zwróć true jeśli przekroczono
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
    }

    #[test]
    fn test_deadline_from_duration() {
        let d = Deadline::from_duration(Duration::from_millis(50));
        assert!(!d.expired());
        std::thread::sleep(Duration::from_millis(60));
        assert!(d.expired());
    }

    #[test]
    fn test_check_interval() {
        let d = Deadline::none();
        // Powinno sprawdzać co check_interval iteracji
        for i in 0..10000 {
            let _ = d.check_expired(i);
        }
    }
}
