//! Attack 1: protocol fuzzing — drive the JSON protocol with adversarial
//! input and assert the engine never panics and always replies valid
//! JSON with an `ok` or documented shape.

use sonar::api::{Engine, EngineConfig};
use sonar::json_server::handle_line;

/// Mutation-based fuzz over a corpus of *valid* commands.
pub fn run(quick: bool) -> (u32, u32) {
    let corpus: Vec<String> = [
        r#"{"cmd":"version"}"#,
        r#"{"cmd":"new_game","seed":42}"#,
        r#"{"cmd":"place_smart"}"#,
        r#"{"cmd":"choose_move","deadline_secs":1}"#,
        r#"{"cmd":"observe","r":5,"c":5,"result":"hit"}"#,
        r#"{"cmd":"snapshot"}"#,
        r#"{"cmd":"set_rules","rules":{"board_size":7,"ship_lengths":[4,3,2]}}"#,
        r#"{"cmd":"variant_list"}"#,
        r#"{"cmd":"variant_new","preset":"micro","seed":1}"#,
        r#"{"cmd":"variant_move"}"#,
        r#"{"cmd":"variant_fire","r":0,"c":0}"#,
        r#"{"cmd":"variant_state"}"#,
        r#"{"cmd":"bench","games":1,"opponent":"random"}"#,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let mut rng: u64 = 0xA11CE;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };

    let rounds = if quick { 400 } else { 2000 };
    let mut panics = 0u32;
    let mut inputs = 0u32;

    for i in 0..rounds {
        // Base command (rotating corpus).
        let mut line = corpus[i % corpus.len()].clone();

        // Mutations: flip bytes, inject junk, truncate, explode numbers.
        let mutations = (next() % 4) + 1;
        for _ in 0..mutations {
            if line.is_empty() {
                break;
            }
            match next() % 8 {
                0 => {
                    // Random byte flip — byte-level, so multi-byte UTF-8
                    // sequences get corrupted safely (the engine must cope).
                    let mut bytes = line.into_bytes();
                    let pos = (next() as usize) % bytes.len().max(1);
                    bytes[pos] ^= (next() as u8).max(1);
                    line = String::from_utf8_lossy(&bytes).into_owned();
                }
                1 => {
                    // Byte-level truncation (multi-byte chars may split —
                    // the engine must cope with invalid UTF-8 shards).
                    let cut = (next() as usize) % line.len().max(1);
                    let bytes = line.into_bytes();
                    line = String::from_utf8_lossy(&bytes[..cut]).into_owned();
                }
                2 => line.push_str(&"x".repeat((next() as usize) % 64)),
                3 => line = format!("{}{}", line, next() % 1_000_000),
                4 => line.push_str("\u{7f}\u{0}\u{1f680}"),
                5 => line = line.replace('}', &"}{".repeat(3)),
                6 => line = line.replace(':', "=="),
                _ => line = format!("[{},\"{}\"]", line, next()),
            }
        }

        // The target: a fresh engine with learning disabled (the harness
        // must never touch the user's statistics database).
        inputs += 1;
        let mut engine = Engine::new(EngineConfig {
            use_learning: false,
            hypothesis_soft_target: 16,
            default_deadline_secs: 1,
            ..Default::default()
        });
        let moved_line = line;
        // The engine holds a boxed strategy (mutable trait object) — not
        // UnwindSafe by default; a panic inside the handler leaves the
        // engine dropped, so asserting unwind safety is sound here.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let reply = handle_line(&mut engine, &moved_line);
            // Every reply must be a JSON object.
            assert!(reply.is_object(), "reply not an object for {:?}", moved_line);
        }));
        if result.is_err() {
            panics += 1;
        }
    }

    // Direct adversarial inputs — every one must yield a well-formed
    // reply, never a panic.
    let nasty = [
        "".to_string(),
        "\u{0}".to_string(),
        "{}".to_string(),
        "null".to_string(),
        "[]".to_string(),
        "{\"cmd\":null}".to_string(),
        "{\"cmd\":[]}".to_string(),
        "{\"cmd\":{}}".to_string(),
        "{\"cmd\":2147483648}".to_string(),
        "{\"cmd\":\"observe\",\"r\":1e309,\"c\":-1}".to_string(),
        "{\"cmd\":\"observe\",\"r\":true,\"c\":null,\"result\":[]}".to_string(),
        "{\"cmd\":\"place_manual\",\"ships\":[[null,null,null,null]]}".to_string(),
        "{\"cmd\":\"place_manual\",\"ships\":[[99999999,99999999,255,2]]}".to_string(),
        "{\"cmd\":\"set_rules\",\"rules\":null}".to_string(),
        "{\"cmd\":\"set_rules\",\"rules\":{\"board_size\":1e400}}".to_string(),
        "{\"cmd\":\"variant_new\",\"preset\":12345}".to_string(),
        "{\"cmd\":\"variant_new\",\"rules\":{\"geometry\":{\"width\":1e300,\"height\":0}}}".to_string(),
        "{\"cmd\":\"variant_fire\",\"r\":4294967296,\"c\":4294967296}".to_string(),
        "{".repeat(50),
        format!("{{\"cmd\":\"{}\"}}", "A".repeat(10000)),
    ];
    for line in nasty {
        inputs += 1;
        let mut engine = Engine::new(EngineConfig {
            use_learning: false,
            ..Default::default()
        });
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let reply = handle_line(&mut engine, &line);
                assert!(reply.is_object(), "reply not an object");
            }));
        if result.is_err() {
            panics += 1;
        }
    }

    (panics, inputs)
}

#[cfg(test)]
mod tests {
    #[test]
    fn protocol_fuzz_quick_gate() {
        let (panics, inputs) = super::run(true);
        assert_eq!(panics, 0, "protocol must never panic");
        assert!(inputs > 400, "the fuzzer must have run: {}", inputs);
    }
}
