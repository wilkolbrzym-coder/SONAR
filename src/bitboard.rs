//! BitBoard 128-bitowy dla planszy 10x10.
//!
//! Konwencja: pole (r, c) gdzie r,c in 0..=9
//! bit index = r * 10 + c  (0..=99)
//! Maska wszystkich 100 pól = `MASK_100`
//!
//! Wszystkie operacje są inline i kompilują się do 2-3 instrukcji SIMD na x86_64.

/// Reprezentacja bitowa 10x10 (górne 100 bitów u128)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct BitBoard(pub u128);

/// Maska 100 pól planszy
pub const MASK_100: u128 = (1u128 << 100) - 1;

/// Maska kolumny 0 (lewy brzeg) - bity 0, 10, 20, 30, 40, 50, 60, 70, 80, 90
pub const COL_LEFT: u128 = 0b0000000001_0000000001_0000000001_0000000001_0000000001_0000000001_0000000001_0000000001_0000000001_0000000001u128;
/// Maska kolumny 9 (prawy brzeg) - bity 9, 19, 29, 39, 49, 59, 69, 79, 89, 99
pub const COL_RIGHT: u128 = 0b1000000000_1000000000_1000000000_1000000000_1000000000_1000000000_1000000000_1000000000_1000000000_1000000000u128;

/// Prekalkulowana tablica masek sąsiadów 8-kierunkowych dla każdego z 100 pól.
/// NEIGHBORS8[idx] = maska 8 sąsiadów pola o indeksie idx.
/// Wypełniana raz przy starcie (OnceLock).
pub static NEIGHBORS8_CACHE: std::sync::OnceLock<[u128; 100]> = std::sync::OnceLock::new();

/// Pobierz maskę 8 sąsiadów pola (r,c) z cache (szybciej niż liczenie za każdym razem)
#[inline(always)]
pub fn neighbors8_cached(r: usize, c: usize) -> u128 {
    NEIGHBORS8_CACHE.get_or_init(|| {
        let mut arr = [0u128; 100];
        for rr in 0..10 {
            for cc in 0..10 {
                let mut m = 0u128;
                let lo_r = if rr > 0 { rr - 1 } else { 0 };
                let hi_r = if rr < 9 { rr + 1 } else { 9 };
                let lo_c = if cc > 0 { cc - 1 } else { 0 };
                let hi_c = if cc < 9 { cc + 1 } else { 9 };
                for dr in lo_r..=hi_r {
                    for dc in lo_c..=hi_c {
                        if dr == rr && dc == cc { continue; }
                        m |= 1u128 << (dr * 10 + dc);
                    }
                }
                arr[rr * 10 + cc] = m;
            }
        }
        arr
    })[r * 10 + c]
}

/// Prekalkulowana tablica masek 4 sąsiadów (Von Neumanna)
pub static NEIGHBORS4_CACHE: std::sync::OnceLock<[u128; 100]> = std::sync::OnceLock::new();

#[inline(always)]
pub fn neighbors4_cached(r: usize, c: usize) -> u128 {
    NEIGHBORS4_CACHE.get_or_init(|| {
        let mut arr = [0u128; 100];
        for rr in 0..10 {
            for cc in 0..10 {
                let mut m = 0u128;
                if rr > 0 { m |= 1u128 << ((rr - 1) * 10 + cc); }
                if rr < 9 { m |= 1u128 << ((rr + 1) * 10 + cc); }
                if cc > 0 { m |= 1u128 << (rr * 10 + (cc - 1)); }
                if cc < 9 { m |= 1u128 << (rr * 10 + (cc + 1)); }
                arr[rr * 10 + cc] = m;
            }
        }
        arr
    })[r * 10 + c]
}

impl BitBoard {
    /// Wiersz w masce bitowej (10 bitów)
    #[inline(always)]
    pub const fn row_mask(r: usize) -> u128 {
        0x3FFu128 << (r * 10)
    }

    /// Maska kolumny
    #[inline(always)]
    pub const fn col_mask(c: usize) -> u128 {
        let mut m = 0u128;
        let mut r = 0;
        while r < 10 {
            m |= 1u128 << (r * 10 + c);
            r += 1;
        }
        m
    }
}

impl BitBoard {
    pub const EMPTY: BitBoard = BitBoard(0);
    pub const FULL: BitBoard = BitBoard(MASK_100);

    #[inline(always)]
    pub const fn new() -> Self {
        BitBoard(0)
    }

    #[inline(always)]
    pub const fn from_bits(b: u128) -> Self {
        BitBoard(b & MASK_100)
    }

    /// Indeks bitu (r,c) = r*10 + c
    #[inline(always)]
    pub const fn idx(r: usize, c: usize) -> u32 {
        (r * 10 + c) as u32
    }

    #[inline(always)]
    pub const fn bit(r: usize, c: usize) -> u128 {
        1u128 << Self::idx(r, c)
    }

    /// Ustaw pole (r,c)
    #[inline(always)]
    pub fn set(&mut self, r: usize, c: usize) {
        self.0 |= Self::bit(r, c);
    }

    /// Czyść pole (r,c)
    #[inline(always)]
    pub fn clear(&mut self, r: usize, c: usize) {
        self.0 &= !Self::bit(r, c);
    }

    /// Sprawdź czy pole jest ustawione
    #[inline(always)]
    pub fn test(&self, r: usize, c: usize) -> bool {
        (self.0 & Self::bit(r, c)) != 0
    }

    /// Ilość zapalonych bitów (popcount - pojedyncza instrukcja CPU)
    #[inline(always)]
    pub fn popcount(self) -> u32 {
        self.0.count_ones()
    }

    /// Czy pusty
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// AND
    #[inline(always)]
    pub const fn and(self, o: BitBoard) -> BitBoard {
        BitBoard(self.0 & o.0)
    }

    /// OR
    #[inline(always)]
    pub const fn or(self, o: BitBoard) -> BitBoard {
        BitBoard(self.0 | o.0)
    }

    /// XOR
    #[inline(always)]
    pub const fn xor(self, o: BitBoard) -> BitBoard {
        BitBoard(self.0 ^ o.0)
    }

    /// NOT (z maską 100 bitów)
    #[inline(always)]
    pub const fn not(self) -> BitBoard {
        BitBoard((!self.0) & MASK_100)
    }

    /// Różnica (self bez o)
    #[inline(always)]
    pub const fn diff(self, o: BitBoard) -> BitBoard {
        BitBoard(self.0 & !o.0)
    }

    /// Iteruj po zapalonych polach - zwraca (r, c)
    #[inline]
    pub fn iter_cells(self) -> BitIter {
        BitIter { bits: self.0 }
    }

    /// Pierwszy zapalony bit jako (r, c) lub None
    #[inline(always)]
    pub fn first_cell(self) -> Option<(usize, usize)> {
        if self.0 == 0 {
            None
        } else {
            let idx = self.0.trailing_zeros() as usize;
            Some((idx / 10, idx % 10))
        }
    }

    /// Czy sąsiaduje (8-kierunkowo) z jakimkolwiek zapalonym bitem
    #[inline]
    pub fn has_adjacent(self, r: usize, c: usize) -> bool {
        let mut m = 0u128;
        // wiersz wyżej
        if r > 0 {
            if c > 0 { m |= Self::bit(r - 1, c - 1); }
            m |= Self::bit(r - 1, c);
            if c < 9 { m |= Self::bit(r - 1, c + 1); }
        }
        // ten sam wiersz
        if c > 0 { m |= Self::bit(r, c - 1); }
        if c < 9 { m |= Self::bit(r, c + 1); }
        // wiersz niżej
        if r < 9 {
            if c > 0 { m |= Self::bit(r + 1, c - 1); }
            m |= Self::bit(r + 1, c);
            if c < 9 { m |= Self::bit(r + 1, c + 1); }
        }
        (self.0 & m) != 0
    }

    /// Maska 8-sąsiadów pola (r,c) ograniczona do planszy
    #[inline]
    pub fn neighbors8(r: usize, c: usize) -> BitBoard {
        let mut m = 0u128;
        if r > 0 {
            if c > 0 { m |= Self::bit(r - 1, c - 1); }
            m |= Self::bit(r - 1, c);
            if c < 9 { m |= Self::bit(r - 1, c + 1); }
        }
        if c > 0 { m |= Self::bit(r, c - 1); }
        if c < 9 { m |= Self::bit(r, c + 1); }
        if r < 9 {
            if c > 0 { m |= Self::bit(r + 1, c - 1); }
            m |= Self::bit(r + 1, c);
            if c < 9 { m |= Self::bit(r + 1, c + 1); }
        }
        BitBoard(m)
    }

    /// Maska 4-sąsiadów (góra/dół/lewo/prawo)
    #[inline]
    pub fn neighbors4(r: usize, c: usize) -> BitBoard {
        let mut m = 0u128;
        if r > 0 { m |= Self::bit(r - 1, c); }
        if r < 9 { m |= Self::bit(r + 1, c); }
        if c > 0 { m |= Self::bit(r, c - 1); }
        if c < 9 { m |= Self::bit(r, c + 1); }
        BitBoard(m)
    }

    /// Shift w lewo o 1 (na planszy: r, c-1) - z maskowaniem brzegów
    #[inline(always)]
    pub fn shift_left(self) -> BitBoard {
        // Najpierw shift, potem wyczyść prawą kolumnę bo przeleje się z lewej
        let shifted = self.0 >> 1;
        BitBoard(shifted & !COL_RIGHT)
    }

    /// Shift w prawo o 1 (r, c+1)
    #[inline(always)]
    pub fn shift_right(self) -> BitBoard {
        let shifted = self.0 << 1;
        BitBoard(shifted & !COL_LEFT & MASK_100)
    }

    /// Shift w górę (r-1, c)
    #[inline(always)]
    pub fn shift_up(self) -> BitBoard {
        BitBoard(self.0 >> 10)
    }

    /// Shift w dół (r+1, c)
    #[inline(always)]
    pub fn shift_down(self) -> BitBoard {
        BitBoard((self.0 << 10) & MASK_100)
    }

    /// Dilate: rozszerza o 1 pole we wszystkich 8 kierunkach (Moore neighborhood)
    #[inline]
    pub fn dilate8(self) -> BitBoard {
        // 4 sąsiadów Von Neumanna (góra/dół/lewo/prawo)
        let mut r = self.0;
        r |= (self.0 >> 1) & !COL_RIGHT;
        r |= (self.0 << 1) & !COL_LEFT & MASK_100;
        r |= self.0 >> 10;
        r |= (self.0 << 10) & MASK_100;
        // 4 przekątne (rogi) - dilacja przesunięta po przekątnej
        // LEWO-GÓRA: shift_left + shift_up
        r |= (self.0 >> 11) & !COL_RIGHT;
        // PRAWO-GÓRA: shift_right + shift_up
        r |= (self.0 >> 9) & !COL_LEFT;
        // LEWO-DÓŁ: shift_left + shift_down
        r |= (self.0 << 9) & !COL_RIGHT & MASK_100;
        // PRAWO-DÓŁ: shift_right + shift_down
        r |= (self.0 << 11) & !COL_LEFT & MASK_100;
        BitBoard(r & MASK_100)
    }
}

/// Iterator po zapalonych bitach - bardzo szybki (trailing_zeros)
pub struct BitIter {
    bits: u128,
}

impl Iterator for BitIter {
    type Item = (usize, usize);

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.bits == 0 {
            return None;
        }
        let idx = self.bits.trailing_zeros() as usize;
        self.bits &= self.bits.wrapping_sub(1); // clear lowest set bit
        Some((idx / 10, idx % 10))
    }
}

impl std::ops::BitAnd for BitBoard {
    type Output = BitBoard;
    #[inline(always)]
    fn bitand(self, o: BitBoard) -> BitBoard { BitBoard(self.0 & o.0) }
}
impl std::ops::BitOr for BitBoard {
    type Output = BitBoard;
    #[inline(always)]
    fn bitor(self, o: BitBoard) -> BitBoard { BitBoard(self.0 | o.0) }
}
impl std::ops::BitXor for BitBoard {
    type Output = BitBoard;
    #[inline(always)]
    fn bitxor(self, o: BitBoard) -> BitBoard { BitBoard(self.0 ^ o.0) }
}
impl std::ops::Not for BitBoard {
    type Output = BitBoard;
    #[inline(always)]
    fn not(self) -> BitBoard { BitBoard((!self.0) & MASK_100) }
}
impl std::ops::BitAndAssign for BitBoard {
    #[inline(always)]
    fn bitand_assign(&mut self, o: BitBoard) { self.0 &= o.0; }
}
impl std::ops::BitOrAssign for BitBoard {
    #[inline(always)]
    fn bitor_assign(&mut self, o: BitBoard) { self.0 |= o.0; }
}
impl std::ops::BitXorAssign for BitBoard {
    #[inline(always)]
    fn bitxor_assign(&mut self, o: BitBoard) { self.0 ^= o.0; }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_test_clear() {
        let mut b = BitBoard::new();
        b.set(5, 7);
        assert!(b.test(5, 7));
        assert!(!b.test(5, 8));
        assert!(!b.test(4, 7));
        assert_eq!(b.popcount(), 1);
        b.clear(5, 7);
        assert!(!b.test(5, 7));
        assert_eq!(b.popcount(), 0);
    }

    #[test]
    fn test_iter_cells() {
        let mut b = BitBoard::new();
        b.set(0, 0);
        b.set(9, 9);
        b.set(3, 5);
        let cells: Vec<_> = b.iter_cells().collect();
        assert_eq!(cells.len(), 3);
        assert!(cells.contains(&(0, 0)));
        assert!(cells.contains(&(9, 9)));
        assert!(cells.contains(&(3, 5)));
    }

    #[test]
    fn test_neighbors() {
        let n = BitBoard::neighbors8(5, 5);
        assert_eq!(n.popcount(), 8);
        let n = BitBoard::neighbors8(0, 0);
        assert_eq!(n.popcount(), 3);
        let n = BitBoard::neighbors8(9, 9);
        assert_eq!(n.popcount(), 3);
        let n = BitBoard::neighbors4(5, 5);
        assert_eq!(n.popcount(), 4);
    }

    #[test]
    fn test_shifts() {
        let mut b = BitBoard::new();
        b.set(5, 5);
        assert!(b.shift_left().test(5, 4));
        assert!(b.shift_right().test(5, 6));
        assert!(b.shift_up().test(4, 5));
        assert!(b.shift_down().test(6, 5));
    }

    #[test]
    fn test_edge_wrap_prevention() {
        // Bit na kolumnie 0 po shift_right nie powinien zawinąć
        let mut b = BitBoard::new();
        b.set(0, 0);
        let sr = b.shift_right();
        assert!(sr.test(0, 1));
        assert!(!sr.test(0, 0));

        // Bit na kolumnie 9 po shift_left nie powinien zawinąć
        let mut b2 = BitBoard::new();
        b2.set(0, 9);
        let sl = b2.shift_left();
        assert!(sl.test(0, 8));
        assert!(!sl.test(0, 9));
    }

    #[test]
    fn test_dilate() {
        let mut b = BitBoard::new();
        b.set(5, 5);
        let d = b.dilate8();
        assert_eq!(d.popcount(), 9); // środek + 8 sąsiadów
    }

    #[test]
    fn test_mask_100() {
        assert_eq!(MASK_100.count_ones(), 100);
        let b = BitBoard::FULL;
        assert_eq!(b.popcount(), 100);
    }

    #[test]
    fn test_column_masks() {
        // COL_LEFT powinno mieć 10 bitów (po 1 w każdym wierszu, kolumna 0)
        assert_eq!(COL_LEFT.count_ones(), 10, "COL_LEFT must have 10 bits");
        // Sprawdź konkretne bity
        for r in 0..10 {
            assert_ne!(COL_LEFT & (1u128 << (r * 10)), 0, "COL_LEFT missing bit {}", r * 10);
        }
        // COL_RIGHT powinno mieć 10 bitów (po 1 w każdym wierszu, kolumna 9)
        assert_eq!(COL_RIGHT.count_ones(), 10, "COL_RIGHT must have 10 bits");
        for r in 0..10 {
            assert_ne!(COL_RIGHT & (1u128 << (r * 10 + 9)), 0, "COL_RIGHT missing bit {}", r * 10 + 9);
        }
    }

    #[test]
    fn test_dilate_corner_ship() {
        // Statek w kolumnie 9, wiersze 4-7 (pionowy, len 4)
        let mut b = BitBoard::new();
        b.set(4, 9);
        b.set(5, 9);
        b.set(6, 9);
        b.set(7, 9);
        let d = b.dilate8();
        // Dilate powinno obejmować wiersze 3-8, kolumny 8-9 (bez kolumny 10 - OOB)
        // Sprawdź czy bit 90 (wiersz 9, kolumna 0) NIE jest ustawiony - to byłby wrap-around
        assert!(!d.test(9, 0), "Dilate8 wraparound bug: (9,0) set for ship at col 9");
        // Sprawdź czy sąsiednie pola są ustawione
        assert!(d.test(3, 8));
        assert!(d.test(3, 9));
        assert!(d.test(8, 8));
        assert!(d.test(8, 9));
    }
}
