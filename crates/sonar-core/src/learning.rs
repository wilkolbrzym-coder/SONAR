//! Game statistics database — persistent JSON storage of finished games.
//!
//! ## What this is (and is not)
//!
//! Sonar records every finished game into a JSON file so players can
//! inspect their own statistics: win rates, fleet patterns, move counts.
//! The file lives at `~/.local/share/sonar/learning.json`
//! (override with `$SONAR_LEARNING_PATH`).
//!
//! **Records are passive statistics.** They are *never* used to bias
//! placement or targeting decisions. This is a deliberate design
//! constraint from Sonar's adversarial-robustness contract: an opponent
//! that collects thousands of games must not be able to train a predictor
//! on "what Sonar learned from past games", because there is no such
//! influence path in the code. The 0.1.0 "micro-learning bias" was removed
//! in 0.2.0 for exactly this reason.
//!
//! On WebAssembly builds the database stays in memory only (no filesystem).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ─────────────────────────────────────────────────────────────────────────────
// GameRecord / LearningDB
// ─────────────────────────────────────────────────────────────────────────────

/// A single finished-game record.
///
/// `my_fleet_mask` is serialised as a decimal string because `serde_json`
/// does not support `u128` natively.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameRecord {
    /// Bitmask of our fleet cells (100 bits packed in `u128`), as a decimal string.
    pub my_fleet_mask: String,
    /// Our shot sequence: `(cell_index, hit?)`.
    pub my_shots: Vec<(u8, bool)>,
    /// Did we win?
    pub won: bool,
    /// How many moves we fired.
    pub moves: u32,
    /// Fleet lengths we used (always `[5,4,3,3,2]` for standard rules).
    pub fleet_lengths: Vec<u8>,
    /// Unix timestamp (seconds).
    pub timestamp: u64,
}

/// The game statistics database.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LearningDB {
    /// Chronological list of recorded games.
    pub games: Vec<GameRecord>,
    /// Format version (currently 1).
    pub version: u32,
}

impl LearningDB {
    /// Create an empty database.
    pub fn new() -> Self {
        Self { games: Vec::new(), version: 1 }
    }

    /// Load from a JSON file. Returns an empty DB if the file does not
    /// exist or is unreadable.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| Self::new()),
            Err(_) => Self::new(),
        }
    }

    /// Save to a JSON file (pretty-printed).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let s = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, s)
    }

    /// Append a game record. The database is capped at 10 000 entries
    /// (FIFO eviction).
    pub fn record(&mut self, rec: GameRecord) {
        self.games.push(rec);
        if self.games.len() > 10_000 {
            let drain = self.games.len() - 10_000;
            self.games.drain(0..drain);
        }
    }

    /// Number of recorded games.
    pub fn len(&self) -> usize {
        self.games.len()
    }

    /// Is the database empty?
    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    /// Win rate over all recorded games (0.0–1.0).
    pub fn win_rate(&self) -> f64 {
        if self.games.is_empty() {
            return 0.0;
        }
        let wins = self.games.iter().filter(|g| g.won).count();
        wins as f64 / self.games.len() as f64
    }

    /// Average number of moves per game.
    pub fn avg_moves(&self) -> f64 {
        if self.games.is_empty() {
            return 0.0;
        }
        let total: u64 = self.games.iter().map(|g| g.moves as u64).sum();
        total as f64 / self.games.len() as f64
    }

    /// Top-N fleet placement patterns by win rate (passive statistics).
    ///
    /// Returns `(fleet_mask_string, win_rate, sample_size)` for each
    /// pattern with at least 2 recorded games.
    pub fn best_fleet_patterns(&self, top_n: usize) -> Vec<(String, f32, u32)> {
        use std::collections::HashMap;
        let mut stats: HashMap<String, (u32, u32)> = HashMap::new();
        for g in &self.games {
            let e = stats.entry(g.my_fleet_mask.clone()).or_insert((0, 0));
            if g.won {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
        let mut v: Vec<(String, f32, u32)> = stats
            .into_iter()
            .map(|(m, (w, l))| (m, w as f32 / (w + l).max(1) as f32, w + l))
            .filter(|(_, _, n)| *n >= 2)
            .collect();
        v.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.2.cmp(&a.2))
        });
        v.truncate(top_n);
        v
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Global DB — used by the default statistics path.
// ─────────────────────────────────────────────────────────────────────────────

static GLOBAL_DB: Mutex<Option<LearningDB>> = Mutex::new(None);
static DB_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Default file path for the statistics database.
///
/// On WebAssembly there is no filesystem, so a relative placeholder is
/// returned and persistence silently no-ops.
pub fn default_path() -> PathBuf {
    if let Some(p) = DB_PATH.get() {
        return p.clone();
    }
    let p = std::env::var("SONAR_LEARNING_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(not(target_arch = "wasm32"))]
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(".local/share/sonar/learning.json");
            }
            PathBuf::from("sonar-learning.json")
        });
    let _ = DB_PATH.set(p.clone());
    p
}

/// Like [`default_path`] but returns `None` when no sensible location
/// exists (no `$SONAR_LEARNING_PATH`, no `$HOME`, or a WASM build).
pub fn default_path_opt() -> Option<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        if std::env::var("SONAR_LEARNING_PATH").is_ok() {
            return Some(default_path());
        }
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if std::env::var("SONAR_LEARNING_PATH").is_ok() || std::env::var("HOME").is_ok() {
            Some(default_path())
        } else {
            None
        }
    }
}

/// Borrow the global statistics DB (lazy-loaded from disk on first access).
pub fn global() -> std::sync::MutexGuard<'static, Option<LearningDB>> {
    let mut g = GLOBAL_DB
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if g.is_none() {
        let path = default_path();
        *g = Some(LearningDB::load(&path));
    }
    g
}

/// Force-save the global DB to disk.
pub fn save_global() -> std::io::Result<()> {
    let path = default_path();
    let g = GLOBAL_DB
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(db) = g.as_ref() {
        db.save(&path)
    } else {
        Ok(())
    }
}

/// Append a game record to the global DB and persist to disk (best effort).
pub fn record_game(rec: GameRecord) {
    {
        let mut g = GLOBAL_DB
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if g.is_none() {
            let path = default_path();
            *g = Some(LearningDB::load(&path));
        }
        if let Some(db) = g.as_mut() {
            db.record(rec);
        }
    }
    let _ = save_global();
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        p.push(format!("sonar-test-{}", nonce));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = tempdir();
        let path = dir.join("learning.json");
        let mut db = LearningDB::new();
        db.record(GameRecord {
            my_fleet_mask: "0xff".to_string(),
            my_shots: vec![(0, true), (1, false)],
            won: true,
            moves: 17,
            fleet_lengths: vec![2, 3, 3, 4, 5],
            timestamp: 12345,
        });
        db.save(&path).unwrap();
        let loaded = LearningDB::load(&path);
        assert_eq!(loaded.games.len(), 1);
        assert_eq!(loaded.games[0].my_fleet_mask, "0xff");
        assert!(loaded.games[0].won);
        assert_eq!(loaded.games[0].moves, 17);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_win_rate_and_avg_moves() {
        let mut db = LearningDB::new();
        db.record(GameRecord {
            my_fleet_mask: "0".to_string(),
            my_shots: vec![],
            won: true,
            moves: 10,
            fleet_lengths: vec![],
            timestamp: 1,
        });
        db.record(GameRecord {
            my_fleet_mask: "0".to_string(),
            my_shots: vec![],
            won: false,
            moves: 20,
            fleet_lengths: vec![],
            timestamp: 2,
        });
        assert!((db.win_rate() - 0.5).abs() < 1e-9);
        assert!((db.avg_moves() - 15.0).abs() < 1e-9);
        assert_eq!(db.len(), 2);
    }

    #[test]
    fn test_best_fleet_patterns() {
        let mut db = LearningDB::new();
        for _ in 0..2 {
            db.record(GameRecord {
                my_fleet_mask: "0x10".to_string(),
                my_shots: vec![],
                won: true,
                moves: 10,
                fleet_lengths: vec![],
                timestamp: 0,
            });
        }
        db.record(GameRecord {
            my_fleet_mask: "0x20".to_string(),
            my_shots: vec![],
            won: true,
            moves: 10,
            fleet_lengths: vec![],
            timestamp: 0,
        });
        db.record(GameRecord {
            my_fleet_mask: "0x20".to_string(),
            my_shots: vec![],
            won: false,
            moves: 10,
            fleet_lengths: vec![],
            timestamp: 0,
        });
        db.record(GameRecord {
            my_fleet_mask: "0x20".to_string(),
            my_shots: vec![],
            won: false,
            moves: 10,
            fleet_lengths: vec![],
            timestamp: 0,
        });
        let patterns = db.best_fleet_patterns(10);
        assert!(!patterns.is_empty());
        assert_eq!(patterns[0].0, "0x10"); // the 100% win-rate pattern wins
    }

    #[test]
    fn test_load_missing_file() {
        let db = LearningDB::load(Path::new("/nonexistent/path/xyz.json"));
        assert!(db.is_empty());
    }

    #[test]
    fn test_size_limit() {
        let mut db = LearningDB::new();
        for i in 0..15_000 {
            db.record(GameRecord {
                my_fleet_mask: (i as u128).to_string(),
                my_shots: vec![],
                won: true,
                moves: 1,
                fleet_lengths: vec![],
                timestamp: i as u64,
            });
        }
        assert_eq!(db.games.len(), 10_000);
    }
}
