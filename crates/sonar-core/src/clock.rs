//! Monotonic clock abstraction shared by native and WebAssembly builds.
//!
//! Native builds use `std::time::Instant`. WASM builds import a
//! `performance.now()`-backed millisecond clock from the host
//! (`sonar_env.now_ms`), because `Instant::now()` is not available on
//! `wasm32-unknown-unknown`.
//!
//! All timing inside the engine goes through this module, which keeps every
//! build target free of platform panics.

/// Microseconds elapsed on a monotonic clock since an arbitrary epoch
/// (process start on native, page load on the web).
///
/// This is only used for *reporting* (elapsed-time fields, benchmark
/// output). Search limits are enforced in "work units" (hypothesis
/// attempts), not wall-clock time, so results stay deterministic and
/// cross-platform comparable.
#[inline]
pub fn now_us() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_micros() as u64
    }
    #[cfg(target_arch = "wasm32")]
    {
        now_ms_wasm() as u64 * 1_000
    }
}

/// Milliseconds elapsed on a monotonic clock.
#[inline]
pub fn now_ms() -> u64 {
    now_us() / 1_000
}

/// Imported host clock for WASM builds. The JS glue module must provide
/// `sonar_env.now_ms()` (typically `performance.now()`).
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "sonar_env")]
extern "C" {
    #[link_name = "now_ms"]
    fn now_ms_wasm() -> f64;
}

/// Wall-clock seconds since the Unix epoch (used for record timestamps).
#[inline]
pub fn unix_secs() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (now_ms() / 1_000) as u64
    }
}
