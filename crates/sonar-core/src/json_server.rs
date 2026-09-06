//! Pure-Rust JSON IPC server.
//!
//! Sonar runs as a long-lived process communicating over newline-delimited
//! JSON on stdin/stdout. This is the integration point used by external
//! UIs, tools, and any language that wants to drive the engine. The same
//! command handler ([`handle_request`]) also powers the WebAssembly build,
//! so the browser app speaks the exact same protocol as the CLI.
//!
//! ## Protocol (version 1)
//!
//! Each request is a single line of JSON with a `cmd` field. Each reply is
//! a single line of JSON.
//!
//! | Command           | Request                                            | Reply                                                        |
//! |-------------------|----------------------------------------------------|--------------------------------------------------------------|
//! | `version`         | `{"cmd":"version"}`                                | `{"name":"sonar","version":"…","channel":"beta",…}`          |
//! | `place_random`    | `{"cmd":"place_random"}`                           | `{"ok":true}`                                                |
//! | `place_smart`     | `{"cmd":"place_smart"}`                            | `{"ok":true}`                                                |
//! | `place_manual`    | `{"cmd":"place_manual","ships":[[r,c,len,h],…]}`   | `{"ok":true}` or `{"ok":false,"error":"…","bad_index":N}`    |
//! | `choose_move`     | `{"cmd":"choose_move","deadline_secs":20}`         | `{"row":R,"col":C}`                                          |
//! | `suggest_move`    | `{"cmd":"suggest_move","deadline_secs":20}`        | `MoveSuggestion` (JSON)                                      |
//! | `observe`         | `{"cmd":"observe","r":R,"c":C,"result":"miss"}`    | `{"ok":true}`                                                |
//! | `receive_shot`    | `{"cmd":"receive_shot","r":R,"c":C}`               | `{"result":"miss"}`                                          |
//! | `snapshot`        | `{"cmd":"snapshot"}`                               | `EngineSnapshot` (JSON)                                      |
//! | `probability`     | `{"cmd":"probability"}`                            | `{"matrix":[…100…],"hypothesis_count":N}`                    |
//! | `density`         | `{"cmd":"density"}`                                | `{"matrix":[…100…]}`                                         |
//! | `config`          | `{"cmd":"config"}`                                 | `EngineConfig` (JSON)                                        |
//! | `set_config`      | `{"cmd":"set_config","config":{…}}`                | `{"ok":true}`                                                |
//! | `rules`           | `{"cmd":"rules"}`                                  | `GameRules` (JSON)                                           |
//! | `set_rules`       | `{"cmd":"set_rules","rules":{…}}`                  | `{"ok":true}`                                                |
//! | `reseed`          | `{"cmd":"reseed","seed":123}`                      | `{"ok":true}`                                                |
//! | `new_game`        | `{"cmd":"new_game","seed":123}`                    | `{"ok":true}`                                                |
//! | `reset`           | `{"cmd":"reset"}`                                  | `{"ok":true}`                                                |
//! | `record_game`     | `{"cmd":"record_game","won":true}`                 | `{"ok":true}`                                                |
//! | `learning`        | `{"cmd":"learning"}`                               | `{"games":N,"wins":N,"losses":N,…}`                          |
//! | `bench`           | `{"cmd":"bench","games":20,"opponent":"random"}`   | benchmark summary (JSON)                                     |
//! | `variant_list`   | `{"cmd":"variant_list"}`                           | `{"presets":[…]}` (0.4)                                      |
//! | `variant_new`    | `{"cmd":"variant_new","preset":"torus8","seed":S}` | `{"ok":true,"width":…}` (0.4)                                |
//! | `variant_move`   | `{"cmd":"variant_move"}`                           | `{"row":R,"col":C,"density":[…]}` (0.4)                      |
//! | `variant_fire`   | `{"cmd":"variant_fire","r":R,"c":C}`               | `{"result":"hit","len":N}` (0.4)                            |
//! | `variant_state`  | `{"cmd":"variant_state"}`                          | `GeneralSnapshot` (0.4)                                      |
//! | `variant_feasible`| `{"cmd":"variant_feasible"}`                      | `{"feasible":true,"configs":N}` (0.4)                       |
//! | `variant_play`   | `{"cmd":"variant_play","seed":S}`                  | `{"shots":N,"won":true}` (0.4)                               |
//! | `quit`            | `{"cmd":"quit"}`                                   | (closes stdin)                                               |
//!
//! `result` values: `"miss"`, `"hit"`, `"sunk"`, `"sunk_5"` (with length),
//! `"already"`, `"invalid"`.
//!
//! Evolution policy: the wire format is **additive-only**. New fields and
//! new commands may appear; existing ones never change meaning (see
//! `STABILITY.md`).

use crate::api::{Engine, EngineConfig, MoveSuggestion};
use crate::board::ShotResult;
use crate::time_limit::Deadline;
use serde::Deserialize;
use serde_json::Value;
use std::io::{self, BufRead, Write};

// ─────────────────────────────────────────────────────────────────────────────
// Request envelope
// ─────────────────────────────────────────────────────────────────────────────

/// The protocol version of the JSON IPC interface.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
pub struct Request {
    cmd: String,
    #[serde(flatten)]
    extra: Value,
}

// ─────────────────────────────────────────────────────────────────────────────
// Command handler (shared by stdio server and WASM exports)
// ─────────────────────────────────────────────────────────────────────────────

/// Parse one request line and dispatch it against `engine`.
///
/// This is the single source of truth for the JSON protocol — the stdio
/// server, the WebAssembly exports, and the integration tests all funnel
/// through here.
pub fn handle_line(engine: &mut Engine, line: &str) -> Value {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return serde_json::json!({"ok": false, "error": "empty request"});
    }
    let req: Request = match serde_json::from_str(trimmed) {
        Ok(r) => r,
        Err(e) => {
            return serde_json::json!({"ok": false, "error": format!("bad json: {}", e)});
        }
    };
    handle_request(engine, &req)
}

/// Dispatch a parsed request. Public so embedders (WASM, tests) can drive
/// the engine with structured values instead of strings.
pub fn handle_request(engine: &mut Engine, req: &Request) -> Value {
    // 0.4: the variant family drives the generalised engine through its
    // own thread-local state and never touches the classic engine.
    if crate::variant_server::is_variant_cmd(&req.cmd) {
        return crate::variant_server::handle(&req.cmd, &req.extra);
    }
    match req.cmd.as_str() {
        "version" => serde_json::json!({
            "name": "sonar",
            "version": env!("CARGO_PKG_VERSION"),
            "channel": "beta",
            "protocol": PROTOCOL_VERSION,
            "language": "rust",
            "features": [
                "pdf-density",
                "bayesian-hypotheses",
                "exact-census-endgame",
                "constraint-dispersal-placement",
                "generalised-boards",
                "polyomino-ships",
                "torus-holes-variants",
                "feasibility-solver",
                "simd-kernels",
                "sprt-ladders",
                "json-ipc",
                "wasm",
            ],
        }),
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
                Err(i) => {
                    serde_json::json!({"ok": false, "error": "illegal placement", "bad_index": i})
                }
            }
        }
        "choose_move" => {
            let dl = match parse_deadline(&req.extra) {
                Ok(dl) => dl,
                Err(e) => return e,
            };
            let (r, c) = engine.choose_move(dl);
            serde_json::json!({"row": r, "col": c})
        }
        "suggest_move" => {
            let dl = match parse_deadline(&req.extra) {
                Ok(dl) => dl,
                Err(e) => return e,
            };
            let s: MoveSuggestion = engine.suggest_move(dl);
            serde_json::to_value(&s).unwrap_or_else(|_| serde_json::json!({"ok": false}))
        }
        "observe" => {
            // Strict validation: a malformed observation must never
            // corrupt the engine state. r/c must name a cell, and the
            // result must be a protocol result string (miss | hit |
            // sunk | sunk_<len> | already | invalid).
            let r = match req.extra.get("r").and_then(|v| v.as_u64()) {
                Some(r) if r < 10 => r as usize,
                _ => {
                    return serde_json::json!({
                        "ok": false,
                        "error": "observe: r out of range [0, 10)"
                    });
                }
            };
            let c = match req.extra.get("c").and_then(|v| v.as_u64()) {
                Some(c) if c < 10 => c as usize,
                _ => {
                    return serde_json::json!({
                        "ok": false,
                        "error": "observe: c out of range [0, 10)"
                    });
                }
            };
            let res_str = match req.extra.get("result").and_then(|v| v.as_str()) {
                Some(s) if is_valid_result_string(s) => s,
                _ => {
                    return serde_json::json!({
                        "ok": false,
                        "error": "observe: unknown result (miss | hit | sunk | sunk_<len>)"
                    });
                }
            };
            let res = parse_shot_result(res_str);
            engine.observe_result(r, c, res);
            serde_json::json!({"ok": true})
        }
        "receive_shot" => {
            let r = req.extra.get("r").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let c = req.extra.get("c").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let res = engine.receive_shot(r, c);
            // NOTE: `len` is an additive field (protocol v1 evolution) so
            // clients can observe "sunk_<len>" precisely. Without it, a
            // wire-driven game cannot reconstruct sunk ships correctly.
            let mut v = serde_json::json!({"result": format_shot_result(res)});
            if let ShotResult::Sunk(len) = res {
                v["len"] = serde_json::json!(len);
            }
            v
        }
        "snapshot" => {
            let snap = engine.snapshot();
            serde_json::to_value(&snap).unwrap_or_else(|_| serde_json::json!({"ok": false}))
        }
        "probability" => {
            let m = engine.probability_matrix();
            serde_json::json!({
                "matrix": m.to_vec(),
                "hypothesis_count": engine.hypothesis_count(),
            })
        }
        "density" => {
            let m = engine.density_matrix();
            serde_json::json!({"matrix": m.to_vec()})
        }
        "config" => serde_json::to_value(engine.config())
            .unwrap_or_else(|_| serde_json::json!({"ok": false})),
        "rules" => serde_json::to_value(engine.rules())
            .unwrap_or_else(|_| serde_json::json!({"ok": false})),
        "set_rules" => {
            if let Some(rules_val) = req.extra.get("rules")
                && let Ok(rules) =
                    serde_json::from_value::<crate::rules::GameRules>(rules_val.clone())
            {
                *engine.rules_mut() = rules;
                match engine.apply_rules() {
                    Ok(()) => return serde_json::json!({"ok": true}),
                    Err(e) => return serde_json::json!({"ok": false, "error": e}),
                }
            }
            serde_json::json!({"ok": false, "error": "bad rules"})
        }
        "set_config" => {
            if let Some(cfg_val) = req.extra.get("config")
                && let Ok(cfg) = serde_json::from_value::<EngineConfig>(cfg_val.clone())
            {
                *engine.config_mut() = cfg;
                engine.apply_config();
                return serde_json::json!({"ok": true});
            }
            serde_json::json!({"ok": false, "error": "bad config"})
        }
        "reseed" => {
            let seed = req.extra.get("seed").and_then(|v| v.as_u64()).unwrap_or(0);
            engine.reseed(seed);
            serde_json::json!({"ok": true})
        }
        "new_game" => {
            // Convenience: reset + optional reseed + optional fresh config,
            // in one atomic command (used by the web client).
            let seed = req.extra.get("seed").and_then(|v| v.as_u64());
            if let Some(cfg_val) = req.extra.get("config")
                && let Ok(cfg) = serde_json::from_value::<EngineConfig>(cfg_val.clone())
            {
                *engine.config_mut() = cfg;
                engine.apply_config();
            }
            engine.reset();
            if let Some(s) = seed {
                engine.reseed(s);
            }
            serde_json::json!({"ok": true, "seed": seed})
        }
        "reset" => {
            engine.reset();
            serde_json::json!({"ok": true})
        }
        "record_game" => {
            let won = req
                .extra
                .get("won")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
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
                    "avg_moves": db.avg_moves(),
                    "path": crate::learning::default_path().display().to_string(),
                })
            } else {
                serde_json::json!({"games": 0, "learning_enabled": false})
            }
        }
        "bench" => {
            let games = req
                .extra
                .get("games")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .min(10_000) as u32;
            let opponent = req
                .extra
                .get("opponent")
                .and_then(|v| v.as_str())
                .unwrap_or("random");
            let seed = req
                .extra
                .get("seed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0x5EED_0000_0000_0001);
            let soft = req
                .extra
                .get("soft_target")
                .and_then(|v| v.as_u64())
                .unwrap_or(256) as usize;
            let opp_kind = match opponent {
                "random" => crate::benchmark::BotKind::Random,
                "pdf" => crate::benchmark::BotKind::Pdf,
                "self" | "hybrid" => crate::benchmark::BotKind::Hybrid,
                other => {
                    return serde_json::json!({"ok": false,
                        "error": format!("unknown opponent '{}'", other)});
                }
            };
            let cfg = crate::benchmark::BenchmarkConfig {
                games,
                threads: 1,
                max_hypotheses: soft,
                smart_placement: true,
                seed,
            };
            let report = crate::benchmark::run_benchmark_seq(
                &cfg,
                crate::benchmark::BotKind::Hybrid,
                opp_kind,
            );
            let s1 = &report.stats1;
            let s2 = &report.stats2;
            let (lo, hi) = s1.win_rate_wilson_95();
            serde_json::json!({
                "ok": true,
                "games": s1.games(),
                "seed": seed,
                "sonar": {
                    "wins": s1.wins,
                    "win_rate": s1.win_rate(),
                    "wilson95": [lo * 100.0, hi * 100.0],
                    "avg_moves": s1.avg_moves(),
                    "moves_stderr": s1.moves_stderr(),
                },
                "opponent": {
                    "kind": opponent,
                    "wins": s2.wins,
                    "win_rate": s2.win_rate(),
                    "avg_moves": s2.avg_moves(),
                },
                "elapsed_ms": report.elapsed_us / 1000,
            })
        }
        "quit" => serde_json::json!({"ok": true}),
        other => serde_json::json!({"ok": false, "error": format!("unknown cmd: {}", other)}),
    }
}

/// Parse the `deadline_secs` request field into a Deadline.
///
/// Strict validation (hostile-input hardening): a *present but invalid*
/// field is an error, not a silent 20-second default; and a valid but
/// absurd value is clamped to 24 h so `Instant + Duration` arithmetic
/// can never overflow regardless of what the client sends.
fn parse_deadline(extra: &Value) -> Result<Deadline, Value> {
    const MAX_DEADLINE_SECS: u64 = 24 * 60 * 60;
    match extra.get("deadline_secs") {
        None => Ok(Deadline::from_secs(20)),
        Some(v) => {
            let Some(secs) = v.as_u64() else {
                return Err(serde_json::json!({
                    "ok": false,
                    "error": "deadline_secs must be a non-negative integer"
                }));
            };
            Ok(Deadline::from_secs(secs.min(MAX_DEADLINE_SECS)))
        }
    }
}

/// Is this a valid observe result string? ("sunk_<len>" with a
/// 1..=u8::MAX length counts; anything else is rejected.)
fn is_valid_result_string(s: &str) -> bool {
    matches!(s, "miss" | "hit" | "sunk" | "already" | "invalid")
        || (s.starts_with("sunk_") && s[5..].parse::<u8>().is_ok())
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
// stdio transport
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
        if line.trim().is_empty() {
            continue;
        }
        let resp = handle_line(&mut engine, &line);
        reply(stdout, &resp)?;
        // Robust quit detection: check the parsed command, not raw text.
        let is_quit = serde_json::from_str::<Value>(line.trim())
            .map(|v| v.get("cmd").and_then(|c| c.as_str()) == Some("quit"))
            .unwrap_or(false);
        if is_quit {
            break;
        }
    }
    Ok(())
}

fn reply<W: Write>(w: &mut W, v: &Value) -> io::Result<()> {
    writeln!(w, "{}", v)?;
    w.flush()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_deadline_strict_validation() {
        // Hostile-input hardening: garbage deadlines are errors (not a
        // silent 20 s wall-clock run), and huge-but-valid values are
        // clamped so Instant arithmetic can never overflow.
        let mut e = crate::api::Engine::new(crate::api::EngineConfig::default());
        e.place_fleet_smart();

        for (line, why) in [
            (
                r#"{"cmd":"choose_move","deadline_secs":-5}"#,
                "negative deadline",
            ),
            (
                r#"{"cmd":"choose_move","deadline_secs":1e18}"#,
                "float deadline",
            ),
            (
                r#"{"cmd":"suggest_move","deadline_secs":"soon"}"#,
                "string deadline",
            ),
        ] {
            let v = handle_line(&mut e, line);
            assert_eq!(v["ok"], false, "{} should be rejected: {}", why, line);
        }

        // A valid but absurd deadline is clamped — must answer instantly,
        // never panic on Instant overflow.
        let v = handle_line(
            &mut e,
            r#"{"cmd":"choose_move","deadline_secs":18446744073709551615}"#,
        );
        assert!(v["row"].as_u64().is_some(), "clamped deadline move: {}", v);
    }

    #[test]
    fn test_observe_strict_validation() {
        // Malformed observations must be rejected without touching the
        // engine state (hardening — previously "r":-5 silently
        // observed a miss at (0,0) and "zzz" parsed as miss).
        let mut e = crate::api::Engine::new(crate::api::EngineConfig::default());
        e.place_fleet_smart();

        for (line, why) in [
            (
                r#"{"cmd":"observe","r":-5,"c":0,"result":"miss"}"#,
                "negative r",
            ),
            (
                r#"{"cmd":"observe","r":0,"c":99,"result":"hit"}"#,
                "c out of range",
            ),
            (
                r#"{"cmd":"observe","r":0,"c":0,"result":"zzz"}"#,
                "unknown result",
            ),
            (
                r#"{"cmd":"observe","result":"sunk_999"}"#,
                "sunk len overflow",
            ),
            (
                r#"{"cmd":"observe","result":"sunk_abc"}"#,
                "sunk len garbage",
            ),
            (
                r#"{"cmd":"observe","r":18446744073709551615,"c":0,"result":"miss"}"#,
                "u64 r",
            ),
        ] {
            let v = handle_line(&mut e, line);
            assert_eq!(v["ok"], false, "{} should be rejected: {}", why, line);
            assert!(
                v["error"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("observe:")),
                "error should name the field: {}",
                line
            );
        }

        // Valid observations still pass — and only the valid one mutated state.
        let before = e.snapshot().moves_fired;
        let v = handle_line(&mut e, r#"{"cmd":"observe","r":3,"c":3,"result":"sunk_2"}"#);
        assert_eq!(v["ok"], true);
        assert_eq!(e.snapshot().moves_fired, before + 1);
    }

    #[test]
    fn test_full_game_via_json() {
        // Sonar driven through the JSON protocol.
        let input = Cursor::new(
            r#"{"cmd":"new_game","seed":42,"config":{"use_learning":false,"hypothesis_soft_target":16}}
{"cmd":"place_smart"}
{"cmd":"choose_move","deadline_secs":1}
{"cmd":"quit"}
"#
            .to_string(),
        );
        let mut out = Vec::new();
        run(input, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\"ok\":true"));
        assert!(s.contains("row"));
    }

    #[test]
    fn test_version_reports_beta() {
        let mut engine = Engine::new(EngineConfig {
            use_learning: false,
            ..Default::default()
        });
        let v = handle_line(&mut engine, r#"{"cmd":"version"}"#);
        assert_eq!(v["name"], "sonar");
        assert_eq!(v["channel"], "beta");
        assert_eq!(v["protocol"], 1);
        assert!(v["version"].as_str().is_some_and(|s| s.contains("beta")));
    }

    #[test]
    fn test_set_and_get_config() {
        let input = Cursor::new(
            r#"{"cmd":"set_config","config":{"hypothesis_soft_target":128,"smart_placement":true,"placement":{"candidates":256,"sampling_epsilon":4.0,"penalty_contact":10.0,"penalty_edge":0.3,"penalty_corner":1.5,"parity_balance":true},"default_deadline_secs":10,"use_learning":false,"learning_path":null,"rules":{"board_size":10,"ship_lengths":[5,4,3,3,2],"contact_rule":"NoContact","sunk_rule":"RevealNeighbors"}}}
{"cmd":"config"}
{"cmd":"quit"}
"#
            .to_string(),
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
"#
            .to_string(),
        );
        let mut out = Vec::new();
        run(input, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\"board_size\":7"));
        assert!(s.contains("\"AllowCornerContact\""));
        assert!(s.contains("\"NoReveal\""));
    }

    #[test]
    fn test_probability_command() {
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            hypothesis_soft_target: 32,
            ..Default::default()
        });
        e.reseed(7);
        e.place_fleet_smart();
        let _ = e.choose_move(Deadline::none());
        let v = handle_line(&mut e, r#"{"cmd":"probability"}"#);
        let m = v["matrix"].as_array();
        assert!(m.is_some());
        assert_eq!(m.map(|a| a.len()), Some(100));
        assert!(v["hypothesis_count"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn test_bench_command() {
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            ..Default::default()
        });
        let v = handle_line(
            &mut e,
            r#"{"cmd":"bench","games":4,"opponent":"random","seed":123}"#,
        );
        assert_eq!(v["ok"], true);
        assert_eq!(v["games"], 4);
    }

    #[test]
    fn test_malformed_input_never_panics() {
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            ..Default::default()
        });
        let bad = [
            "",
            "null",
            "[]",
            "{",
            "garbage",
            "{\"cmd\":}",
            "{\"cmd\":123}",
            "{\"cmd\":\"observe\",\"r\":\"x\",\"c\":-1,\"result\":\"nonsense\"}",
            "{\"cmd\":\"place_manual\",\"ships\":[[0,0,\"x\",true]]}",
            "{\"cmd\":\"place_manual\",\"ships\":\"not-an-array\"}",
            "{\"cmd\":\"bench\",\"games\":\"lots\"}",
            "{\"cmd\":\"unknown_command\"}",
            "{\"cmd\":\"choose_move\",\"deadline_secs\":\"soon\"}",
        ];
        for input in bad {
            let v = handle_line(&mut e, input);
            // Every reply must be valid JSON with an "ok" or documented shape.
            assert!(v.is_object(), "reply not an object for {:?}", input);
        }
    }
}
