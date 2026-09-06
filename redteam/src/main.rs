//! Sonar red-team harness — the adversarial attack suite (out-of-tree).
//!
//! Four attacks, one goal: prove there is nothing to exploit.
//!
//! | Attack                | Threat model                                             | Metric                          |
//! |-----------------------|----------------------------------------------------------|---------------------------------|
//! | [`protocol_fuzz`]     | malformed / adversarial JSON drives the engine into UB    | zero panics, valid replies      |
//! | [`adversary`]         | an oracle designs the worst possible fleet placement      | shots delta vs uniform-random   |
//! | [`determinism`]       | timing/order side-channels change decisions               | bit-identical replays           |
//! | [`invariants`]        | the engine breaks game invariants under stress            | zero violations incl. variants  |
//!
//! Run: `cargo run --manifest-path redteam/Cargo.toml [-- --quick]`
//! CI:  `cargo test --manifest-path redteam/Cargo.toml` (same attacks,
//! smaller budgets — the full budgets run on the nightly gate).

#![forbid(unsafe_code)]

use serde_json::Value;

mod attack {
    pub mod protocol_fuzz;
    pub mod adversary;
    pub mod determinism;
    pub mod invariants;
}

fn main() {
    let quick = std::env::args().any(|a| a == "--quick");
    let mut report = String::new();
    let mut failures = 0u32;

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   SONAR RED-TEAM — adversarial harness (out-of-tree, never shipped) ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!("mode: {}\n", if quick { "quick" } else { "full" });

    // ── Attack 1: protocol fuzz ─────────────────────────────────────────
    let (panics, replies) = attack::protocol_fuzz::run(quick);
    let ok = panics == 0 && replies > 0;
    println!("[{ }] protocol_fuzz      : {} inputs, {} panics", mark(ok), replies, panics);
    report.push_str(&format!(
        "## protocol_fuzz\n\n- adversarial JSON inputs: {}\n- panics: {}\n- status: {}\n\n",
        replies,
        panics,
        status(ok)
    ));
    failures += u32::from(!ok);

    // ── Attack 2: adversarial placement ─────────────────────────────────
    let adv = attack::adversary::run(quick);
    println!(
        "[{ }] adversary          : worst fleet costs Sonar {:.1} shots (random baseline {:.1}, delta {:.1})",
        mark(adv.delta <= adv.threshold),
        adv.adversary_shots,
        adv.random_shots,
        adv.delta
    );
    report.push_str(&format!(
        "## adversary placement\n\n- adversarial fleet: {:.1} shots for Sonar\n- uniform-random fleet: {:.1} shots\n- exploitability delta: {:.2} shots (threshold {:.2})\n- status: {}\n\n",
        adv.adversary_shots,
        adv.random_shots,
        adv.delta,
        adv.threshold,
        status(adv.delta <= adv.threshold)
    ));
    failures += u32::from(adv.delta > adv.threshold);

    // ── Attack 3: determinism ───────────────────────────────────────────
    let det = attack::determinism::run(quick);
    println!(
        "[{ }] determinism        : {} seeds replayed bit-identically, {} mismatches",
        mark(det.mismatches == 0),
        det.seeds,
        det.mismatches
    );
    report.push_str(&format!(
        "## determinism\n\n- seeds replayed: {}\n- mismatches: {}\n- status: {}\n\n",
        det.seeds,
        det.mismatches,
        status(det.mismatches == 0)
    ));
    failures += u32::from(det.mismatches != 0);

    // ── Attack 4: invariants ────────────────────────────────────────────
    let inv = attack::invariants::run(quick);
    println!(
        "[{ }] invariants         : {} games (incl. variants), {} violations",
        mark(inv.violations == 0),
        inv.games,
        inv.violations
    );
    report.push_str(&format!(
        "## invariants\n\n- games stressed: {} (classic + generalised variants)\n- violations: {}\n- status: {}\n\n",
        inv.games,
        inv.violations,
        status(inv.violations == 0)
    ));
    failures += u32::from(inv.violations != 0);

    println!();
    if failures == 0 {
        println!("RESULT: ALL ATTACKS REPELLED — engine state is clean.");
    } else {
        println!("RESULT: {} ATTACK(S) BREACHED — see report above.", failures);
    }

    // Write the machine-readable metrics (feeds EXPLOITABILITY.md).
    let metrics = serde_json::json!({
        "version": sonar::json_server::PROTOCOL_VERSION,
        "sonar_version": env!("CARGO_PKG_VERSION"),
        "protocol_fuzz": {"inputs": replies, "panics": panics},
        "adversary": {
            "adversary_shots": adv.adversary_shots,
            "random_shots": adv.random_shots,
            "delta": adv.delta,
            "threshold": adv.threshold,
        },
        "determinism": {"seeds": det.seeds, "mismatches": det.mismatches},
        "invariants": {"games": inv.games, "violations": inv.violations},
        "failures": failures,
    });
    let _ = std::fs::write("redteam/metrics.json", serde_json::to_string_pretty(&metrics).unwrap_or_default());
    let _ = std::fs::write("redteam/report.md", format!("# Sonar red-team report\n\n{report}"));
    let _ = Value::Null;

    std::process::exit(i32::from(failures != 0));
}

fn mark(ok: bool) -> char {
    if ok { '+' } else { 'x' }
}

fn status(ok: bool) -> &'static str {
    if ok { "PASSED" } else { "FAILED" }
}
