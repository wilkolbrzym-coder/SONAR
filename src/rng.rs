//! Własny, bardzo szybki PRNG (xoshiro256**) - znacznie szybszy niż rand crate.
//! 16 bajtów stanu, period 2^256 - 1, dobre rozkład.

use std::sync::Mutex;

/// Stan xoshiro256**
#[derive(Clone, Copy, Debug)]
pub struct Xoshiro256 {
    s: [u64; 4],
}

impl Xoshiro256 {
    /// Inicjalizacja z ziarna (SplitMix64 do rozproszenia bitów)
    #[inline(always)]
    pub fn from_seed(seed: u64) -> Self {
        let mut sm = SplitMix64 { state: seed };
        Self {
            s: [sm.next(), sm.next(), sm.next(), sm.next()],
        }
    }

    /// Następna liczba 64-bitowa
    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;

        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];

        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);

        result
    }

    /// Następna liczba w zakresie [0, n)
    #[inline(always)]
    pub fn gen_range(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        // Lemire's method - bez modulo, bez biasu
        let mut x = (self.next_u64() as u128) * (n as u128);
        let mut l = x as u64;
        if l < n {
            let t = n.wrapping_neg() % n;
            while l < t {
                x = (self.next_u64() as u128) * (n as u128);
                l = x as u64;
            }
        }
        (x >> 64) as u64
    }

    /// Wypełnij bufor losowymi bajtami
    #[inline(always)]
    pub fn fill_bytes(&mut self, dst: &mut [u8]) {
        let mut i = 0;
        while i + 8 <= dst.len() {
            let v = self.next_u64().to_le_bytes();
            dst[i..i + 8].copy_from_slice(&v);
            i += 8;
        }
        if i < dst.len() {
            let v = self.next_u64().to_le_bytes();
            for (k, &b) in v.iter().enumerate() {
                if i + k >= dst.len() {
                    break;
                }
                dst[i + k] = b;
            }
        }
    }
}

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    #[inline(always)]
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
}

// Globalny ziarno-łączny generator (Mutex, bo Rust wymaga Send dla globali)
use std::sync::OnceLock;

static GLOBAL_RNG: OnceLock<Mutex<Xoshiro256>> = OnceLock::new();

fn global() -> &'static Mutex<Xoshiro256> {
    GLOBAL_RNG.get_or_init(|| {
        // Ziarno z /dev/urandom + czas
        let mut seed = 0u64;
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let _ = f.read_exact(unsafe {
                std::slice::from_raw_parts_mut(&mut seed as *mut u64 as *mut u8, 8)
            });
        }
        if seed == 0 {
            seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xC0FFEE);
        }
        Mutex::new(Xoshiro256::from_seed(seed))
    })
}

/// Globalna liczba losowa 64-bit
#[inline(always)]
pub fn random_u64() -> u64 {
    global().lock().unwrap().next_u64()
}

/// Globalna liczba w zakresie [0, n)
#[inline(always)]
pub fn random_range(n: u64) -> u64 {
    global().lock().unwrap().gen_range(n)
}

/// Pobierz własny, odłączony generator (np. dla wątków benchmarku)
pub fn thread_rng() -> Xoshiro256 {
    Xoshiro256::from_seed(random_u64().wrapping_mul(0x100000001B3).wrapping_add(std::process::id() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_distribution() {
        let mut rng = Xoshiro256::from_seed(42);
        let mut counts = [0u32; 10];
        for _ in 0..100_000 {
            counts[rng.gen_range(10) as usize] += 1;
        }
        // Każda wartość powinna wystąpić ~10000 razy ±5%
        for &c in counts.iter() {
            assert!(c > 9000 && c < 11000, "Bad distribution: {}", c);
        }
    }

    #[test]
    fn test_deterministic() {
        let mut a = Xoshiro256::from_seed(123);
        let mut b = Xoshiro256::from_seed(123);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }
}
