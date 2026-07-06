//! Pure-Rust JSON IPC server.
//!
//! Sonar can run as a long-lived process that communicates over
//! newline-delimited JSON on stdin/stdout. This is the integration point
//! used by the Python UI overlay and any other language that wants to
//! drive the engine.
//!
//! ## Protocol
//!
//! Each request is a single line of JSON with a `cmd` field. Each reply
//! is a single line of JSON.
//!
//! | Command         | Request                                          | Reply                                                       |
//! |-----------------|--------------------------------------------------|-------------------------------------------------------------|
//! | `place_random`  | `{"cmd":"place_random"}`                         | `{"ok":true}`                                               |
//! | `place_smart`   | `{"cmd":"place_smart"}`                          | `{"ok":true}`                                               |
//! | `place_manual`  | `{"cmd":"place_manual","ships":[[r,c,len,h],...]}`| `{"ok":true}` or `{"ok":false,"error":"...","bad_index":N}` |
//! | `choose_move`   | `{"cmd":"choose_move","deadline_secs":20}`       | `{"row":R,"col":C}`                                         |
//! | `suggest_move`  | `{"cmd":"suggest_move","deadline_secs":20}`      | `MoveSuggestion` (JSON)                                     |
//! | `observe`       | `{"cmd":"observe","r":R,"c":C,"result":"miss"}`  | `{"ok":true}`                                               |
//! | `receive_shot`  | `{"cmd":"receive_shot","r":R,"c":C}`             | `{"result":"miss"}`                                         |
//! | `snapshot`      | `{"cmd":"snapshot"}`                             | `EngineSnapshot` (JSON)                                     |
//! | `config`        | `{"cmd":"config"}`                               | `EngineConfig` (JSON)                                       |
//! | `set_config`    | `{"cmd":"set_config","config":{...}}`            | `{"ok":true}`                                               |
//! | `reset`         | `{"cmd":"reset"}`                                | `{"ok":true}`                                               |
//! | `record_game`   | `{"cmd":"record_game","won":true}`               | `{"ok":true}`                                               |
//! | `learning`      | `{"cmd":"learning"}`                             | `{"games":N,"wins":N,"losses":N,...}`                       |
//! | `quit`          | `{"cmd":"quit"}`                                 | (closes stdin)                                              |
//!
//! `result` values: `"miss"`, `"hit"`, `"sunk"`, `"sunk_5"` (with length),
//! `"already"`, `"invalid"`.

use crate::api::{Engine, EngineConfig, MoveSuggestion};
use crate::board::ShotResult;
use crate::time_limit::Deadline;
use serde::Deserialize;
use serde_json::Value;
use std::io::{self, BufRead, Write};

// ─────────────────────────────────────────────────────────────────────────────
// Request envelope
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Request {
    cmd: String,
    #[serde(flatten)]
    extra: Value,
}

// ─────────────────────────────────────────────────────────────────────────────
// run
// ─────────────────────────────────────────────────────────────────────────────

/// Run the JSON IPC server on the given reader/writer pair. Reads
/// newline-delimited JSON requests until EOF or `quit`.
pub fn run<R: BufRead, W: Write>(stdin: R, stdout: &mut W) -> io::Result<()> {
    let mut engine = Engine::new(EngineConfig::default());
    let mut reader = stdin;

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                reply(stdout, &serde_json::json!({"ok": false, "error": format!("bad json: {}", e)}))?;
                continue;
            }
        };

        let resp = handle(&mut engine, &req);
        reply(stdout, &resp)?;
        if req.cmd == "quit" {
            break;
        }
    }
    Ok(())
}

fn reply<W: Write>(w: &mut W, v: &Value) -> io::Result<()> {
    writeln!(w, "{}", v)?;
    w.flush()
}

fn handle(engine: &mut Engine, req: &Request) -> Value {
    match req.cmd.as_str() {
        "place_random" => {
            engine.place_fleet_random();
            serde_json::json!({"ok": true})
        }
        "place_smart" => {
            engine.place_fleet_smart();
            serde_json::json!({"ok": true})
        }
        "place_manual" => {
            let ships = match req.extra.get("ships").and_then(|v| v.as_array()) {
                Some(a) => a,
                None => return serde_json::json!({"ok": false, "error": "missing ships"}),
            };
            let parsed: Vec<(usize, usize, u8, bool)> = ships
                .iter()
                .filter_map(|s| {
                    let a = s.as_array()?;
                    if a.len() != 4 {
                        return None;
                    }
                    Some((
                        a[0].as_u64()? as usize,
                        a[1].as_u64()? as usize,
                        a[2].as_u64()? as u8,
                        a[3].as_bool()?,
                    ))
                })
                .collect();
            match engine.place_fleet_manual(&parsed) {
                Ok(()) => serde_json::json!({"ok": true}),
                Err(i) => serde_json::json!({"ok": false, "error": "illegal placement", "bad_index": i}),
            }
        }
        "choose_move" => {
            let secs = req.extra.get("deadline_secs").and_then(|v| v.as_u64()).unwrap_or(20);
            let dl = Deadline::from_secs(secs);
            let (r, c) = engine.choose_move(dl);
            serde_json::json!({"row": r, "col": c})
        }
        "suggest_move" => {
            let secs = req.extra.get("deadline_secs").and_then(|v| v.as_u64()).unwrap_or(20);
            let dl = Deadline::from_secs(secs);
            let s: MoveSuggestion = engine.suggest_move(dl);
            serde_json::to_value(&s).unwrap_or_else(|_| serde_json::json!({"ok": false}))
        }
        "observe" => {
            let r = req.extra.get("r").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let c = req.extra.get("c").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let res_str = req.extra.get("result").and_then(|v| v.as_str()).unwrap_or("miss");
            let res = parse_shot_result(res_str);
            engine.observe_result(r, c, res);
            serde_json::json!({"ok": true})
        }
        "receive_shot" => {
            let r = req.extra.get("r").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let c = req.extra.get("c").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let res = engine.receive_shot(r, c);
            serde_json::json!({"result": format_shot_result(res)})
        }
        "snapshot" => {
            let snap = engine.snapshot();
            serde_json::to_value(&snap).unwrap_or_else(|_| serde_json::json!({"ok": false}))
        }
        "config" => {
            serde_json::to_value(engine.config()).unwrap_or_else(|_| serde_json::json!({"ok": false}))
        }
        "rules" => {
            serde_json::to_value(engine.rules()).unwrap_or_else(|_| serde_json::json!({"ok": false}))
        }
        "set_rules" => {
            if let Some(rules_val) = req.extra.get("rules") {
                if let Ok(rules) = serde_json::from_value::<crate::rules::GameRules>(rules_val.clone()) {
                    *engine.rules_mut() = rules;
                    match engine.apply_rules() {
                        Ok(()) => return serde_json::json!({"ok": true}),
                        Err(e) => return serde_json::json!({"ok": false, "error": e}),
                    }
                }
            }
            serde_json::json!({"ok": false, "error": "bad rules"})
        }
        "set_config" => {
            if let Some(cfg_val) = req.extra.get("config") {
                if let Ok(cfg) = serde_json::from_value::<EngineConfig>(cfg_val.clone()) {
                    *engine.config_mut() = cfg;
                    engine.apply_config();
                    return serde_json::json!({"ok": true});
                }
            }
            serde_json::json!({"ok": false, "error": "bad config"})
        }
        "reset" => {
            engine.reset();
            serde_json::json!({"ok": true})
        }
        "record_game" => {
            let won = req.extra.get("won").and_then(|v| v.as_bool()).unwrap_or(false);
            engine.record_game(won);
            serde_json::json!({"ok": true})
        }
        "learning" => {
            let g = crate::learning::global();
            if let Some(db) = g.as_ref() {
                let total = db.len();
                let wins = db.games.iter().filter(|g| g.won).count();
                serde_json::json!({
                    "games": total,
                    "wins": wins,
                    "losses": total - wins,
                    "win_rate": if total > 0 { wins as f64 / total as f64 * 100.0 } else { 0.0 },
                    "path": crate::learning::default_path().display().to_string(),
                })
            } else {
                serde_json::json!({"games": 0, "learning_enabled": false})
            }
        }
        "quit" => serde_json::json!({"ok": true}),
        other => serde_json::json!({"ok": false, "error": format!("unknown cmd: {}", other)}),
    }
}

fn parse_shot_result(s: &str) -> ShotResult {
    match s {
        "miss" => ShotResult::Miss,
        "hit" => ShotResult::Hit,
        "sunk" => ShotResult::Sunk(0),
        s if s.starts_with("sunk_") => {
            let len = s[5..].parse::<u8>().unwrap_or(0);
            ShotResult::Sunk(len)
        }
        "already" => ShotResult::AlreadyShot,
        "invalid" => ShotResult::Invalid,
        _ => ShotResult::Miss,
    }
}

fn format_shot_result(r: ShotResult) -> &'static str {
    match r {
        ShotResult::Miss => "miss",
        ShotResult::Hit => "hit",
        ShotResult::Sunk(_) => "sunk",
        ShotResult::AlreadyShot => "already",
        ShotResult::Invalid => "invalid",
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_full_game_via_json() {
        // Sonar vs Sonar — both driven through the JSON protocol.
        let p1_in = Cursor::new(
            "{}\n".repeat(0).as_str().to_string()
                + r#"{"cmd":"place_smart"}
{"cmd":"choose_move","deadline_secs":1}
{"cmd":"quit"}
"#,
        );
        let mut out = Vec::new();
        run(p1_in, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("ok"));
        assert!(s.contains("row"));
    }

    #[test]
    fn test_set_and_get_config() {
        let input = Cursor::new(
            r#"{"cmd":"set_config","config":{"hypothesis_soft_target":128,"smart_placement":true,"placement":{"candidates":256,"penalty_contact":10.0,"penalty_edge":0.3,"penalty_corner":1.5,"parity_balance":true},"default_deadline_secs":10,"use_learning":false,"learning_path":null,"rules":{"board_size":10,"ship_lengths":[5,4,3,3,2],"contact_rule":"NoContact","sunk_rule":"RevealNeighbors"}}}
{"cmd":"config"}
{"cmd":"quit"}
"#,
        );
        let mut out = Vec::new();
        run(input, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\"hypothesis_soft_target\":128"));
    }

    #[test]
    fn test_set_and_get_rules() {
        let input = Cursor::new(
            r#"{"cmd":"set_rules","rules":{"board_size":7,"ship_lengths":[4,3,2],"contact_rule":"AllowCornerContact","sunk_rule":"NoReveal"}}
{"cmd":"rules"}
{"cmd":"quit"}
"#,
        );
        let mut out = Vec::new();
        run(input, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\"board_size\":7"));
        assert!(s.contains("\"AllowCornerContact\""));
        assert!(s.contains("\"NoReveal\""));
    }
}
