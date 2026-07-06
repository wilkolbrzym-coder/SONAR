//! Micro-learning database — persistent JSON storage of finished games.
//!
//! Sonar can optionally record every finished game into a JSON file and
//! use the accumulated history to bias future placement and targeting
//! decisions. The file lives at `~/.local/share/sonar/learning.json`
//! (override with `$SONAR_LEARNING_PATH`).

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

/// The full learning database.
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
            std::fs::create_dir_all(parent)?;
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

    /// Per-cell targeting bias derived from historical hits.
    ///
    /// `decay` is the exponential decay factor applied to older games
    /// (e.g. `0.92` means each older game contributes 92% as much as the
    /// one after it).
    pub fn targeting_bias(&self, decay: f32) -> [f32; 100] {
        let mut m = [0.0f32; 100];
        if self.games.is_empty() {
            return m;
        }
        let mut weight = 1.0f32;
        let mut total_weight = 0.0f32;
        for g in self.games.iter().rev() {
            for &(pos, hit) in &g.my_shots {
                if hit {
                    m[pos as usize] += weight;
                }
            }
            total_weight += weight;
            weight *= decay;
            if weight < 1e-3 {
                break;
            }
        }
        if total_weight > 0.0 {
            for v in m.iter_mut() {
                *v /= total_weight;
            }
        }
        m
    }

    /// Top-N fleet placement patterns by win rate.
    ///
    /// Returns `(fleet_mask_string, win_rate, sample_size)` for each pattern
    /// that has at least 2 recorded games.
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
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(top_n);
        v
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Global DB — used by the default learning path.
// ─────────────────────────────────────────────────────────────────────────────

static GLOBAL_DB: Mutex<Option<LearningDB>> = Mutex::new(None);
static DB_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Default file path for the learning database.
pub fn default_path() -> PathBuf {
    if let Some(p) = DB_PATH.get() {
        return p.clone();
    }
    let p = std::env::var("SONAR_LEARNING_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if let Ok(home) = std::env::var("HOME") {
                let p = PathBuf::from(home).join(".local/share/sonar/learning.json");
                if std::fs::create_dir_all(p.parent().unwrap()).is_ok() {
                    return p;
                }
            }
            PathBuf::from("sonar-learning.json")
        });
    let _ = DB_PATH.set(p.clone());
    p
}

/// Like [`default_path`] but returns `None` if neither `$SONAR_LEARNING_PATH`
/// nor `$HOME` is set.
pub fn default_path_opt() -> Option<PathBuf> {
    if std::env::var("SONAR_LEARNING_PATH").is_ok() || std::env::var("HOME").is_ok() {
        Some(default_path())
    } else {
        None
    }
}

/// Borrow the global learning DB (lazy-initialised from disk on first access).
pub fn global() -> std::sync::MutexGuard<'static, Option<LearningDB>> {
    let mut g = GLOBAL_DB.lock().unwrap();
    if g.is_none() {
        let path = default_path();
        *g = Some(LearningDB::load(&path));
    }
    g
}

/// Force-save the global DB to disk.
pub fn save_global() -> std::io::Result<()> {
    let path = default_path();
    let g = GLOBAL_DB.lock().unwrap();
    if let Some(db) = g.as_ref() {
        db.save(&path)
    } else {
        Ok(())
    }
}

/// Append a game record to the global DB and persist to disk.
pub fn record_game(rec: GameRecord) {
    let mut g = GLOBAL_DB.lock().unwrap();
    if g.is_none() {
        let path = default_path();
        *g = Some(LearningDB::load(&path));
    }
    if let Some(db) = g.as_mut() {
        db.record(rec);
    }
    drop(g);
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
    fn test_targeting_bias() {
        let mut db = LearningDB::new();
        db.record(GameRecord {
            my_fleet_mask: "0".to_string(),
            my_shots: vec![(0, true), (1, false)],
            won: true,
            moves: 5,
            fleet_lengths: vec![2],
            timestamp: 1,
        });
        db.record(GameRecord {
            my_fleet_mask: "0".to_string(),
            my_shots: vec![(5, true)],
            won: false,
            moves: 3,
            fleet_lengths: vec![2],
            timestamp: 2,
        });
        let bias = db.targeting_bias(0.9);
        assert!(bias[0] > 0.0);
        assert!(bias[5] > 0.0);
        assert!(bias[5] > bias[0]); // most recent game has higher weight
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
        assert_eq!(patterns[0].0, "0x10"); // 100% win rate wins
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
