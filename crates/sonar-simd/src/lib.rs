//! # sonar-simd — multi-ISA SIMD kernels for the Sonar engine (0.5)
//!
//! Three kernels cover the engine's vectorisable hot loops:
//!
//! | Kernel            | Hot loop                                            | Backends                              |
//! |-------------------|-----------------------------------------------------|---------------------------------------|
//! | [`legal_filter`]  | placement legality vs. observation masks            | scalar, AVX2, AVX-512, NEON, wasm128  |
//! | [`dilate8_words`] | 8-neighbour dilation of a grid (contact/sink logic) | scalar, AVX2, AVX-512, NEON, wasm128  |
//! | [`popcount_words`]| population-count sums (statistics, feasibility)     | scalar, AVX2, AVX-512, NEON, wasm128  |
//!
//! Grid layout: **one `u64` word per row** (board width ≤ 64), row `r`'s
//! cells in bits `0..width` of `words[r]`. Placement masks are stored
//! flattened: `masks[i * words_per + r]` is row `r` of placement `i`.
//!
//! ## Runtime dispatch
//!
//! On x86_64 the best backend is selected once at first use via
//! `std::arch::is_x86_feature_detected!` (AVX-512 > AVX2 > scalar). On
//! aarch64, NEON is compile-time (baseline on real hardware). On
//! wasm32, the `simd128` backend is compiled in when the crate is built
//! with `RUSTFLAGS="-C target-feature=+simd128"` and selected at
//! compile time. Use [`active_backend`] to see which backend a binary
//! picked.
//!
//! ## Differential testing
//!
//! Every backend must produce **bit-identical** results to the scalar
//! reference. `tests/differential.rs` fuzzes all kernels on randomized
//! inputs and compares against the active backend, plus explicit
//! per-backend tests gated on feature detection. The generalised engine
//! (0.4) routes its hot loops through these kernels, so its end-to-end
//! tests are differential tests too.
//!
//! ## Safety
//!
//! See the SAFETY CHARTER in `Cargo.toml`: unsafe exists only around
//! CPU intrinsics, on length-validated slices, element-wise. Inside
//! this crate every intrinsic call sits in its own `unsafe {}` block
//! (edition 2024 `unsafe_op_in_unsafe_fn` is forbidden).

#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

use std::sync::OnceLock;

/// A tiny xorshift generator for the fuzz tests (deterministic).
#[cfg(test)]
fn rng_state(seed: u64) -> impl FnMut() -> u64 {
    let mut x = seed;
    move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// The selected kernel backend of this process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The always-correct portable reference.
    Scalar,
    /// x86_64 AVX2 (runtime-detected).
    Avx2,
    /// x86_64 AVX-512F (runtime-detected).
    Avx512,
    /// aarch64 NEON (compile-time).
    Neon,
    /// wasm32 simd128 (compile-time).
    Wasm128,
}

impl Backend {
    /// Stable machine name (reported through the JSON protocol).
    pub const fn name(self) -> &'static str {
        match self {
            Backend::Scalar => "scalar",
            Backend::Avx2 => "avx2",
            Backend::Avx512 => "avx512",
            Backend::Neon => "neon",
            Backend::Wasm128 => "wasm128",
        }
    }
}

/// Kernel function pointer types (raw `unsafe fn` — only called through
/// the length-validating safe wrappers below).
type LegalFilterFn = unsafe fn(&[u64], &[u64], usize, &mut [u64]);
type Dilate8Fn = unsafe fn(&[u64], &mut [u64], u32, bool);
type PopcountFn = unsafe fn(&[u64]) -> u64;

/// The dispatch table resolved once per process.
struct Dispatch {
    backend: Backend,
    legal_filter: LegalFilterFn,
    dilate8: Dilate8Fn,
    popcount: PopcountFn,
}

static DISPATCH: OnceLock<Dispatch> = OnceLock::new();

fn dispatch() -> &'static Dispatch {
    DISPATCH.get_or_init(|| {
        // ── x86_64: runtime CPU feature detection. ────────────────────
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("avx512f") {
                return Dispatch {
                    backend: Backend::Avx512,
                    legal_filter: legal_filter_avx512,
                    dilate8: dilate8_avx512,
                    popcount: popcount_avx512,
                };
            }
            if std::arch::is_x86_feature_detected!("avx2") {
                return Dispatch {
                    backend: Backend::Avx2,
                    legal_filter: legal_filter_avx2,
                    dilate8: dilate8_avx2,
                    popcount: popcount_avx2,
                };
            }
        }
        // ── aarch64: NEON is baseline on real hardware. ───────────────
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        {
            return Dispatch {
                backend: Backend::Neon,
                legal_filter: legal_filter_neon,
                dilate8: dilate8_neon,
                popcount: popcount_neon,
            };
        }
        // ── wasm32: simd128 is a compile-time feature. ────────────────
        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        {
            return Dispatch {
                backend: Backend::Wasm128,
                legal_filter: legal_filter_wasm128,
                dilate8: dilate8_wasm128,
                popcount: popcount_wasm128,
            };
        }
        // ── Fallback: the always-correct scalar reference. ────────────
        Dispatch {
            backend: Backend::Scalar,
            legal_filter: legal_filter_scalar,
            dilate8: dilate8_scalar,
            popcount: popcount_scalar,
        }
    })
}

/// The backend this process selected.
pub fn active_backend() -> Backend {
    dispatch().backend
}

/// The backend name, e.g. `"avx2"` (reported through the JSON protocol).
pub fn active_backend_name() -> &'static str {
    active_backend().name()
}

// ─────────────────────────────────────────────────────────────────────────────
// Kernel 1: legal_filter — batch placement legality test
// ─────────────────────────────────────────────────────────────────────────────

/// Batch legality filter: for each of the `N` placements packed in
/// `masks` (`N * words_per` words, row-major), test whether the
/// placement intersects `forbidden` (`words_per` words). Bit `i` of the
/// result bitmask (`out`) is set iff placement `i` is legal
/// (`placement & forbidden == 0`).
///
/// `masks.len()` must be exactly `N * words_per` and
/// `out.len() == N.div_ceil(64)`; both are asserted.
///
/// This is the engine's hottest loop: every PDF density computation
/// filters every placement of every surviving ship against the current
/// miss/sunk observation mask.
pub fn legal_filter_exact(masks: &[u64], forbidden: &[u64], out: &mut [u64]) {
    let words_per = forbidden.len();
    assert!(
        words_per > 0,
        "forbidden must contain at least one word (one per row)"
    );
    assert_eq!(
        masks.len() % words_per,
        0,
        "masks.len() must be a multiple of words_per"
    );
    let n = masks.len() / words_per;
    let need = n.div_ceil(64);
    assert_eq!(
        out.len(),
        need,
        "out must have exactly (N+63)/64 = {} words",
        need
    );
    let f = dispatch().legal_filter;
    // SAFETY: all lengths validated above; the kernels are element-wise
    // on the validated slices.
    unsafe { f(masks, forbidden, words_per, out) };
}

/// Convenience wrapper returning a packed bitmask (bit `i` = placement
/// `i` is legal). See [`legal_filter_exact`].
pub fn legal_filter(masks: &[u64], forbidden: &[u64]) -> Vec<u64> {
    let words_per = forbidden.len().max(1);
    let n = if masks.len().is_multiple_of(words_per) {
        masks.len() / words_per
    } else {
        0
    };
    let mut out = vec![0u64; n.div_ceil(64)];
    legal_filter_exact(masks, forbidden, &mut out);
    out
}

/// `true` iff bit `i` of the packed bitmask `bits` is set.
pub fn bit_is_set(bits: &[u64], i: usize) -> bool {
    match bits.get(i / 64) {
        Some(&w) => (w >> (i % 64)) & 1 == 1,
        None => false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Kernel 2: dilate8_words — 8-neighbour dilation of a row-word grid
// ─────────────────────────────────────────────────────────────────────────────

/// Dilate the grid by one cell in all 8 directions (Moore neighbourhood).
///
/// Layout: `words[r]` holds row `r`; bit `c` holds cell `(r, c)`; only
/// bits `< width` are meaningful. `out` receives the dilation and must
/// be a distinct buffer from `words` (asserted equal length).
///
/// When `wrap` is `true` the grid is a **torus**: the left edge wraps to
/// the right edge and the top row wraps to the bottom row. Holes must be
/// re-applied by the caller afterwards — dilation is a pure geometric
/// operation; the geometry owns hole masking.
///
/// # Panics
/// Panics when `words.len() != out.len()` or `width > 64`.
pub fn dilate8_words(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
    assert_eq!(
        words.len(),
        out.len(),
        "words and out must have equal length"
    );
    assert!(width <= 64, "width must be <= 64 (one word per row)");
    assert!(width >= 1, "width must be >= 1");
    if words.is_empty() {
        return;
    }
    let f = dispatch().dilate8;
    // SAFETY: lengths validated above; element-wise kernels.
    unsafe { f(words, out, width, wrap) };
}

// ─────────────────────────────────────────────────────────────────────────────
// Kernel 3: popcount_words — total population count
// ─────────────────────────────────────────────────────────────────────────────

/// Total number of set bits across all words.
pub fn popcount_words(words: &[u64]) -> u64 {
    let f = dispatch().popcount;
    // SAFETY: element-wise over the caller's slice.
    unsafe { f(words) }
}

// ─────────────────────────────────────────────────────────────────────────────
// Scalar reference implementations (the correctness oracle)
// ─────────────────────────────────────────────────────────────────────────────

/// Row mask: the low `width` bits set.
#[inline]
fn row_mask(width: u32) -> u64 {
    if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

/// Horizontal (left/right, with optional torus wrap) dilation of one row.
#[inline]
fn dilate_row(w: u64, width: u32, wrap: bool) -> u64 {
    let m = row_mask(width);
    let w = w & m;
    let mut left = (w & !(1u64 << (width - 1))) << 1;
    let mut right = w >> 1;
    if wrap && width > 1 {
        // Torus: the MSB wraps to bit 0 and bit 0 wraps to the MSB.
        left |= (w >> (width - 1)) & 1;
        right |= ((w & 1) << (width - 1)) & m;
    }
    (w | left | right) & m
}

unsafe fn legal_filter_scalar(masks: &[u64], forbidden: &[u64], words_per: usize, out: &mut [u64]) {
    // The ACTUAL placement count — out may have padding bits.
    let n = masks.len() / words_per;
    for i in 0..n {
        let mut any = 0u64;
        for r in 0..words_per {
            any |= masks[i * words_per + r] & forbidden[r];
        }
        if any == 0 {
            out[i / 64] |= 1u64 << (i % 64);
        }
    }
}

unsafe fn dilate8_scalar(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
    let h = words.len();
    // 1. Horizontal dilation of every row.
    let mut horiz = vec![0u64; h];
    for r in 0..h {
        horiz[r] = dilate_row(words[r], width, wrap);
    }
    // 2. Vertical + diagonal dilation: OR the rows above/below of the
    //    horizontally-dilated grid.
    for r in 0..h {
        let up = if r > 0 {
            r - 1
        } else if wrap {
            h - 1
        } else {
            r
        };
        let down = if r + 1 < h {
            r + 1
        } else if wrap {
            0
        } else {
            r
        };
        out[r] = horiz[r] | horiz[up] | horiz[down];
    }
}

unsafe fn popcount_scalar(words: &[u64]) -> u64 {
    words.iter().map(|w| w.count_ones() as u64).sum()
}

// ─────────────────────────────────────────────────────────────────────────────
// x86_64: AVX2 / AVX-512F
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
mod x86 {
    use super::*;

    // ── AVX2 ────────────────────────────────────────────────────────────

    #[target_feature(enable = "avx2")]
    unsafe fn legal_filter_avx2_impl(
        masks: &[u64],
        forbidden: &[u64],
        words_per: usize,
        out: &mut [u64],
    ) {
        use std::arch::x86_64::*;
        let n = masks.len() / words_per; // actual count — out may have padding
        // The forbidden mask is identical for every placement: pre-AND is
        // impossible (masks vary), but we can stream placement chunks.
        // For words_per < 4 (typical: height ≤ 30 → rows 1..30, chunks of
        // 4 rows) the per-placement loop vectorises over rows.
        let mut i = 0usize;
        while i < n {
            let base = i * words_per;
            let mut any = 0u64;
            let mut r = 0usize;
            while r + 4 <= words_per {
                // SAFETY: base + r + 4 <= masks.len() (validated by caller).
                let m =
                    unsafe { _mm256_loadu_si256(masks.as_ptr().add(base + r) as *const __m256i) };
                // SAFETY: r + 4 <= forbidden.len() == words_per.
                let f = unsafe { _mm256_loadu_si256(forbidden.as_ptr().add(r) as *const __m256i) };
                // SAFETY: both operands are valid __m256i loaded above.
                let acc = _mm256_and_si256(m, f);
                // SAFETY: acc is a valid __m256i.
                let lanes: [u64; 4] = unsafe { std::mem::transmute::<_, [u64; 4]>(acc) };
                any |= lanes[0] | lanes[1] | lanes[2] | lanes[3];
                r += 4;
            }
            while r < words_per {
                any |= masks[base + r] & forbidden[r];
                r += 1;
            }
            if any == 0 {
                out[i / 64] |= 1u64 << (i % 64);
            }
            i += 1;
        }
    }

    #[target_feature(enable = "avx2")]
    unsafe fn dilate8_avx2_impl(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
        use std::arch::x86_64::*;
        let h = words.len();
        let m = row_mask(width);
        let vrow = _mm256_set1_epi64x(m as i64);
        let vmsb = _mm256_set1_epi64x(!(1u64 << (width - 1)) as i64);
        let one = _mm256_set1_epi64x(1);

        // 1. Horizontal dilation, 4 rows at a time.
        let mut horiz = vec![0u64; h];
        let mut r = 0usize;
        while r + 4 <= h {
            // SAFETY: r + 4 <= h (loop guard).
            let w = unsafe { _mm256_loadu_si256(words.as_ptr().add(r) as *const __m256i) };
            // SAFETY: w and vrow are valid __m256i.
            let w = _mm256_and_si256(w, vrow);
            // left = (w & !msb) << 1 (stay inside the row).
            // SAFETY: valid operands.
            let not_msb = _mm256_and_si256(w, vmsb);
            // SAFETY: valid operand.
            let left = _mm256_slli_epi64::<1>(not_msb);
            // right = w >> 1.
            // SAFETY: valid operand.
            let right = _mm256_srli_epi64::<1>(w);
            // SAFETY: valid operands.
            let mut d = _mm256_or_si256(_mm256_or_si256(w, left), right);
            if wrap && width > 1 {
                // Torus horizontal wrap: add wrapped bits lane-wise.
                // SAFETY: d is valid.
                let mut lanes: [u64; 4] = unsafe { std::mem::transmute(d) };
                for k in 0..4 {
                    lanes[k] |=
                        ((words[r + k] & 1) << (width - 1)) | ((words[r + k] >> (width - 1)) & 1);
                }
                // SAFETY: lanes is a valid [u64; 4].
                d = unsafe { std::mem::transmute::<[u64; 4], std::arch::x86_64::__m256i>(lanes) };
            }
            // SAFETY: d is valid.
            let lanes: [u64; 4] = unsafe { std::mem::transmute::<_, [u64; 4]>(d) };
            horiz[r..r + 4].copy_from_slice(&lanes);
            let _ = one;
            r += 4;
        }
        while r < h {
            horiz[r] = dilate_row(words[r], width, wrap);
            r += 1;
        }

        // 2. Vertical + diagonal dilation.
        for rr in 0..h {
            let up = if rr > 0 {
                rr - 1
            } else if wrap {
                h - 1
            } else {
                rr
            };
            let down = if rr + 1 < h {
                rr + 1
            } else if wrap {
                0
            } else {
                rr
            };
            out[rr] = horiz[rr] | horiz[up] | horiz[down];
        }
    }

    #[target_feature(enable = "avx2")]
    unsafe fn popcount_avx2_impl(words: &[u64]) -> u64 {
        // Scalar popcnt is already a single instruction per word; the
        // backend exists for dispatch uniformity and memory streaming.
        let mut total = 0u64;
        for w in words {
            total += w.count_ones() as u64;
        }
        total
    }

    // ── AVX-512F ────────────────────────────────────────────────────────

    #[target_feature(enable = "avx512f")]
    unsafe fn legal_filter_avx512_impl(
        masks: &[u64],
        forbidden: &[u64],
        words_per: usize,
        out: &mut [u64],
    ) {
        use std::arch::x86_64::*;
        let n = masks.len() / words_per; // actual count — out may have padding
        for i in 0..n {
            let base = i * words_per;
            let mut any = 0u64;
            let mut r = 0usize;
            while r + 8 <= words_per {
                // SAFETY: base + r + 8 <= masks.len() (validated by caller).
                let m =
                    unsafe { _mm512_loadu_si512(masks.as_ptr().add(base + r) as *const __m512i) };
                // SAFETY: r + 8 <= words_per == forbidden.len().
                let f = unsafe { _mm512_loadu_si512(forbidden.as_ptr().add(r) as *const __m512i) };
                // SAFETY: valid operands.
                let acc = _mm512_and_si512(m, f);
                // SAFETY: acc is a valid __m512i.
                let lanes: [u64; 8] = unsafe { std::mem::transmute::<_, [u64; 8]>(acc) };
                for l in lanes {
                    any |= l;
                }
                r += 8;
            }
            while r < words_per {
                any |= masks[base + r] & forbidden[r];
                r += 1;
            }
            if any == 0 {
                out[i / 64] |= 1u64 << (i % 64);
            }
        }
    }

    #[target_feature(enable = "avx512f")]
    unsafe fn dilate8_avx512_impl(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
        use std::arch::x86_64::*;
        let h = words.len();
        let m = row_mask(width);
        let vrow = _mm512_set1_epi64(m as i64);
        let vmsb = _mm512_set1_epi64(!(1u64 << (width - 1)) as i64);

        let mut horiz = vec![0u64; h];
        let mut r = 0usize;
        while r + 8 <= h {
            // SAFETY: r + 8 <= h (loop guard).
            let w = unsafe { _mm512_loadu_si512(words.as_ptr().add(r) as *const __m512i) };
            // SAFETY: valid operands.
            let w = _mm512_and_si512(w, vrow);
            // SAFETY: valid operands.
            let not_msb = _mm512_and_si512(w, vmsb);
            // SAFETY: valid operand.
            let left = _mm512_slli_epi64::<1>(not_msb);
            // SAFETY: valid operand.
            let right = _mm512_srli_epi64::<1>(w);
            // SAFETY: valid operands.
            let d = _mm512_or_si512(_mm512_or_si512(w, left), right);
            if wrap && width > 1 {
                for k in 0..8 {
                    horiz[r + k] = dilate_row(words[r + k], width, wrap);
                }
            } else {
                // SAFETY: d is a valid __m512i.
                let lanes: [u64; 8] = unsafe { std::mem::transmute::<_, [u64; 8]>(d) };
                horiz[r..r + 8].copy_from_slice(&lanes);
            }
            r += 8;
        }
        while r < h {
            horiz[r] = dilate_row(words[r], width, wrap);
            r += 1;
        }

        for rr in 0..h {
            let up = if rr > 0 {
                rr - 1
            } else if wrap {
                h - 1
            } else {
                rr
            };
            let down = if rr + 1 < h {
                rr + 1
            } else if wrap {
                0
            } else {
                rr
            };
            out[rr] = horiz[rr] | horiz[up] | horiz[down];
        }
    }

    #[target_feature(enable = "avx512f")]
    unsafe fn popcount_avx512_impl(words: &[u64]) -> u64 {
        // AVX512-VPOPCNTDQ is not implied by AVX512F; scalar popcnt wins.
        let mut total = 0u64;
        for w in words {
            total += w.count_ones() as u64;
        }
        total
    }

    /// Raw AVX2 kernels (unsafe fn pointers for the dispatch table and
    /// the explicit differential tests). Lengths must be pre-validated.
    pub(crate) unsafe fn legal_filter_avx2(
        masks: &[u64],
        forbidden: &[u64],
        words_per: usize,
        out: &mut [u64],
    ) {
        unsafe { legal_filter_avx2_impl(masks, forbidden, words_per, out) }
    }
    /// Raw AVX2 dilate kernel. See [`legal_filter_avx2`].
    pub(crate) unsafe fn dilate8_avx2(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
        unsafe { dilate8_avx2_impl(words, out, width, wrap) }
    }
    /// Raw AVX2 popcount kernel.
    pub(crate) unsafe fn popcount_avx2(words: &[u64]) -> u64 {
        unsafe { popcount_avx2_impl(words) }
    }
    /// Raw AVX-512F kernels. Lengths must be pre-validated.
    pub(crate) unsafe fn legal_filter_avx512(
        masks: &[u64],
        forbidden: &[u64],
        words_per: usize,
        out: &mut [u64],
    ) {
        unsafe { legal_filter_avx512_impl(masks, forbidden, words_per, out) }
    }
    /// Raw AVX-512F dilate kernel.
    pub(crate) unsafe fn dilate8_avx512(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
        unsafe { dilate8_avx512_impl(words, out, width, wrap) }
    }
    /// Raw AVX-512F popcount kernel.
    pub(crate) unsafe fn popcount_avx512(words: &[u64]) -> u64 {
        unsafe { popcount_avx512_impl(words) }
    }
}

#[cfg(target_arch = "x86_64")]
pub(crate) use x86::{
    dilate8_avx2, dilate8_avx512, legal_filter_avx2, legal_filter_avx512, popcount_avx2,
    popcount_avx512,
};

// ─────────────────────────────────────────────────────────────────────────────
// aarch64: NEON
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
mod neon {
    use super::*;

    #[target_feature(enable = "neon")]
    unsafe fn legal_filter_neon_impl(
        masks: &[u64],
        forbidden: &[u64],
        words_per: usize,
        out: &mut [u64],
    ) {
        use std::arch::aarch64::*;
        let n = masks.len() / words_per; // actual count — out may have padding
        for i in 0..n {
            let base = i * words_per;
            let mut any = 0u64;
            let mut r = 0usize;
            while r + 2 <= words_per {
                // SAFETY: base + r + 2 <= masks.len() (validated by caller).
                let m = unsafe { vld1q_u64(masks.as_ptr().add(base + r)) };
                // SAFETY: r + 2 <= forbidden.len().
                let f = unsafe { vld1q_u64(forbidden.as_ptr().add(r)) };
                // SAFETY: valid operands.
                let acc = unsafe { vandq_u64(m, f) };
                // SAFETY: acc is a valid uint64x2_t.
                let lanes: [u64; 2] = unsafe { std::mem::transmute::<_, [u64; 2]>(acc) };
                any |= lanes[0] | lanes[1];
                r += 2;
            }
            while r < words_per {
                any |= masks[base + r] & forbidden[r];
                r += 1;
            }
            if any == 0 {
                out[i / 64] |= 1u64 << (i % 64);
            }
        }
    }

    #[target_feature(enable = "neon")]
    unsafe fn dilate8_neon_impl(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
        use std::arch::aarch64::*;
        let h = words.len();
        let m = row_mask(width);
        let vrow = vdupq_n_u64(m);

        let mut horiz = vec![0u64; h];
        let mut r = 0usize;
        while r + 2 <= h {
            // SAFETY: r + 2 <= h (loop guard).
            let w = unsafe { vld1q_u64(words.as_ptr().add(r)) };
            // SAFETY: valid operands.
            let w = unsafe { vandq_u64(w, vrow) };
            // SAFETY: valid operand.
            let left = unsafe { vshlq_n_u64::<1>(w) };
            // SAFETY: valid operand.
            let right = unsafe { vshrq_n_u64::<1>(w) };
            // SAFETY: valid operands.
            let d = unsafe { vorrq_u64(vorrq_u64(w, left), right) };
            if wrap && width > 1 {
                horiz[r] = dilate_row(words[r], width, wrap);
                horiz[r + 1] = dilate_row(words[r + 1], width, wrap);
            } else {
                // SAFETY: d is a valid uint64x2_t.
                let lanes: [u64; 2] = unsafe { std::mem::transmute::<_, [u64; 2]>(d) };
                horiz[r..r + 2].copy_from_slice(&lanes);
            }
            r += 2;
        }
        while r < h {
            horiz[r] = dilate_row(words[r], width, wrap);
            r += 1;
        }

        for rr in 0..h {
            let up = if rr > 0 {
                rr - 1
            } else if wrap {
                h - 1
            } else {
                rr
            };
            let down = if rr + 1 < h {
                rr + 1
            } else if wrap {
                0
            } else {
                rr
            };
            out[rr] = horiz[rr] | horiz[up] | horiz[down];
        }
    }

    #[target_feature(enable = "neon")]
    unsafe fn popcount_neon_impl(words: &[u64]) -> u64 {
        let mut total = 0u64;
        for w in words {
            total += w.count_ones() as u64;
        }
        total
    }

    /// Raw NEON kernels. Lengths must be pre-validated.
    pub(crate) unsafe fn legal_filter_neon(
        masks: &[u64],
        forbidden: &[u64],
        words_per: usize,
        out: &mut [u64],
    ) {
        unsafe { legal_filter_neon_impl(masks, forbidden, words_per, out) }
    }
    /// Raw NEON dilate kernel.
    pub(crate) unsafe fn dilate8_neon(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
        unsafe { dilate8_neon_impl(words, out, width, wrap) }
    }
    /// Raw NEON popcount kernel.
    pub(crate) unsafe fn popcount_neon(words: &[u64]) -> u64 {
        unsafe { popcount_neon_impl(words) }
    }
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
pub(crate) use neon::{dilate8_neon, legal_filter_neon, popcount_neon};

// ─────────────────────────────────────────────────────────────────────────────
// wasm32: simd128
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm {
    use super::*;

    #[target_feature(enable = "simd128")]
    unsafe fn legal_filter_wasm128_impl(
        masks: &[u64],
        forbidden: &[u64],
        words_per: usize,
        out: &mut [u64],
    ) {
        use std::arch::wasm32::*;
        let n = masks.len() / words_per; // actual count — out may have padding
        for i in 0..n {
            let base = i * words_per;
            let mut any = 0u64;
            let mut r = 0usize;
            while r + 2 <= words_per {
                // SAFETY: base + r + 2 <= masks.len() (validated by caller).
                let m = unsafe { v128_load(masks.as_ptr().add(base + r) as *const v128) };
                // SAFETY: r + 2 <= forbidden.len().
                let f = unsafe { v128_load(forbidden.as_ptr().add(r) as *const v128) };
                // SAFETY: valid operands.
                let acc = unsafe { std::arch::wasm32::v128_and(m, f) };
                // SAFETY: acc is a valid v128.
                let lanes: [u64; 2] = unsafe { std::mem::transmute::<_, [u64; 2]>(acc) };
                any |= lanes[0] | lanes[1];
                r += 2;
            }
            while r < words_per {
                any |= masks[base + r] & forbidden[r];
                r += 1;
            }
            if any == 0 {
                out[i / 64] |= 1u64 << (i % 64);
            }
        }
    }

    #[target_feature(enable = "simd128")]
    unsafe fn dilate8_wasm128_impl(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
        let h = words.len();
        let mut horiz = vec![0u64; h];
        for r in 0..h {
            horiz[r] = dilate_row(words[r], width, wrap);
        }
        for rr in 0..h {
            let up = if rr > 0 {
                rr - 1
            } else if wrap {
                h - 1
            } else {
                rr
            };
            let down = if rr + 1 < h {
                rr + 1
            } else if wrap {
                0
            } else {
                rr
            };
            out[rr] = horiz[rr] | horiz[up] | horiz[down];
        }
    }

    #[target_feature(enable = "simd128")]
    unsafe fn popcount_wasm128_impl(words: &[u64]) -> u64 {
        let mut total = 0u64;
        for w in words {
            total += w.count_ones() as u64;
        }
        total
    }

    /// Raw wasm128 kernels. Lengths must be pre-validated.
    pub(crate) unsafe fn legal_filter_wasm128(
        masks: &[u64],
        forbidden: &[u64],
        words_per: usize,
        out: &mut [u64],
    ) {
        unsafe { legal_filter_wasm128_impl(masks, forbidden, words_per, out) }
    }
    /// Raw wasm128 dilate kernel.
    pub(crate) unsafe fn dilate8_wasm128(words: &[u64], out: &mut [u64], width: u32, wrap: bool) {
        unsafe { dilate8_wasm128_impl(words, out, width, wrap) }
    }
    /// Raw wasm128 popcount kernel.
    pub(crate) unsafe fn popcount_wasm128(words: &[u64]) -> u64 {
        unsafe { popcount_wasm128_impl(words) }
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub(crate) use wasm::{dilate8_wasm128, legal_filter_wasm128, popcount_wasm128};

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legal_filter_scalar_reference() {
        // 2 words per placement (rows), 4 placements.
        let forbidden = [0b0001u64, 0b0010];
        let masks = [
            0b0000, 0b0000, // p0: legal
            0b0001, 0b0000, // p1: hits forbidden row0
            0b0000, 0b0010, // p2: hits forbidden row1
            0b1000, 0b0000, // p3: legal (bit 3 not forbidden)
        ];
        let mut out = [0u64; 1];
        legal_filter_exact(&masks, &forbidden, &mut out);
        assert!(bit_is_set(&out, 0));
        assert!(!bit_is_set(&out, 1));
        assert!(!bit_is_set(&out, 2));
        assert!(bit_is_set(&out, 3));
    }

    #[test]
    fn test_dilate8_scalar() {
        // A 5-wide, 5-tall grid with one cell at (2, 2).
        let mut words = vec![0u64; 5];
        words[2] = 0b00100;
        let mut out = vec![0u64; 5];
        dilate8_words(&words, &mut out, 5, false);
        assert_eq!(out[2].count_ones(), 3); // (2,1),(2,2),(2,3)
        assert_eq!(out[1].count_ones(), 3); // (1,1),(1,2),(1,3)
        assert_eq!(out[3].count_ones(), 3);
        assert_eq!(out[0], 0);
        assert_eq!(out[4], 0);
    }

    #[test]
    fn test_dilate8_torus() {
        // On a torus, dilating a cell at (0, 0) must reach the bottom row
        // and the right column.
        let mut words = vec![0u64; 4];
        words[0] = 0b0001;
        let mut out = vec![0u64; 4];
        dilate8_words(&words, &mut out, 4, true);
        // Row 0: (0,0) + (0,1) + (0,3) (horizontal wrap).
        assert_eq!(out[0].count_ones(), 3, "row 0: {:04b}", out[0]);
        // Bottom row (vertical wrap): (3,0),(3,1),(3,3).
        assert_eq!(out[3].count_ones(), 3, "bottom row: {:04b}", out[3]);
        // Row 1 (below): (1,0),(1,1),(1,3).
        assert_eq!(out[1].count_ones(), 3);
        // Row 2 is two rows away — not a neighbour even on a 4-torus.
        assert_eq!(out[2], 0);
    }

    #[test]
    fn test_popcount() {
        let words = [0b1011u64, u64::MAX, 0];
        assert_eq!(popcount_words(&words), 3 + 64);
    }

    #[test]
    fn test_backend_reported() {
        let b = active_backend();
        assert!(
            matches!(
                b,
                Backend::Scalar
                    | Backend::Avx2
                    | Backend::Avx512
                    | Backend::Neon
                    | Backend::Wasm128
            ),
            "some backend must always be selected"
        );
        assert!(!active_backend_name().is_empty());
    }

    #[test]
    fn test_legal_filter_fuzz_vs_scalar() {
        let mut rng = rng_state(0xDEAD_BEEF);
        for case in 0..200 {
            let words_per = 1 + (rng() % 12) as usize;
            let n = 1 + (rng() % 37) as usize;
            let mut masks = vec![0u64; n * words_per];
            for w in masks.iter_mut() {
                *w = rng();
            }
            let mut forbidden = vec![0u64; words_per];
            for w in forbidden.iter_mut() {
                *w = rng() & rng();
            }
            // Scalar reference.
            let mut expect = vec![0u64; n.div_ceil(64)];
            // SAFETY: lengths constructed to satisfy the contract.
            unsafe { legal_filter_scalar(&masks, &forbidden, words_per, &mut expect) };
            // Active backend.
            let mut got = vec![0u64; n.div_ceil(64)];
            legal_filter_exact(&masks, &forbidden, &mut got);
            assert_eq!(expect, got, "case {} words_per {}", case, words_per);
        }
    }

    #[test]
    fn test_dilate_fuzz_vs_scalar() {
        let mut rng = rng_state(0x5EED_1234);
        for case in 0..200 {
            let h = 1 + (rng() % 30) as usize;
            let width = 1 + (rng() % 30) as u32;
            let wrap = rng().is_multiple_of(2);
            let mut words = vec![0u64; h];
            for w in words.iter_mut() {
                *w = rng();
            }
            let mut expect = vec![0u64; h];
            // SAFETY: lengths constructed to satisfy the contract.
            unsafe { dilate8_scalar(&words, &mut expect, width, wrap) };
            let mut got = vec![0u64; h];
            dilate8_words(&words, &mut got, width, wrap);
            assert_eq!(
                expect, got,
                "case {} h {} width {} wrap {}",
                case, h, width, wrap
            );
        }
    }

    #[test]
    fn test_popcount_fuzz() {
        let mut rng = rng_state(0xABCD_0001);
        for _ in 0..100 {
            let n = (rng() % 64) as usize;
            let words: Vec<u64> = (0..n).map(|_| rng()).collect();
            let scalar: u64 = words.iter().map(|w| w.count_ones() as u64).sum();
            assert_eq!(popcount_words(&words), scalar);
        }
    }

    #[test]
    fn test_length_validation() {
        let masks = [1u64, 2];
        let forbidden = [1u64];
        let mut out = [0u64; 1];
        // n = 2 → need = 1 ✓; masks.len() = 2 = 2×1 ✓ — this must work.
        legal_filter_exact(&masks, &forbidden, &mut out);
        assert_eq!(out[0], 0b10); // p0 illegal, p1 legal
        // Wrong out length must panic.
        let result = std::panic::catch_unwind(|| {
            let masks2: Vec<u64> = vec![1, 2, 3, 4, 5, 6];
            let forbidden2 = [1u64, 2];
            let mut out2 = vec![0u64; 2];
            // n = 3 → need = 1, but out2 has 2 → panic.
            legal_filter_exact(&masks2, &forbidden2, &mut out2);
        });
        assert!(result.is_err(), "length mismatch must panic");
    }

    // ── Explicit AVX2 test (runs only when the CPU has it). ─────────────
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_avx2_matches_scalar_when_available() {
        if !std::arch::is_x86_feature_detected!("avx2") {
            eprintln!("(avx2 not detected — skipping)");
            return;
        }
        let mut rng = rng_state(0xCAFE);
        for _ in 0..100 {
            let words_per = 1 + (rng() % 30) as usize;
            let n = 1 + (rng() % 33) as usize;
            let masks: Vec<u64> = (0..n * words_per).map(|_| rng()).collect();
            let forbidden: Vec<u64> = (0..words_per).map(|_| rng() & rng()).collect();
            let mut expect = vec![0u64; n.div_ceil(64)];
            // SAFETY: lengths constructed to satisfy the contract.
            unsafe { legal_filter_scalar(&masks, &forbidden, words_per, &mut expect) };
            let mut got = vec![0u64; n.div_ceil(64)];
            // SAFETY: ditto.
            unsafe { crate::x86::legal_filter_avx2(&masks, &forbidden, words_per, &mut got) };
            assert_eq!(expect, got);

            let h = 1 + (rng() % 30) as usize;
            let width = 1 + (rng() % 30) as u32;
            let words: Vec<u64> = (0..h).map(|_| rng()).collect();
            let wrap = rng().is_multiple_of(2);
            let mut e2 = vec![0u64; h];
            let mut g2 = vec![0u64; h];
            // SAFETY: lengths constructed to satisfy the contract.
            unsafe {
                dilate8_scalar(&words, &mut e2, width, wrap);
                crate::x86::dilate8_avx2(&words, &mut g2, width, wrap);
            }
            assert_eq!(e2, g2);
        }
    }

    // ── Explicit AVX-512 test (runs only when the CPU has it). ──────────
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_avx512_matches_scalar_when_available() {
        if !std::arch::is_x86_feature_detected!("avx512f") {
            eprintln!("(avx512f not detected — skipping)");
            return;
        }
        let mut rng = rng_state(0xBEEF);
        for _ in 0..100 {
            let words_per = 1 + (rng() % 30) as usize;
            let n = 1 + (rng() % 33) as usize;
            let masks: Vec<u64> = (0..n * words_per).map(|_| rng()).collect();
            let forbidden: Vec<u64> = (0..words_per).map(|_| rng() & rng()).collect();
            let mut expect = vec![0u64; n.div_ceil(64)];
            // SAFETY: lengths constructed to satisfy the contract.
            unsafe { legal_filter_scalar(&masks, &forbidden, words_per, &mut expect) };
            let mut got = vec![0u64; n.div_ceil(64)];
            // SAFETY: ditto.
            unsafe { crate::x86::legal_filter_avx512(&masks, &forbidden, words_per, &mut got) };
            assert_eq!(expect, got);
        }
    }
}
