//! Algorytm targeting - SERCE siły bota.
//!
//! Hybryda trzech mechanizmów:
//!  1. PDF (Probability Density Function) - dla każdego pola liczymy
//!     na ile sposobów można położyć niezatopione statki tak, by przechodziły przez to pole,
//!     zgodnie z obserwacjami (trafienia/pudła/zatopienia).
//!  2. Hunt/Target - po trafieniu, constraint propagation zawęża możliwe orientacje.
//!  3. Hipotezy o flocie - Bayesowski filtr, odrzucanie niespójnych.
//!
//! Decyzja = argmax pola po PDF z preferencją pól z aktywnym sąsiedztwem trafień.

use crate::bitboard::{BitBoard, MASK_100};
use crate::board::Board;
use crate::fleet::FLEET;
use crate::rng::Xoshiro256;
use crate::time_limit::Deadline;

// Re-eksport z hypothesis dla wygody
pub use crate::hypothesis::HybridTargeting;

// Re-export the fleet placements function for convenience.
pub use crate::fleet::placements;

/// Publiczny interfejs targeting - daje decyzję ruchu
pub trait TargetingStrategy: Send + Sync {
    /// Wybierz następny ruch patrząc na stan wrogiej planszy.
    /// `deadline` określa maksymalny czas na decyzję.
    fn choose(&mut self, view: &EnemyView, rng: &mut Xoshiro256, deadline: Deadline) -> (usize, usize);

    /// Zaktualizuj po strzale
    fn observe(&mut self, r: usize, c: usize, result: crate::board::ShotResult);

    /// Reset do nowej gry
    fn reset(&mut self);
}

/// Widok wrogiej planszy z perspektywy bota
#[derive(Clone, Debug, Default)]
pub struct EnemyView {
    /// Gdzie strzelano (pudła + trafienia + zatopienia)
    pub shots: BitBoard,
    /// Gdzie trafiono (nawet zatopione)
    pub hits: BitBoard,
    /// Gdzie zatopiono
    pub sunk: BitBoard,
    /// Lista długości niezatopionych statków wroga
    pub remaining: Vec<u8>,
}

impl EnemyView {
    pub fn new() -> Self {
        Self {
            shots: BitBoard::new(),
            hits: BitBoard::new(),
            sunk: BitBoard::new(),
            remaining: FLEET.to_vec(),
        }
    }

    /// Pola, na których na pewno NIE ma statku (pudła)
    #[inline]
    pub fn miss_mask(&self) -> BitBoard {
        BitBoard(self.shots.0 & !self.hits.0)
    }

    /// Pola, na których na pewno JEST statek (trafienia + zatopienia)
    #[inline]
    pub fn hit_mask(&self) -> BitBoard {
        self.hits
    }

    /// Pola niezestrzelone - kandydaci do strzału
    #[inline]
    pub fn unknown(&self) -> BitBoard {
        BitBoard(MASK_100 & !self.shots.0)
    }

    /// Aktywne trafienia - niezatopione (środek statku, który jeszcze żyje)
    #[inline]
    pub fn active_hits(&self) -> BitBoard {
        BitBoard(self.hits.0 & !self.sunk.0)
    }

    /// Zaktualizuj stan po wyniku strzału.
    /// UWAGA: bot NIE zna maski statku wroga, więc dla Sunk(len) musi zrekonstruować
    /// pola zatopionego statku z aktywnych trafień. To działa bo statek jest liniowy.
    pub fn observe(&mut self, r: usize, c: usize, result: crate::board::ShotResult) {
        use crate::board::ShotResult::*;
        let bit = 1u128 << (r * 10 + c);
        self.shots.0 |= bit;
        match result {
            Miss => {}
            Hit => {
                self.hits.0 |= bit;
            }
            Sunk(len) => {
                self.hits.0 |= bit;
                // Rekonstrukcja maski zatopionego statku:
                // Szukamy ciągłego pasma `len` pól zawierającego (r,c) z aktywnych trafień.
                let sunk_mask = reconstruct_sunk_ship(self.hits.0 & !self.sunk.0, r, c, len);
                self.sunk.0 |= sunk_mask;
                // Po zatopieniu, otoczenie statku jest pudłami (standardowa zasada)
                let dilated = BitBoard(sunk_mask).dilate8().0;
                let extra = dilated & !sunk_mask & MASK_100;
                self.shots.0 |= extra;
                // Usuwamy długość z remaining
                if let Some(idx) = self.remaining.iter().position(|&l| l == len) {
                    self.remaining.swap_remove(idx);
                }
            }
            _ => {}
        }
    }

    /// Konstrukcja z planszy wroga (do testów)
    pub fn from_board(board: &Board) -> Self {
        let mut v = Self::new();
        v.shots = board.shots;
        v.hits = board.hits;
        v.sunk = board.sunk;
        v.remaining = board.remaining_ship_lengths();
        v
    }
}

/// Rekonstrukcja maski zatopionego statku z aktywnych trafień.
/// Statek jest liniowy (poziomo lub pionowo), więc szukamy pasma `len` pól.
fn reconstruct_sunk_ship(active_hits: u128, r: usize, c: usize, len: u8) -> u128 {
    // Spróbuj poziomo: rozszerzaj w lewo i prawo od (r,c)
    let mut mask_h = 1u128 << (r * 10 + c);
    let mut cc = c;
    while cc > 0 {
        cc -= 1;
        let bit = 1u128 << (r * 10 + cc);
        if (active_hits & bit) != 0 {
            mask_h |= bit;
        } else {
            break;
        }
    }
    let mut cc = c;
    while cc < 9 {
        cc += 1;
        let bit = 1u128 << (r * 10 + cc);
        if (active_hits & bit) != 0 {
            mask_h |= bit;
        } else {
            break;
        }
    }
    if mask_h.count_ones() == len as u32 {
        return mask_h;
    }

    // Spróbuj pionowo
    let mut mask_v = 1u128 << (r * 10 + c);
    let mut rr = r;
    while rr > 0 {
        rr -= 1;
        let bit = 1u128 << (rr * 10 + c);
        if (active_hits & bit) != 0 {
            mask_v |= bit;
        } else {
            break;
        }
    }
    let mut rr = r;
    while rr < 9 {
        rr += 1;
        let bit = 1u128 << (rr * 10 + c);
        if (active_hits & bit) != 0 {
            mask_v |= bit;
        } else {
            break;
        }
    }
    if mask_v.count_ones() == len as u32 {
        return mask_v;
    }

    // Fallback - zwróć poziomą maskę (prawdopodobnie statek był w całości)
    mask_h
}

/// ============================================================
/// PDF Targeting - state-of-the-art
/// ============================================================

/// Konfiguracja PDF strategy
#[derive(Clone, Copy, Debug)]
pub struct PdfConfig {
    /// Waga pól sąsiadujących z trafieniami (target mode bonus)
    pub target_bonus: f32,
    /// Parzystość - preferuj pola parzyste w hunt mode
    pub use_parity: bool,
    /// Po znalezieniu >0 trafień, skup się w pełni na target
    pub hard_target: bool,
}

impl Default for PdfConfig {
    fn default() -> Self {
        Self {
            target_bonus: 100.0,
            use_parity: true,
            hard_target: true,
        }
    }
}

/// PDF Targeting
pub struct PdfTargeting {
    pub config: PdfConfig,
    /// Pojemna macierz gęstości (100 pól) - opcjonalna, dla debugowania
    pub last_density: [f32; 100],
    /// Opcjonalny learning bias (z mikro-learningu)
    pub learning_bias: Option<[f32; 100]>,
    /// Waga learning biasu (0.0 = brak, 1.0 = pełna)
    pub learning_weight: f32,
}

impl PdfTargeting {
    pub fn new(config: PdfConfig) -> Self {
        Self {
            config,
            last_density: [0.0; 100],
            learning_bias: None,
            learning_weight: 0.0,
        }
    }

    /// Włącz learning bias
    pub fn with_learning(mut self, bias: [f32; 100], weight: f32) -> Self {
        self.learning_bias = Some(bias);
        self.learning_weight = weight;
        self
    }

    /// Oblicz macierz gęstości prawdopodobieństwa dla każdego pola.
    /// Density[i] = liczba możliwych umiejscowień niezatopionych statków przechodzących przez i,
    /// zgodnych z obserwacjami.
    pub fn compute_density(&self, view: &EnemyView) -> [f32; 100] {
        let mut density = [0.0f32; 100];
        let miss = view.miss_mask().0;
        let sunk = view.sunk.0;
        let active_hits = view.active_hits().0;
        let known_no_ship = miss | sunk; // na pewno nie ma statku

        let placements = placements();

        // Dla każdego niezatopionego statku
        for &len in &view.remaining {
            let len_idx = len as usize;
            if len_idx >= placements.len() {
                continue;
            }
            // Sprawdzamy wszystkie umiejscowienia tego statku
            for &(_, _, _, mask) in &placements[len_idx] {
                // Czy umiejscowienie zachodzi na pola "na pewno nie ma statku"?
                if (mask & known_no_ship) != 0 {
                    continue;
                }
                // Jeśli są aktywne trafienia, preferuj umiejscowienia które przez nie przechodzą
                let contains_active = (mask & active_hits) != 0;
                let has_active = active_hits != 0;
                if has_active && !contains_active {
                    continue;
                }
                // Waga - jeśli umiejscowienie zawiera aktywne trafienia, zwiększ wagę
                let weight = if contains_active {
                    self.config.target_bonus
                } else {
                    1.0
                };
                // Dodaj wagę do każdego pola umiejscowienia
                let mut m = mask;
                while m != 0 {
                    let idx = m.trailing_zeros() as usize;
                    density[idx] += weight;
                    m &= m - 1;
                }
            }
        }

        // Jeśli używamy parzystości i nie ma aktywnych trafień
        if self.config.use_parity && active_hits == 0 {
            let min_len = view.remaining.iter().copied().min().unwrap_or(2) as usize;
            if min_len >= 2 {
                for r in 0..10 {
                    for c in 0..10 {
                        if (r + c) % 2 == 0 {
                            density[r * 10 + c] *= 1.05;
                        }
                    }
                }
            }
        }

        // Aplikuj learning bias (lekki bonus z mikro-learningu)
        if let Some(bias) = &self.learning_bias {
            if self.learning_weight > 0.0 && active_hits == 0 {
                for i in 0..100 {
                    density[i] += bias[i] * self.learning_weight;
                }
            }
        }

        // Wyzeruj pola niedostępne (już strzelane)
        let mut s = view.shots.0;
        while s != 0 {
            let idx = s.trailing_zeros() as usize;
            density[idx] = 0.0;
            s &= s - 1;
        }

        density
    }

    /// Wybierz najlepszy ruch (deadline-aware, ale PDF jest szybki - ~1ms)
    pub fn choose_move(&self, view: &EnemyView, rng: &mut Xoshiro256, _deadline: Deadline) -> (usize, usize) {
        let density = self.compute_density(view);
        let shots_mask = view.shots.0;
        // Znajdź max z małą losowością dla tie-break, NIE bierzemy strzelanych pól
        let mut best_val = f32::MIN;
        let mut best_cells: [usize; 16] = [0; 16];
        let mut best_count: usize = 0;
        for (i, &v) in density.iter().enumerate() {
            // Pomiń pola już strzelane
            if (shots_mask & (1u128 << i)) != 0 {
                continue;
            }
            if v > best_val {
                best_val = v;
                best_count = 0;
                best_cells[best_count] = i;
                best_count += 1;
            } else if (v - best_val).abs() < 1e-6 && best_count < 16 {
                best_cells[best_count] = i;
                best_count += 1;
            }
        }
        if best_count == 0 {
            // Awaryjnie - losowe niezestrzelone (gdyby density wszystkie były 0)
            let un = view.unknown();
            let cells: Vec<_> = un.iter_cells().collect();
            if cells.is_empty() {
                return (0, 0);
            }
            let i = rng.gen_range(cells.len() as u64) as usize;
            return cells[i];
        }
        let pick = if best_count == 1 { 0 } else { rng.gen_range(best_count as u64) as usize };
        let i = best_cells[pick];
        (i / 10, i % 10)
    }
}

impl TargetingStrategy for PdfTargeting {
    fn choose(&mut self, view: &EnemyView, rng: &mut Xoshiro256, deadline: Deadline) -> (usize, usize) {
        let mv = self.choose_move(view, rng, deadline);
        self.last_density = self.compute_density(view);
        mv
    }

    fn observe(&mut self, _r: usize, _c: usize, _result: crate::board::ShotResult) {
        // Stan trzyma game loop w EnemyView - tutaj nic do zrobienia
    }

    fn reset(&mut self) {
        self.last_density = [0.0; 100];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, Ship, ShotResult};

    #[test]
    fn test_density_initial() {
        let v = EnemyView::new();
        let pdf = PdfTargeting::new(PdfConfig::default());
        let d = pdf.compute_density(&v);
        // Na początku środek planszy powinien mieć najwyższą gęstość
        // (najwięcej umiejscowień statków przez środek)
        let center = d[5 * 10 + 5];
        let corner = d[0];
        assert!(center > corner, "Center ({}) should be > corner ({})", center, corner);
        // I nie powinno być zera nigdzie (poza debug)
        for &v in d.iter() {
            assert!(v >= 0.0);
        }
    }

    #[test]
    fn test_density_after_miss() {
        let mut v = EnemyView::new();
        v.observe(5, 5, ShotResult::Miss);
        let pdf = PdfTargeting::new(PdfConfig::default());
        let d = pdf.compute_density(&v);
        // Pole (5,5) ma gęstość 0
        assert_eq!(d[5 * 10 + 5], 0.0);
    }

    #[test]
    fn test_target_mode_after_hit() {
        let mut v = EnemyView::new();
        v.observe(5, 5, ShotResult::Hit);
        let pdf = PdfTargeting::new(PdfConfig::default());
        let d = pdf.compute_density(&v);
        // Pola sąsiadujące z (5,5) powinny mieć wyższą gęstość niż dalekie
        let adjacent = d[5 * 10 + 6]; // (5,6)
        let far = d[0]; // (0,0)
        assert!(adjacent > far, "Adjacent ({}) should be > far ({})", adjacent, far);
    }

    #[test]
    fn test_choose_never_shoots_same_cell() {
        let mut v = EnemyView::new();
        let mut pdf = PdfTargeting::new(PdfConfig::default());
        let mut rng = Xoshiro256::from_seed(42);

        // Wykonaj 30 ruchów
        for _ in 0..30 {
            let (r, c) = pdf.choose(&v, &mut rng, Deadline::none());
            assert!(!v.shots.test(r, c), "Bot shot at already-shot cell ({},{})", r, c);
            // Symuluj wynik
            let res = if (r + c) % 3 == 0 { ShotResult::Hit } else { ShotResult::Miss };
            v.observe(r, c, res);
        }
    }

    #[test]
    fn test_density_respects_remaining() {
        let mut v = EnemyView::new();
        v.remaining = vec![2]; // tylko statek długości 2
        let pdf = PdfTargeting::new(PdfConfig::default());
        let d = pdf.compute_density(&v);
        // Suma gęstości powinna być znacznie mniejsza niż gdybyśmy mieli wszystkie statki
        let sum: f32 = d.iter().sum();
        assert!(sum > 0.0, "Density should be > 0");
        assert!(sum < 5000.0, "Density should be bounded");
    }

    #[test]
    fn test_sunk_removes_from_remaining() {
        let mut v = EnemyView::new();
        assert_eq!(v.remaining.len(), 5);
        v.observe(0, 0, ShotResult::Sunk(2));
        assert_eq!(v.remaining.len(), 4);
        assert!(!v.remaining.contains(&2));
    }
}
