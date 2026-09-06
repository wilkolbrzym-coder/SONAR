//! Variant server (0.4): generalised-game commands on the JSON protocol.
//!
//! The `variant_*` command family drives the generalised engine
//! (arbitrary boards, polyomino fleets, torus/holes) through the same
//! newline-delimited JSON protocol as the classic engine — one protocol,
//! every runtime (CLI stdio, WebAssembly, the web app).
//!
//! ## Commands (protocol v1, additive)
//!
//! | Command           | Request                                          | Reply                                          |
//! |-------------------|--------------------------------------------------|------------------------------------------------|
//! | `variant_list`    | `{"cmd":"variant_list"}`                         | `{"presets":[…]}`                              |
//! | `variant_new`     | `{"cmd":"variant_new","preset":"torus8","seed":S}` or `{"cmd":"variant_new","rules":{…},"seed":S}` | `{"ok":true,"width":…,…}` |
//! | `variant_move`    | `{"cmd":"variant_move"}`                         | `{"row":R,"col":C,"density":[…]}`              |
//! | `variant_fire`    | `{"cmd":"variant_fire","r":R,"c":C}`             | `{"result":"hit","len":N}`                     |
//! | `variant_state`   | `{"cmd":"variant_state"}`                        | `GeneralSnapshot`                              |
//! | `variant_feasible`| `{"cmd":"variant_feasible"}`                     | `{"feasible":true,"configs":N,…}`              |
//! | `variant_play`    | `{"cmd":"variant_play","seed":S}`                | `{"shots":N,"won":true}`                       |
//!
//! The engine state is thread-local: one variant game at a time per
//! process (the web app is single-threaded by design). `variant_new`
//! resets it deterministically per seed.

use crate::general::{GeneralEngine, GeneralRules};
use crate::grid::Geometry;
use crate::polyomino::ShipSpec;
use crate::rng::Xoshiro256;
use serde_json::{Value, json};
use std::cell::RefCell;

thread_local! {
    static VARIANT: RefCell<Option<GeneralEngine>> = const { RefCell::new(None) };
    static RNG: RefCell<Xoshiro256> = RefCell::new(Xoshiro256::from_seed(0x5EED_0000_0000_0001));
}

/// Is this command handled by the variant server?
pub fn is_variant_cmd(cmd: &str) -> bool {
    cmd.starts_with("variant_")
}

/// Route a `variant_*` request. Returns a JSON reply; never panics.
pub fn handle(cmd: &str, extra: &Value) -> Value {
    match cmd {
        "variant_list" => json!({ "presets": presets_json() }),
        "variant_new" => handle_new(extra),
        "variant_move" => handle_move(),
        "variant_fire" => handle_fire(extra),
        "variant_state" => handle_state(),
        "variant_feasible" => handle_feasible(),
        "variant_play" => handle_play(extra),
        other => json!({"ok": false, "error": format!("unknown variant cmd: {}", other)}),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Presets
// ─────────────────────────────────────────────────────────────────────────────

/// A named preset variant.
pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub rules: GeneralRules,
}

/// The built-in preset variants.
pub fn presets() -> Vec<Preset> {
    use crate::rules::ContactRule::*;
    use crate::rules::SunkRule::*;
    let line = |len: u8| ShipSpec::Line { len };

    vec![
        Preset {
            name: "classic",
            description: "The standard 10x10 fleet: 5,4,3,3,2.",
            rules: GeneralRules {
                geometry: Geometry::rectangle(10, 10),
                fleet: vec![line(5), line(4), line(3), line(3), line(2)],
                contact_rule: NoContact,
                sunk_rule: RevealNeighbors,
            },
        },
        Preset {
            name: "micro",
            description: "A quick 7x7 skirmish: 4,3,2.",
            rules: GeneralRules {
                geometry: Geometry::rectangle(7, 7),
                fleet: vec![line(4), line(3), line(2)],
                contact_rule: NoContact,
                sunk_rule: RevealNeighbors,
            },
        },
        Preset {
            name: "big16",
            description: "A 16x16 ocean with a heavy fleet (narrow->wide tier boundary).",
            rules: GeneralRules {
                geometry: Geometry::rectangle(16, 16),
                fleet: vec![
                    line(6),
                    line(5),
                    line(5),
                    line(4),
                    line(4),
                    line(3),
                    line(3),
                    line(2),
                ],
                contact_rule: NoContact,
                sunk_rule: RevealNeighbors,
            },
        },
        Preset {
            name: "huge30",
            description: "The 30x30 flagship: twelve ships, wide-tier SIMD bitboards.",
            rules: GeneralRules {
                geometry: Geometry::rectangle(30, 30),
                fleet: vec![
                    line(6),
                    line(5),
                    line(5),
                    line(4),
                    line(4),
                    line(4),
                    line(3),
                    line(3),
                    line(3),
                    line(2),
                    line(2),
                    line(2),
                ],
                contact_rule: NoContact,
                sunk_rule: RevealNeighbors,
            },
        },
        Preset {
            name: "torus8",
            description: "An 8x8 donut: the edges wrap, nothing is a safe corner.",
            rules: GeneralRules {
                geometry: Geometry {
                    torus: true,
                    ..Geometry::rectangle(8, 8)
                },
                fleet: vec![line(5), line(4), line(3), line(2)],
                contact_rule: NoContact,
                sunk_rule: RevealNeighbors,
            },
        },
        Preset {
            name: "archipelago12",
            description: "A 12x12 map with an island chain (holes) blocking placements.",
            rules: GeneralRules {
                geometry: Geometry {
                    holes: vec![
                        (5, 5),
                        (5, 6),
                        (6, 5),
                        (6, 6),
                        (5, 8),
                        (6, 8),
                        (8, 3),
                        (8, 4),
                    ],
                    ..Geometry::rectangle(12, 12)
                },
                fleet: vec![line(5), line(4), line(3), line(3), line(2)],
                contact_rule: NoContact,
                sunk_rule: RevealNeighbors,
            },
        },
        Preset {
            name: "poly10",
            description: "A 10x10 polyomino fleet: L5, T4, S4, L3, O2.",
            rules: GeneralRules {
                geometry: Geometry::rectangle(10, 10),
                fleet: crate::polyomino::ShipShape::preset_l_fleet()
                    .into_iter()
                    .map(|s| ShipSpec::Shape {
                        cells: s.cells.iter().map(|&(r, c)| (r, c)).collect(),
                        name: Some(s.name.clone()),
                    })
                    .collect(),
                contact_rule: NoContact,
                sunk_rule: RevealNeighbors,
            },
        },
    ]
}

fn presets_json() -> Value {
    presets()
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "description": p.description,
                "width": p.rules.geometry.width,
                "height": p.rules.geometry.height,
                "torus": p.rules.geometry.torus,
                "holes": p.rules.geometry.holes.len(),
                "ships": p.rules.fleet.len(),
            })
        })
        .collect::<Vec<_>>()
        .into()
}

// ─────────────────────────────────────────────────────────────────────────────
// Command handlers
// ─────────────────────────────────────────────────────────────────────────────

fn handle_new(extra: &Value) -> Value {
    let seed = extra.get("seed").and_then(|v| v.as_u64());

    let rules: Option<GeneralRules> =
        if let Some(preset_name) = extra.get("preset").and_then(|v| v.as_str()) {
            presets()
                .into_iter()
                .find(|p| p.name == preset_name)
                .map(|p| p.rules)
        } else if let Some(rules_val) = extra.get("rules") {
            serde_json::from_value(rules_val.clone()).ok()
        } else {
            None
        };

    let Some(rules) = rules else {
        return json!({"ok": false, "error": "unknown preset or missing rules"});
    };

    let mut engine = match GeneralEngine::new(rules) {
        Ok(e) => e,
        Err(e) => return json!({"ok": false, "error": e}),
    };

    // Seed the RNG and place the hidden fleet.
    let placed = RNG.with(|rng| {
        if let Some(s) = seed {
            *rng.borrow_mut() = Xoshiro256::from_seed(s);
        }
        let mut rng = rng.borrow_mut();
        engine.place_fleet_random(&mut rng)
    });
    if !placed {
        return json!({"ok": false, "error": "fleet placement failed (too dense?)"});
    }

    // The state must live in the thread-local; return the summary.
    let summary = VARIANT.with(|v| {
        *v.borrow_mut() = Some(engine);
        v.borrow().as_ref().map(|e| {
            json!({
                "ok": true,
                "width": e.rules.geometry.width,
                "height": e.rules.geometry.height,
                "torus": e.rules.geometry.torus,
                "holes": e.rules.geometry.holes,
                "ships": e.rules.fleet.len(),
                "total_ship_cells": e.board.ships_mask.popcount(&e.rules.geometry),
                "simd_backend": sonar_simd::active_backend_name(),
            })
        })
    });
    summary.unwrap_or_else(|| json!({"ok": false, "error": "engine state error"}))
}

fn handle_move() -> Value {
    VARIANT.with(|v| {
        let Ok(mut guard) = v.try_borrow_mut() else {
            return json!({"ok": false, "error": "engine busy"});
        };
        let Some(engine) = guard.as_mut() else {
            return json!({"ok": false, "error": "no variant game — call variant_new first"});
        };
        match engine.choose_move() {
            Some((r, c)) => {
                let d = engine.density();
                json!({
                    "ok": true,
                    "row": r,
                    "col": c,
                    "density": d.cells,
                    "legal_counts": d.legal_counts,
                })
            }
            None => json!({"ok": false, "error": "no moves left"}),
        }
    })
}

fn handle_fire(extra: &Value) -> Value {
    let r = extra.get("r").and_then(|v| v.as_u64()).unwrap_or(u64::MAX) as usize;
    let c = extra.get("c").and_then(|v| v.as_u64()).unwrap_or(u64::MAX) as usize;
    VARIANT.with(|v| {
        let Ok(mut guard) = v.try_borrow_mut() else {
            return json!({"ok": false, "error": "engine busy"});
        };
        let Some(engine) = guard.as_mut() else {
            return json!({"ok": false, "error": "no variant game — call variant_new first"});
        };
        let g = engine.rules.geometry.clone();
        if r >= g.height || c >= g.width {
            return json!({"ok": false, "error": "out of bounds"});
        }
        let res = engine.fire(r, c);
        let mut reply = json!({
            "ok": true,
            "result": match res {
                crate::board::ShotResult::Miss => "miss",
                crate::board::ShotResult::Hit => "hit",
                crate::board::ShotResult::Sunk(_) => "sunk",
                crate::board::ShotResult::AlreadyShot => "already",
                crate::board::ShotResult::Invalid => "invalid",
            },
            "all_sunk": engine.board.all_sunk(),
        });
        if let crate::board::ShotResult::Sunk(len) = res {
            reply["len"] = json!(len);
        }
        reply
    })
}

fn handle_state() -> Value {
    VARIANT.with(|v| {
        let Ok(mut guard) = v.try_borrow_mut() else {
            return json!({"ok": false, "error": "engine busy"});
        };
        let Some(engine) = guard.as_mut() else {
            return json!({"ok": false, "error": "no variant game — call variant_new first"});
        };
        serde_json::to_value(engine.snapshot())
            .unwrap_or_else(|_| json!({"ok": false, "error": "snapshot failed"}))
    })
}

fn handle_feasible() -> Value {
    VARIANT.with(|v| {
        let Ok(guard) = v.try_borrow_mut() else {
            return json!({"ok": false, "error": "engine busy"});
        };
        let Some(engine) = guard.as_ref() else {
            return json!({"ok": false, "error": "no variant game — call variant_new first"});
        };
        let f = engine.feasibility();
        json!({
            "ok": true,
            "feasible": f.feasible,
            "configs": f.configs,
            "complete": f.complete,
        })
    })
}

fn handle_play(extra: &Value) -> Value {
    let seed = extra
        .get("seed")
        .and_then(|v| v.as_u64())
        .unwrap_or(0x5EED_0000_0000_0002);
    VARIANT.with(|v| {
        let Ok(mut guard) = v.try_borrow_mut() else {
            return json!({"ok": false, "error": "engine busy"});
        };
        let Some(engine) = guard.as_mut() else {
            return json!({"ok": false, "error": "no variant game — call variant_new first"});
        };
        let mut rng = Xoshiro256::from_seed(seed);
        let shots = engine.self_play(&mut rng);
        match shots {
            Some(n) => json!({"ok": true, "shots": n, "won": true, "seed": seed}),
            None => json!({"ok": false, "error": "game did not finish"}),
        }
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn req(cmd: &str) -> Value {
        handle(cmd, &json!({}))
    }

    fn req_with(cmd: &str, extra: Value) -> Value {
        handle(cmd, &extra)
    }

    #[test]
    fn test_variant_list() {
        let v = req("variant_list");
        let presets = v["presets"].as_array().unwrap();
        assert!(presets.len() >= 6);
        let names: Vec<&str> = presets.iter().filter_map(|p| p["name"].as_str()).collect();
        for expected in [
            "classic",
            "micro",
            "big16",
            "huge30",
            "torus8",
            "archipelago12",
            "poly10",
        ] {
            assert!(names.contains(&expected), "missing preset {}", expected);
        }
    }

    #[test]
    fn test_variant_new_preset_and_play() {
        let v = req_with("variant_new", json!({"preset": "micro", "seed": 42}));
        assert_eq!(v["ok"], true);
        assert_eq!(v["width"], 7);
        assert_eq!(v["ships"], 3);

        // A full self-play game through the protocol.
        let mut moves = 0;
        loop {
            let m = req("variant_move");
            assert_eq!(m["ok"], true, "move failed: {}", m);
            let r = m["row"].as_u64().unwrap() as usize;
            let c = m["col"].as_u64().unwrap() as usize;
            let f = req_with("variant_fire", json!({"r": r, "c": c}));
            assert_eq!(f["ok"], true);
            moves += 1;
            if f["all_sunk"] == true {
                break;
            }
            assert!(moves < 200, "game did not finish in 200 moves");
            if moves >= 200 {
                break;
            }
        }
        assert!(moves < 200);
        let s = req("variant_state");
        assert_eq!(s["moves_fired"], moves);
    }

    #[test]
    fn test_variant_new_custom_rules() {
        let v = req_with(
            "variant_new",
            json!({
                "rules": {
                    "geometry": {"width": 9, "height": 9, "holes": [], "torus": false},
                    "fleet": [
                        {"kind": "Line", "len": 4},
                        {"kind": "Line", "len": 3},
                        {"kind": "Shape", "cells": [[0,0],[1,0],[1,1]], "name": "L3"}
                    ],
                    "contact_rule": "NoContact",
                    "sunk_rule": "RevealNeighbors"
                },
                "seed": 7
            }),
        );
        assert_eq!(v["ok"], true, "{}", v);
        assert_eq!(v["width"], 9);
        assert_eq!(v["ships"], 3);
    }

    #[test]
    fn test_variant_new_bad_requests() {
        // Unknown preset.
        let v = req_with("variant_new", json!({"preset": "nonsense"}));
        assert_eq!(v["ok"], false);
        // Missing everything.
        let v = req("variant_new");
        assert_eq!(v["ok"], false);
        // Invalid rules.
        let v = req_with(
            "variant_new",
            json!({"rules": {"geometry": {"width": 3, "height": 3, "holes": [], "torus": false}, "fleet": []}}),
        );
        assert_eq!(v["ok"], false);
    }

    #[test]
    fn test_variant_commands_require_game() {
        for cmd in [
            "variant_move",
            "variant_state",
            "variant_feasible",
            "variant_fire",
            "variant_play",
        ] {
            // (A prior test may have left a game in the thread-local —
            // clear it first by testing on a fresh thread.)
            let v = std::thread::spawn(move || handle(cmd, &serde_json::json!({"r": 0, "c": 0})))
                .join();
            let v = v.unwrap_or_else(|_| serde_json::json!({"ok": false}));
            assert_eq!(v["ok"], false, "{} must require a game: {}", cmd, v);
        }
    }

    #[test]
    fn test_variant_feasible_command() {
        let _ = req_with("variant_new", json!({"preset": "micro", "seed": 9}));
        let v = req("variant_feasible");
        assert_eq!(v["ok"], true);
        assert_eq!(v["feasible"], true);
        assert!(v["configs"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_variant_play_command() {
        let _ = req_with("variant_new", json!({"preset": "micro", "seed": 3}));
        let v = req_with("variant_play", json!({"seed": 17}));
        assert_eq!(v["ok"], true, "{}", v);
        let shots = v["shots"].as_u64().unwrap();
        assert!((9..=60).contains(&shots), "shots: {}", shots);
    }

    #[test]
    fn test_is_variant_cmd() {
        assert!(is_variant_cmd("variant_new"));
        assert!(is_variant_cmd("variant_play"));
        assert!(!is_variant_cmd("new_game"));
        assert!(!is_variant_cmd("version"));
    }

    #[test]
    fn test_unknown_variant_cmd() {
        let v = req("variant_nonsense");
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("unknown variant"));
    }
}
