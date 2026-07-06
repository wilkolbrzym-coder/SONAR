//! `sonar` — command-line interface.
//!
//! Thin wrapper around the Sonar engine. All heavy lifting lives in the
//! `sonar` library crate; this binary only wires up argument parsing,
//! stdout printing, and the JSON IPC server mode.
//!
//! ## Usage
//!
//! ```text
//! sonar                       # show help
//! sonar bench                 # 100-game self-play benchmark
//! sonar bench-fast            #  20-game self-play benchmark
//! sonar bench-ref [N]         # N games vs reference bots (default 50)
//! sonar bench-ext [N]         # N games vs external Python engine
//! sonar bench-2x [N]          # N games where opponent gets 2× the time
//! sonar play                  # play vs Sonar in the terminal
//! sonar serve                 # run as JSON IPC server (stdin/stdout)
//! sonar learning              # show learning DB stats
//! sonar help
//! ```

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match cmd {
        "bench" | "benchmark" => run_benchmark(100),
        "bench-fast" => run_benchmark(20),
        "bench-big" => run_benchmark(500),
        "bench-ref" => {
            let n = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(50);
            run_benchmark_reference(n);
        }
        "bench-ext" => {
            let n = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(10);
            run_benchmark_external(n);
        }
        "bench-2x" => {
            let n = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(50);
            run_benchmark_2x_time(n);
        }
        "play" => run_play_cli(),
        "serve" | "server" => run_server(),
        "learning" => run_learning_stats(),
        "help" | "--help" | "-h" | "" => print_help(),
        other => {
            eprintln!("unknown command: {}", other);
            print_help();
            std::process::exit(1);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Help
// ─────────────────────────────────────────────────────────────────────────────

fn print_help() {
    println!("Sonar — the world's strongest battleship AI engine");
    println!();
    println!("USAGE:");
    println!("  sonar                 show this help");
    println!("  sonar bench           100-game self-play benchmark");
    println!("  sonar bench-fast       20-game self-play benchmark");
    println!("  sonar bench-big       500-game self-play benchmark");
    println!("  sonar bench-ref [N]   N games vs reference bots (default 50)");
    println!("  sonar bench-ext [N]   N games vs external Python engine");
    println!("  sonar bench-2x  [N]   N games where the opponent gets 2× the time");
    println!("  sonar play            play vs Sonar in the terminal");
    println!("  sonar serve           run as JSON IPC server on stdin/stdout");
    println!("  sonar learning        show learning database stats");
    println!();
    println!("Apache-2.0 licence. See LICENSE for full text.");
}

// ─────────────────────────────────────────────────────────────────────────────
// bench
// ─────────────────────────────────────────────────────────────────────────────

fn run_benchmark(games: u32) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Sonar self-play benchmark                                       ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    let cfg = sonar::benchmark::BenchmarkConfig {
        games,
        games_per_thread: (games + 3) / 4,
        threads: 4,
        max_hypotheses: 256,
        smart_placement: true,
    };

    println!(">>> Sonar-Hybrid vs Random ({} games)", games);
    let start = Instant::now();
    let (s1, s2, elapsed) = sonar::benchmark::run_benchmark(
        &cfg,
        sonar::benchmark::BotKind::Hybrid,
        sonar::benchmark::BotKind::Random,
    );
    println!("Time: {:?}", elapsed);
    println!("  Hybrid: {} wins ({:.1}%) | avg {:.1} moves/win | {:.0} moves/sec",
        s1.wins, s1.win_rate(), s1.avg_moves_in_wins(), s1.moves_per_sec());
    println!("  Random: {} wins ({:.1}%)", s2.wins, s2.win_rate());
    println!();

    println!(">>> Sonar-Hybrid vs PdfOnly ({} games)", games);
    let (s1, s2, elapsed) = sonar::benchmark::run_benchmark(
        &cfg,
        sonar::benchmark::BotKind::Hybrid,
        sonar::benchmark::BotKind::Pdf,
    );
    println!("Time: {:?}", elapsed);
    println!("  Hybrid:  {} wins ({:.1}%) | avg {:.1} moves/win",
        s1.wins, s1.win_rate(), s1.avg_moves_in_wins());
    println!("  PdfOnly: {} wins ({:.1}%)", s2.wins, s2.win_rate());
    println!();

    println!(">>> PdfOnly vs Random ({} games)", games);
    let (s1, s2, elapsed) = sonar::benchmark::run_benchmark(
        &cfg,
        sonar::benchmark::BotKind::Pdf,
        sonar::benchmark::BotKind::Random,
    );
    println!("Time: {:?}", elapsed);
    println!("  PdfOnly: {} wins ({:.1}%)", s1.wins, s1.win_rate());
    println!("  Random:  {} wins ({:.1}%)", s2.wins, s2.win_rate());
    println!();

    println!(">>> Sonar-Hybrid vs Sonar-Hybrid ({} games, coherence test)", games);
    let (s1, s2, elapsed) = sonar::benchmark::run_benchmark(
        &cfg,
        sonar::benchmark::BotKind::Hybrid,
        sonar::benchmark::BotKind::Hybrid,
    );
    println!("Time: {:?}", elapsed);
    println!("  Hybrid(P1): {} wins ({:.1}%)", s1.wins, s1.win_rate());
    println!("  Hybrid(P2): {} wins ({:.1}%)", s2.wins, s2.win_rate());
    println!("  Expected ~50/50 if there is no first-mover bias.");
    println!();
    println!("Total benchmark time: {:?}", start.elapsed());
}

// ─────────────────────────────────────────────────────────────────────────────
// bench-ref
// ─────────────────────────────────────────────────────────────────────────────

fn run_benchmark_reference(games: u32) {
    use sonar::game::Game;
    use sonar::player::{BotPlayer, Player};
    use sonar::placement::{place_best_fleet, PlacementConfig};
    use sonar::reference_bots::{make_reference, ReferenceKind};
    use sonar::time_limit::Deadline;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    use std::sync::Arc;

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Sonar vs reference bots (published algorithms)                  ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    let refs: Vec<(&str, ReferenceKind)> = vec![
        ("HuntTarget", ReferenceKind::HuntTarget),
        ("BurnsPdf", ReferenceKind::BurnsPdf),
        ("MonteCarlo-256", ReferenceKind::MonteCarlo(256)),
        ("MonteCarlo-512", ReferenceKind::MonteCarlo(512)),
    ];

    let num_threads = 4;

    for (name, kind) in refs {
        println!(">>> Sonar-Hybrid vs {} ({} games)", name, games);
        let start = Instant::now();

        let wins = Arc::new(AtomicU32::new(0));
        let moves_in_wins = Arc::new(AtomicU64::new(0));

        let games_per_thread = (games + num_threads - 1) / num_threads;
        let mut handles = Vec::new();

        for t in 0..num_threads {
            let wins = wins.clone();
            let moves_in_wins = moves_in_wins.clone();
            let start_idx = t * games_per_thread;
            let end_idx = std::cmp::min(start_idx + games_per_thread, games);
            if start_idx >= end_idx {
                continue;
            }

            handles.push(std::thread::spawn(move || {
                let dl = Deadline::none();
                for i in start_idx..end_idx {
                    let mut our = BotPlayer::new("Sonar", 256, true)
                        .without_learning()
                        .with_deadline(sonar::time_limit::Deadline::none());
                    let mut opp = make_reference(kind, name);
                    our.place_fleet();
                    let mut rng_for_opp = sonar::rng::Xoshiro256::from_seed(
                        (i as u64).wrapping_mul(0xDEADBEEF).wrapping_add(7),
                    );
                    *opp.board_mut() = place_best_fleet(&mut rng_for_opp, &PlacementConfig::default());

                    let mut g = Game::new(Box::new(our), Box::new(opp));
                    let winner = g.play(dl);
                    if winner == 1 {
                        wins.fetch_add(1, Ordering::Relaxed);
                        moves_in_wins.fetch_add(g.moves_p1 as u64, Ordering::Relaxed);
                    }
                }
            }));
        }

        for h in handles {
            let _ = h.join();
        }

        let elapsed = start.elapsed();
        let total_wins = wins.load(Ordering::Relaxed);
        let total_moves_in_wins = moves_in_wins.load(Ordering::Relaxed);
        let win_rate = total_wins as f64 / games as f64 * 100.0;
        let avg_moves = if total_wins > 0 { total_moves_in_wins as f64 / total_wins as f64 } else { 0.0 };
        println!("  Wins: {}/{} ({:.1}%) | avg {:.1} moves/win", total_wins, games, win_rate, avg_moves);
        println!("  Time: {:?}", elapsed);
        println!();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// bench-ext (Python external engine)
// ─────────────────────────────────────────────────────────────────────────────

fn run_benchmark_external(games: u32) {
    use sonar::external::mitchelljy_engine;
    use sonar::game::Game;
    use sonar::player::BotPlayer;
    use sonar::time_limit::Deadline;

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Sonar vs external engine (mitchelljy/battleships_ai Python)     ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    let probe = mitchelljy_engine();
    let path = match probe.as_ref().map(|e| e.path.clone()) {
        Some(p) => {
            println!("Found external engine: {}", p.display());
            p
        }
        None => {
            println!("External engine not available.");
            println!("Install python3 + numpy in external/mitchelljy/");
            return;
        }
    };
    drop(probe);

    let dl = Deadline::none();
    let mut wins = 0u32;
    let start = Instant::now();
    for i in 0..games {
        let mut our = BotPlayer::new("Sonar", 256, true).without_learning().with_deadline(sonar::time_limit::Deadline::none());
        our.place_fleet();
        let engine = sonar::external::ExternalEngine::new("mitchelljy-MC", path.clone());
        let mut g = Game::new(Box::new(our), Box::new(engine));
        let winner = g.play(dl);
        if winner == 1 {
            wins += 1;
        }
        println!("Game {}: {}", i + 1, if winner == 1 { "WIN" } else { "loss" });
    }
    let elapsed = start.elapsed();
    println!();
    println!("Result: {}/{} wins ({:.1}%)", wins, games, wins as f64 / games as f64 * 100.0);
    println!("Time:   {:?}", elapsed);
}

// ─────────────────────────────────────────────────────────────────────────────
// bench-2x — opponent gets 2× our move time
// ─────────────────────────────────────────────────────────────────────────────

fn run_benchmark_2x_time(games: u32) {
    use sonar::game::Game;
    use sonar::player::{BotPlayer, Player};
    use sonar::reference_bots::{make_reference, ReferenceKind};
    use sonar::time_limit::Deadline;

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Sonar vs best opponent — opponent gets 2× the move time         ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    // Sonar gets 5s/move, opponent gets 10s/move.
    let sonar_secs: u64 = 5;
    let opp_secs: u64 = sonar_secs * 2;

    let opponents: Vec<(&str, ReferenceKind)> = vec![
        ("BurnsPdf", ReferenceKind::BurnsPdf),
        ("MonteCarlo-512", ReferenceKind::MonteCarlo(512)),
    ];

    for (name, kind) in opponents {
        println!(">>> Sonar ({}s/move) vs {} ({}s/move) — {} games",
            sonar_secs, name, opp_secs, games);
        let mut wins = 0u32;
        let start = Instant::now();
        for i in 0..games {
            let mut our = BotPlayer::new("Sonar", 256, true)
                .without_learning()
                .with_deadline(Deadline::from_secs(sonar_secs));
            let mut opp = make_reference(kind, name);
            our.place_fleet();
            let mut rng_for_opp = sonar::rng::Xoshiro256::from_seed(
                (i as u64).wrapping_mul(0xCAFE).wrapping_add(11),
            );
            use sonar::placement::{place_best_fleet, PlacementConfig};
            *opp.board_mut() = place_best_fleet(&mut rng_for_opp, &PlacementConfig::default());

            // Opponent gets 2× the time per move via a generous deadline.
            let dl_sonar = Deadline::from_secs(sonar_secs);
            let dl_opp = Deadline::from_secs(opp_secs);
            let mut g = Game::new(Box::new(our), Box::new(opp));
            // We can't pass different deadlines to play(), so we drive the loop manually.
            let winner = play_with_asymmetric_deadlines(&mut g, dl_sonar, dl_opp);
            if winner == 1 {
                wins += 1;
            }
        }
        let elapsed = start.elapsed();
        println!("  Wins: {}/{} ({:.1}%)", wins, games, wins as f64 / games as f64 * 100.0);
        println!("  Time: {:?}", elapsed);
        println!();
    }
}

fn play_with_asymmetric_deadlines(g: &mut sonar::game::Game, dl1: sonar::time_limit::Deadline, dl2: sonar::time_limit::Deadline) -> u8 {
    use sonar::player::Player;
    let max_moves = 200;
    for _ in 0..max_moves {
        let (r, c) = g.p1.choose_move(dl1);
        let res = g.p2.board_mut().shoot(r, c);
        g.p1.observe_result(r, c, res);
        g.p2.observe_incoming(r, c, res);
        g.moves_p1 += 1;
        if g.p2.is_defeated() {
            g.finished = Some(std::time::Instant::now());
            return 1;
        }
        let (r, c) = g.p2.choose_move(dl2);
        let res = g.p1.board_mut().shoot(r, c);
        g.p2.observe_result(r, c, res);
        g.p1.observe_incoming(r, c, res);
        g.moves_p2 += 1;
        if g.p1.is_defeated() {
            g.finished = Some(std::time::Instant::now());
            return 2;
        }
    }
    g.finished = Some(std::time::Instant::now());
    if g.moves_p1 <= g.moves_p2 { 1 } else { 2 }
}

// ─────────────────────────────────────────────────────────────────────────────
// play (CLI)
// ─────────────────────────────────────────────────────────────────────────────

fn run_play_cli() {
    use sonar::board::ShotResult;
    use sonar::player::{BotPlayer, Player};
    use sonar::placement::PlacementConfig;
    use sonar::rng::Xoshiro256;
    use sonar::time_limit::Deadline;

    println!("=== Sonar — you vs the engine (CLI) ===");
    println!("Time limit per move for Sonar: 20s (default). Adjust with $SONAR_MOVE_SECS.");
    let secs = std::env::var("SONAR_MOVE_SECS")
        .ok().and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20);
    let dl = Deadline::from_secs(secs);

    let mut rng = Xoshiro256::from_seed(sonar::rng::random_u64());
    let mut my_board = sonar::placement::place_best_fleet(&mut rng, &PlacementConfig::default());
    let mut bot = BotPlayer::new("Sonar", 256, true).without_learning().with_deadline(sonar::time_limit::Deadline::none());
    bot.place_fleet();
    let mut bot_board = bot.board.clone();
    let mut moves = 0u32;

    loop {
        print_cli_boards(&bot_board, &my_board);
        println!("Move #{}. Enter coordinates (e.g. E5) or 'q' to quit: ", moves + 1);
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).expect("read error");
        let input = input.trim();
        if input == "q" || input == "quit" { return; }
        let (r, c) = match sonar::helpers::parse_coordinate(input) {
            Some(rc) => rc,
            None => { println!("Bad coordinate. Try e.g. E5"); continue; }
        };

        let res = bot_board.shoot(r, c);
        bot.observe_result(r, c, res);
        moves += 1;
        let s = match res {
            ShotResult::Miss => "MISS",
            ShotResult::Hit => "HIT",
            ShotResult::Sunk(_) => "SUNK!",
            ShotResult::AlreadyShot => { println!("Already shot there"); continue; }
            ShotResult::Invalid => { println!("Invalid move"); continue; }
        };
        println!("Your shot at ({},{}): {}", r, c, s);

        if bot_board.all_sunk() {
            print_cli_boards(&bot_board, &my_board);
            println!("*** YOU WIN in {} moves! ***", moves);
            return;
        }

        let (br, bc) = bot.choose_move(dl);
        let bres = my_board.shoot(br, bc);
        moves += 1;
        let bs = match bres {
            ShotResult::Miss => "MISS",
            ShotResult::Hit => "HIT",
            ShotResult::Sunk(_) => "SUNK!",
            _ => "?",
        };
        println!("Sonar fires at ({},{}): {}", br, bc, bs);
        if my_board.all_sunk() {
            print_cli_boards(&bot_board, &my_board);
            println!("*** SONAR WINS in {} moves ***", moves);
            return;
        }
    }
}

fn print_cli_boards(enemy: &sonar::board::Board, mine: &sonar::board::Board) {
    println!();
    println!("Enemy board (fire here):        Your board:");
    println!("   A B C D E F G H I J             A B C D E F G H I J");
    for r in 0..10 {
        print!("{:2} ", r + 1);
        for c in 0..10 {
            use sonar::board::Cell::*;
            let ch = match enemy.cell_state_for_attacker(r, c) {
                Unknown => '·',
                Miss => '○',
                Hit => 'x',
                Sunk => 'X',
            };
            print!("{} ", ch);
        }
        print!("     {:2} ", r + 1);
        for c in 0..10 {
            use sonar::board::OwnerCell::*;
            let ch = match mine.cell_state_for_owner(r, c) {
                Empty => '·',
                Ship => '#',
                MissedShot => '○',
                HitShip => 'x',
                SunkShip => 'X',
            };
            print!("{} ", ch);
        }
        println!();
    }
    println!();
}

// ─────────────────────────────────────────────────────────────────────────────
// serve (JSON IPC)
// ─────────────────────────────────────────────────────────────────────────────

fn run_server() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    if let Err(e) = sonar::json_server::run(stdin.lock(), &mut stdout) {
        eprintln!("sonar server error: {}", e);
        std::process::exit(1);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// learning stats
// ─────────────────────────────────────────────────────────────────────────────

fn run_learning_stats() {
    let g = sonar::learning::global();
    if let Some(db) = g.as_ref() {
        println!("=== Sonar micro-learning stats ===");
        println!("Total games: {}", db.len());
        if db.is_empty() {
            println!("(empty — play some games to build the database)");
            return;
        }
        let wins = db.games.iter().filter(|g| g.won).count() as u32;
        let losses = db.len() as u32 - wins;
        println!("Wins: {} | Losses: {} | Win rate: {:.1}%",
            wins, losses, wins as f64 / db.len() as f64 * 100.0);
        println!();
        println!("Top fleet placement patterns (top 5):");
        for (i, (mask, rate, n)) in db.best_fleet_patterns(5).iter().enumerate() {
            println!("  {}. win_rate={:.1}% ({} games) mask={}", i + 1, rate * 100.0, n, mask);
        }
        println!();
        println!("Learning file: {}", sonar::learning::default_path().display());
    }
}
