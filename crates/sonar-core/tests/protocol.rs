//! JSON protocol integration tests — full games over the wire, golden
//! response shapes, robustness against hostile inputs.
//!
//! These tests drive the exact protocol surface used by external
//! integrators, the WebAssembly build, and the browser app.

use serde_json::Value;
use sonar::api::{Engine, EngineConfig};
use sonar::json_server::handle_line;
use sonar::time_limit::Deadline;
use std::io::Cursor;

fn fresh_engine() -> Engine {
    Engine::new(EngineConfig {
        use_learning: false,
        default_deadline_secs: 0, // fast path for tests
        ..Default::default()
    })
}

fn send(engine: &mut Engine, line: &str) -> Value {
    let v = handle_line(engine, line);
    assert!(
        v.is_object(),
        "protocol replies must be JSON objects, got: {}",
        v
    );
    v
}

#[test]
fn test_full_game_over_protocol() {
    // Two engines duel through the JSON protocol, exactly like an
    // external integration would drive them.
    let mut sonar_side = fresh_engine();
    let mut human_side = fresh_engine();
    send(&mut sonar_side, r#"{"cmd":"new_game","seed":4242}"#);
    send(&mut human_side, r#"{"cmd":"new_game","seed":1717}"#);
    send(&mut sonar_side, r#"{"cmd":"place_smart"}"#);
    send(&mut human_side, r#"{"cmd":"place_random"}"#);

    let mut moves = 0u32;
    let winner;
    loop {
        // Sonar's engine picks a move targeting the human's fleet.
        let mv = send(&mut sonar_side, r#"{"cmd":"choose_move","deadline_secs":0}"#);
        let r = mv["row"].as_u64().expect("row") as usize;
        let c = mv["col"].as_u64().expect("col") as usize;
        // The human resolves the shot against their own fleet.
        let res = send(&mut human_side, &format!(r#"{{"cmd":"receive_shot","r":{},"c":{}}}"#, r, c));
        let result_str = res["result"].as_str().expect("result");
        // Sonar observes the outcome.
        send(
            &mut sonar_side,
            &format!(r#"{{"cmd":"observe","r":{},"c":{},"result":"{}"}}"#, r, c, result_str),
        );
        moves += 1;
        if result_str == "sunk" {
            let snap = send(&mut human_side, r#"{"cmd":"snapshot"}"#);
            let sunk_mask: u128 = snap["our_sunk_mask"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let fleet_mask: u128 = snap["our_fleet_mask"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            if sunk_mask == fleet_mask && fleet_mask != 0 {
                winner = "sonar";
                break;
            }
        }
        // The human (simulated) fires back through the other engine.
        let mv2 = send(&mut human_side, r#"{"cmd":"choose_move","deadline_secs":0}"#);
        let r2 = mv2["row"].as_u64().expect("row") as usize;
        let c2 = mv2["col"].as_u64().expect("col") as usize;
        let res2 = send(&mut sonar_side, &format!(r#"{{"cmd":"receive_shot","r":{},"c":{}}}"#, r2, c2));
        let result2 = res2["result"].as_str().expect("result");
        send(
            &mut human_side,
            &format!(r#"{{"cmd":"observe","r":{},"c":{},"result":"{}"}}"#, r2, c2, result2),
        );
        moves += 1;
        if result2 == "sunk" {
            let snap = send(&mut sonar_side, r#"{"cmd":"snapshot"}"#);
            let sunk_mask: u128 = snap["our_sunk_mask"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0);
            let fleet_mask: u128 = snap["our_fleet_mask"].as_str().and_then(|s| s.parse().ok()).unwrap_or(1);
            if sunk_mask == fleet_mask && fleet_mask != 0 {
                winner = "human";
                break;
            }
        }
        assert!(moves < 400, "protocol game did not terminate");
    }
    assert!(winner == "sonar" || winner == "human");
}

#[test]
fn test_protocol_response_shapes() {
    let mut e = fresh_engine();
    e.reseed(1);

    // version
    let v = send(&mut e, r#"{"cmd":"version"}"#);
    assert_eq!(v["name"], "sonar");
    assert_eq!(v["protocol"], 1);
    assert!(v["version"].is_string());

    // place_smart → ok
    assert_eq!(send(&mut e, r#"{"cmd":"place_smart"}"#)["ok"], true);

    // snapshot → all documented fields present
    let s = send(&mut e, r#"{"cmd":"snapshot"}"#);
    for field in [
        "our_fleet_mask",
        "our_shots_mask",
        "our_hits_mask",
        "our_sunk_mask",
        "enemy_remaining",
        "hypothesis_count",
        "density_matrix",
        "moves_fired",
    ] {
        assert!(s.get(field).is_some(), "snapshot missing field {}", field);
    }

    // suggest_move → documented fields
    let m = send(&mut e, r#"{"cmd":"suggest_move","deadline_secs":0}"#);
    for field in ["row", "col", "coordinate", "confidence", "hypothesis_count", "elapsed_us"] {
        assert!(m.get(field).is_some(), "suggest_move missing field {}", field);
    }

    // probability → 100-element matrix
    let p = send(&mut e, r#"{"cmd":"probability"}"#);
    assert_eq!(p["matrix"].as_array().map(|a| a.len()), Some(100));

    // density → 100-element matrix
    let d = send(&mut e, r#"{"cmd":"density"}"#);
    assert_eq!(d["matrix"].as_array().map(|a| a.len()), Some(100));

    // reseed
    assert_eq!(send(&mut e, r#"{"cmd":"reseed","seed":42}"#)["ok"], true);

    // config round-trip
    let c = send(&mut e, r#"{"cmd":"config"}"#);
    assert!(c["rules"]["board_size"].is_u64());
}

#[test]
fn test_stdio_server_end_to_end() {
    // The stdio transport: feed a scripted session, read line-by-line.
    let input = Cursor::new(
        r#"{"cmd":"new_game","seed":9}
{"cmd":"place_random"}
{"cmd":"choose_move","deadline_secs":0}
{"cmd":"reset"}
{"cmd":"quit"}
"#
        .to_string(),
    );
    let mut out = Vec::new();
    sonar::json_server::run(input, &mut out).expect("server run ok");
    let text = String::from_utf8(out).expect("utf8 output");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5, "one reply per request, got: {}", text);
    for line in lines {
        assert!(serde_json::from_str::<Value>(line).is_ok(), "non-JSON reply: {}", line);
    }
}

#[test]
fn test_hostile_inputs_never_panic() {
    // Fuzz-style battery: garbage, truncation, type confusion, huge and
    // negative values. The server must always answer with JSON.
    let mut e = fresh_engine();
    let hostile = [
        "null",
        "[]",
        "{}",
        "42",
        "\"string\"",
        "{\"cmd\":null}",
        "{\"cmd\":\"\"}",
        "{\"cmd\":[]}",
        "{\"cmd\":{}}",
        "{\"cmd\":\"place_manual\",\"ships\":[null]}",
        "{\"cmd\":\"place_manual\",\"ships\":[[0,0]]}",
        "{\"cmd\":\"place_manual\",\"ships\":[[0,0,3,\"x\"]]}",
        "{\"cmd\":\"place_manual\",\"ships\":[[0,0,3,true],[9999,9999,3,true]]}",
        "{\"cmd\":\"observe\",\"r\":-1,\"c\":-1,\"result\":\"miss\"}",
        "{\"cmd\":\"observe\",\"r\":1e9,\"c\":1e9,\"result\":\"hit\"}",
        "{\"cmd\":\"receive_shot\",\"r\":99,\"c\":99}",
        "{\"cmd\":\"choose_move\",\"deadline_secs\":-5}",
        "{\"cmd\":\"choose_move\",\"deadline_secs\":1e18}",
        "{\"cmd\":\"suggest_move\",\"deadline_secs\":\"20\"}",
        "{\"cmd\":\"set_config\",\"config\":null}",
        "{\"cmd\":\"set_config\",\"config\":42}",
        "{\"cmd\":\"set_rules\",\"rules\":{\"board_size\":999}}",
        "{\"cmd\":\"set_rules\",\"rules\":{\"board_size\":3}}",
        "{\"cmd\":\"reseed\",\"seed\":\"not-a-number\"}",
        "{\"cmd\":\"bench\",\"games\":999999999}",
        "{\"cmd\":\"bench\",\"opponent\":\"geoffrey\"}",
        "{\"cmd\":\"unknown\"}",
        "{\"cmd\":\"quit\"}",
        "\u{1F980} unicode garbage \u{1F980}",
        "{\"cmd\":\"observe\",\"result\":\"sunk_999\"}",
        "{\"cmd\":\"observe\",\"result\":\"sunk_-1\"}",
        "{\"cmd\":\"observe\",\"result\":\"sunk_abc\"}",
    ];
    for input in hostile {
        let reply = handle_line(&mut e, input);
        assert!(reply.is_object(), "hostile input {:?} broke the protocol", input);
    }
    // The engine must still be fully functional after the abuse.
    let v = send(&mut e, r#"{"cmd":"place_smart"}"#);
    assert_eq!(v["ok"], true);
    let m = send(&mut e, r#"{"cmd":"choose_move","deadline_secs":0}"#);
    assert!(m["row"].is_u64() && m["col"].is_u64());
}

#[test]
fn test_protocol_version_stability() {
    // Wire-format contract: the response of `version` must keep its
    // documented fields stable (additive-only evolution).
    let mut e = fresh_engine();
    let v = send(&mut e, r#"{"cmd":"version"}"#);
    for field in ["name", "version", "channel", "protocol", "language", "features"] {
        assert!(v.get(field).is_some(), "version reply lost field {}", field);
    }
    assert_eq!(v["protocol"].as_u64(), Some(1), "protocol version must stay 1 (frozen)");
}

#[test]
fn test_deadline_zero_means_fast_path() {
    // deadline_secs = 0 must not hang (work-limited fast path) — this
    // guards the browser experience where no wall clock exists.
    let mut e = fresh_engine();
    send(&mut e, r#"{"cmd":"new_game","seed":5}"#);
    send(&mut e, r#"{"cmd":"place_smart"}"#);
    let t0 = std::time::Instant::now();
    let m = send(&mut e, r#"{"cmd":"choose_move","deadline_secs":0}"#);
    let elapsed = t0.elapsed();
    assert!(m["row"].is_u64(), "no row in reply");
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "deadline_secs=0 took {:?} — fast path broken",
        elapsed
    );
}

#[test]
fn test_engine_api_deadline_zero() {
    // Same guarantee at the API level: Deadline::none() + default 0 must
    // answer quickly.
    let mut e = fresh_engine();
    e.reseed(3);
    e.place_fleet_smart();
    let t0 = std::time::Instant::now();
    let _ = e.choose_move(Deadline::none());
    assert!(t0.elapsed() < std::time::Duration::from_secs(2));
}
