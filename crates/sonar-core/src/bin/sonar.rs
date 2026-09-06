//! `sonar` — command-line interface.
//!
//! A thin wrapper around the Sonar engine. All heavy lifting lives in the
//! `sonar` library crate; this binary only wires up argument parsing,
//! stdout printing, and the JSON IPC server mode.
//!
//! ## Usage
//!
//! ```text
//! sonar                        show help
//! sonar version                print version + build info
//! sonar play                   play vs Sonar in the terminal
//! sonar serve                  run as JSON IPC server (stdin/stdout)
//! sonar learning               show game statistics DB
//! sonar bench            [N]   self-play benchmark (default 100)
//! sonar bench-fast       [N]   quick self-play benchmark (default 20)
//! sonar bench-big        [N]   large self-play benchmark (default 500)
//! sonar bench-ref        [N]   N games vs reference bots (default 50)
//! sonar bench-2x         [N]   opponent gets 2× the time (default 50)
//! sonar bench-half       [N]   opponent gets 2× the hypotheses (default 50)
//! sonar endgame          [S]   demo: sink a touched submarine optimally (seed S)
//! sonar sprt             [N]   SPRT match: hybrid-512 vs hybrid-64 (max N games)
//! sonar ladder           [N]   SPRT strength ladder over hypothesis budgets
//! sonar verify-release   [N]   seed-verified release numbers (N games/matchup)
//! sonar help
//! ```

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match cmd {
        "version" | "--version" | "-V" => print_version(),
        "bench" | "benchmark" => run_benchmark(arg_n(&args, 100)),
        "bench-fast" => run_benchmark(arg_n(&args, 20)),
        "bench-big" => run_benchmark(arg_n(&args, 500)),
        "bench-ref" => run_benchmark_reference(arg_n(&args, 50)),
        "bench-2x" => run_benchmark_asymmetric(arg_n(&args, 50), "2x-time", 5, 10, 256, 256),
        "bench-half" => run_benchmark_hypotheses(arg_n(&args, 50)),
        "play" => run_play_cli(),
        "serve" | "server" => run_server(),
        "learning" => run_learning_stats(),
        "endgame" => run_endgame_demo(arg_seed(&args, 2024)),
        "sprt" => run_sprt(arg_n64(&args, 400)),
        "ladder" => run_ladder(arg_n64(&args, 400)),
        "verify-release" => run_verify_release(arg_n(&args, 20)),
        "help" | "--help" | "-h" | "" => print_help(),
        other => {
            eprintln!("unknown command: {}", other);
            print_help();
            std::process::exit(1);
        }
    }
}

fn arg_n(args: &[String], default: u32) -> u32 {
    args.get(2)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(default)
}

fn arg_n64(args: &[String], default: u64) -> u64 {
    args.get(2)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default)
}

fn arg_seed(args: &[String], default: u64) -> u64 {
    arg_n64(args, default)
}

// ─────────────────────────────────────────────────────────────────────────────
// Help / version
// ─────────────────────────────────────────────────────────────────────────────

fn print_version() {
    println!("sonar {} (beta)", env!("CARGO_PKG_VERSION"));
    println!(
        "protocol: json-ipc v{}",
        sonar::json_server::PROTOCOL_VERSION
    );
    println!("engine: PDF density + Bayesian hypothesis filter");
    println!("license: Apache-2.0");
}

fn print_help() {
    println!(
        "Sonar {} — the world's strongest battleship AI engine",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USAGE:");
    println!("  sonar                        show this help");
    println!("  sonar version                print version + build info");
    println!("  sonar play                   play vs Sonar in the terminal");
    println!("  sonar serve                  run as JSON IPC server on stdin/stdout");
    println!("  sonar learning               show game statistics database");
    println!("  sonar bench            [N]   self-play benchmark (default 100)");
    println!("  sonar bench-fast       [N]   20-game quick self-play benchmark");
    println!("  sonar bench-big        [N]   500-game self-play benchmark");
    println!("  sonar bench-ref        [N]   N games vs reference bots (default 50)");
    println!("  sonar bench-2x         [N]   opponent gets 2x the move time");
    println!("  sonar bench-half       [N]   opponent gets 2x the hypothesis budget");
    println!("  sonar endgame          [S]   exact-census endgame demo (seed S)");
    println!("  sonar sprt             [N]   SPRT strength match (max N games)");
    println!("  sonar ladder           [N]   SPRT strength ladder (max N games/rung)");
    println!("  sonar verify-release   [N]   seed-verified release numbers");
    println!();
    println!("Apache-2.0 licence. See LICENSE for full text.");
}

// ─────────────────────────────────────────────────────────────────────────────
// bench — self-play with Wilson CIs
// ─────────────────────────────────────────────────────────────────────────────

fn run_benchmark(games: u32) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Sonar self-play benchmark                                       ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    let cfg = sonar::benchmark::BenchmarkConfig {
        games,
        threads: 4,
        // Sonar's honest default strength (EngineConfig default).
        max_hypotheses: 1024,
        smart_placement: true,
        seed: 0xBEEF_CAFE_1234_5678,
    };

    let matchups: Vec<(&str, sonar::benchmark::BotKind, sonar::benchmark::BotKind)> = vec![
        (
            "Sonar-Hybrid vs Random",
            sonar::benchmark::BotKind::Hybrid,
            sonar::benchmark::BotKind::Random,
        ),
        (
            "Sonar-Hybrid vs PdfOnly",
            sonar::benchmark::BotKind::Hybrid,
            sonar::benchmark::BotKind::Pdf,
        ),
        (
            "PdfOnly vs Random",
            sonar::benchmark::BotKind::Pdf,
            sonar::benchmark::BotKind::Random,
        ),
        (
            "Sonar-Hybrid vs Sonar-Hybrid (first-mover check)",
            sonar::benchmark::BotKind::Hybrid,
            sonar::benchmark::BotKind::Hybrid,
        ),
    ];

    let total_start = std::time::Instant::now();
    for (title, k1, k2) in matchups {
        println!(">>> {} ({} games)", title, games);
        let start = std::time::Instant::now();
        let (s1, s2, _) = sonar::benchmark::run_benchmark(&cfg, k1, k2);
        sonar::benchmark::print_report(k1.name(), &s1, k2.name(), &s2, start.elapsed());
        if k1 == k2 {
            println!("  Expected ~50/50 if there is no first-mover bias.");
        }
        println!();
    }
    println!("Total benchmark time: {:?}", total_start.elapsed());
}

// ─────────────────────────────────────────────────────────────────────────────
// bench-ref — vs published reference algorithms
// ─────────────────────────────────────────────────────────────────────────────

fn run_benchmark_reference(games: u32) {
    use sonar::game::Game;
    use sonar::placement::{PlacementConfig, place_best_fleet};
    use sonar::player::{BotPlayer, Player};
    use sonar::reference_bots::{ReferenceKind, make_reference};
    use sonar::time_limit::Deadline;

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
        let start = std::time::Instant::now();

        let wins = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let moves_in_wins = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

        let games_per_thread = games.div_ceil(num_threads);
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
                    let mut our = BotPlayer::new("Sonar", 1024, true)
                        .without_learning()
                        .with_deadline(sonar::time_limit::Deadline::none());
                    let mut opp = make_reference(kind, name);
                    our.place_fleet();
                    let mut rng_for_opp = sonar::rng::Xoshiro256::from_seed(
                        (i as u64).wrapping_mul(0xDEADBEEF).wrapping_add(7),
                    );
                    *opp.board_mut() =
                        place_best_fleet(&mut rng_for_opp, &PlacementConfig::default());

                    let mut g = Game::new(Box::new(our), Box::new(opp));
                    let winner = g.play(dl);
                    if winner == 1 {
                        wins.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        moves_in_wins
                            .fetch_add(g.moves_p1 as u64, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }));
        }

        for h in handles {
            let _ = h.join();
        }

        let elapsed = start.elapsed();
        let total_wins = wins.load(std::sync::atomic::Ordering::Relaxed);
        let total_moves_in_wins = moves_in_wins.load(std::sync::atomic::Ordering::Relaxed);
        let win_rate = total_wins as f64 / games as f64 * 100.0;
        let avg_moves = if total_wins > 0 {
            total_moves_in_wins as f64 / total_wins as f64
        } else {
            0.0
        };
        println!(
            "  Wins: {}/{} ({:.1}%) | avg {:.1} moves/win",
            total_wins, games, win_rate, avg_moves
        );
        println!("  Time: {:?}", elapsed);
        println!();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// bench-2x — time-handicap benchmark (opponent gets 2× the time)
// ─────────────────────────────────────────────────────────────────────────────

fn run_benchmark_asymmetric(
    games: u32,
    title: &str,
    sonar_secs: u64,
    opp_secs: u64,
    sonar_hyp: usize,
    opp_hyp: usize,
) {
    use sonar::game::Game;
    use sonar::player::{BotPlayer, Player};
    use sonar::reference_bots::{ReferenceKind, make_reference};
    use sonar::time_limit::Deadline;

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!(
        "║   Sonar handicapped — {} (opponent advantage)                     ",
        title
    );
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    let opponents: Vec<(&str, ReferenceKind)> = vec![
        ("BurnsPdf", ReferenceKind::BurnsPdf),
        ("MonteCarlo-512", ReferenceKind::MonteCarlo(512)),
    ];

    for (name, kind) in opponents {
        println!(
            ">>> Sonar ({}s/move, {} hyp) vs {} ({}s/move, {} hyp) — {} games",
            sonar_secs, sonar_hyp, name, opp_secs, opp_hyp, games
        );
        let mut wins = 0u32;
        let start = std::time::Instant::now();
        for i in 0..games {
            let mut our = BotPlayer::new("Sonar", sonar_hyp, true)
                .without_learning()
                .with_deadline(Deadline::from_secs(sonar_secs));
            let mut opp = make_reference(kind, name);
            our.place_fleet();
            let mut rng_for_opp =
                sonar::rng::Xoshiro256::from_seed((i as u64).wrapping_mul(0xCAFE).wrapping_add(11));
            use sonar::placement::{PlacementConfig, place_best_fleet};
            *opp.board_mut() = place_best_fleet(&mut rng_for_opp, &PlacementConfig::default());

            let dl_sonar = Deadline::from_secs(sonar_secs);
            let dl_opp = Deadline::from_secs(opp_secs);
            let mut g = Game::new(Box::new(our), Box::new(opp));
            // Different deadlines per player.
            let winner = g.play_asymmetric(dl_sonar, dl_opp);
            if winner == 1 {
                wins += 1;
            }
        }
        let elapsed = start.elapsed();
        println!(
            "  Wins: {}/{} ({:.1}%)",
            wins,
            games,
            wins as f64 / games as f64 * 100.0
        );
        println!("  Time: {:?}", elapsed);
        println!();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// bench-half — hypothesis-budget handicap (opponent gets 2× the hypotheses)
// ─────────────────────────────────────────────────────────────────────────────

fn run_benchmark_hypotheses(games: u32) {
    // The mirror of bench-2x: instead of time, the opponent gets twice the
    // hypothesis budget. Demonstrates the algorithm (not just compute)
    // carries the advantage.
    use sonar::game::Game;
    use sonar::placement::{PlacementConfig, place_best_fleet};
    use sonar::player::{BotPlayer, Player};
    use sonar::time_limit::Deadline;

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Sonar (128 hypotheses) vs Sonar (256 hypotheses)               ║");
    println!("║   Does a doubled hypothesis budget translate into wins?          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    let mut wins_weak = 0u32; // the 128-hypothesis side
    let mut wins_strong = 0u32;
    let start = std::time::Instant::now();
    let dl = Deadline::none();

    for i in 0..games {
        let seed_base = (i as u64).wrapping_mul(0xA5A5).wrapping_add(3);
        let mut weak = BotPlayer::new("Sonar-128", 128, true)
            .without_learning()
            .with_deadline(Deadline::none());
        let mut strong = BotPlayer::new("Sonar-256", 256, true)
            .without_learning()
            .with_deadline(Deadline::none());
        weak.reseed(seed_base);
        strong.reseed(seed_base ^ 0xDEAD_BEEF);

        let mut rng = sonar::rng::Xoshiro256::from_seed(seed_base ^ 0xFEED_FACE);
        *weak.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());
        *strong.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());

        let mut g = Game::new(Box::new(weak), Box::new(strong));
        let winner = g.play(dl);
        if winner == 1 {
            wins_weak += 1;
        } else {
            wins_strong += 1;
        }
    }

    let elapsed = start.elapsed();
    let total = wins_weak + wins_strong;
    println!(
        "  Sonar-128 wins: {}/{} ({:.1}%)",
        wins_weak,
        total,
        wins_weak as f64 / total.max(1) as f64 * 100.0
    );
    println!(
        "  Sonar-256 wins: {}/{} ({:.1}%)",
        wins_strong,
        total,
        wins_strong as f64 / total.max(1) as f64 * 100.0
    );
    println!("  Time: {:?}", elapsed);
    println!();
}

// ─────────────────────────────────────────────────────────────────────────────
// play (CLI)
// ─────────────────────────────────────────────────────────────────────────────

fn run_play_cli() {
    use sonar::board::ShotResult;
    use sonar::placement::PlacementConfig;
    use sonar::player::{BotPlayer, Player};
    use sonar::rng::Xoshiro256;
    use sonar::time_limit::Deadline;

    println!("=== Sonar — you vs the engine (CLI) ===");
    println!("Time limit per move for Sonar: 20s (default). Adjust with $SONAR_MOVE_SECS.");
    let secs = std::env::var("SONAR_MOVE_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20);
    let dl = Deadline::from_secs(secs);

    let mut rng = Xoshiro256::from_seed(sonar::rng::random_u64());
    let mut my_board = sonar::placement::place_best_fleet(&mut rng, &PlacementConfig::default());
    let mut bot = BotPlayer::new("Sonar", 256, true)
        .without_learning()
        .with_deadline(sonar::time_limit::Deadline::none());
    bot.place_fleet();
    let mut bot_board = bot.board.clone();
    let mut moves = 0u32;

    loop {
        print_cli_boards(&bot_board, &my_board);
        println!(
            "Move #{}. Enter coordinates (e.g. E5) or 'q' to quit: ",
            moves + 1
        );
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            eprintln!("input error, quitting");
            return;
        }
        let input = input.trim();
        if input == "q" || input == "quit" {
            return;
        }
        let (r, c) = match sonar::helpers::parse_coordinate(input) {
            Some(rc) => rc,
            None => {
                println!("Bad coordinate. Try e.g. E5");
                continue;
            }
        };

        let res = bot_board.shoot(r, c);
        bot.observe_result(r, c, res);
        moves += 1;
        let s = match res {
            ShotResult::Miss => "MISS",
            ShotResult::Hit => "HIT",
            ShotResult::Sunk(_) => "SUNK!",
            ShotResult::AlreadyShot => {
                println!("Already shot there");
                continue;
            }
            ShotResult::Invalid => {
                println!("Invalid move");
                continue;
            }
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
        println!("=== Sonar game statistics ===");
        println!("Total games: {}", db.len());
        if db.is_empty() {
            println!("(empty — play some games to build the database)");
            return;
        }
        let wins = db.games.iter().filter(|g| g.won).count() as u32;
        let losses = db.len() as u32 - wins;
        println!(
            "Wins: {} | Losses: {} | Win rate: {:.1}%",
            wins,
            losses,
            wins as f64 / db.len() as f64 * 100.0
        );
        println!("Average moves per game: {:.1}", db.avg_moves());
        println!();
        println!("Top fleet placement patterns (top 5, passive statistics):");
        for (i, (mask, rate, n)) in db.best_fleet_patterns(5).iter().enumerate() {
            println!(
                "  {}. win_rate={:.1}% ({} games) mask={}",
                i + 1,
                rate * 100.0,
                n,
                mask
            );
        }
        println!();
        println!(
            "Statistics file: {}",
            sonar::learning::default_path().display()
        );
        println!("(records are passive statistics — they never influence play)");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// endgame — exact-CENSUS solver demo (0.3)
// ─────────────────────────────────────────────────────────────────────────────

fn run_endgame_demo(seed: u64) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Sonar exact-CENSUS endgame solver — submarine-perfect play     ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!("seed: {}", seed);
    println!("A hidden submarine (length 2) is touched; the solver finishes it");
    println!("with provably optimal play (exact expectimax over the census).");
    println!();

    use sonar::Xoshiro256;
    use sonar::board::{Board, Ship};
    use sonar::endgame::{EndgameConfig, EndgameSolver};
    use sonar::targeting::EnemyView;

    let mut rng = Xoshiro256::from_seed(seed);
    for trial in 1..=5 {
        let mut board = Board::new();
        let ship = loop {
            let r = rng.gen_range(10) as usize;
            let c = rng.gen_range(10) as usize;
            let h = rng.gen_range(2) == 0;
            if let Some(s) = Ship::new(r, c, 2, h)
                && board.place_ship(s)
            {
                break s;
            }
        };
        let (hr, hc) = ship.cells()[0];
        let mut view = EnemyView {
            remaining: vec![2],
            ..Default::default()
        };
        view.observe(hr, hc, board.shoot(hr, hc));

        let mut solver = EndgameSolver::new(EndgameConfig::default());
        let mut shots = 1;
        while !view.remaining.is_empty() && shots < 100 {
            let mv = solver.best_move(&view).unwrap_or_else(|| {
                eprintln!("solver deferred outside its regime");
                std::process::exit(1);
            });
            let res = board.shoot(mv.row, mv.col);
            println!(
                "  trial {}: fire ({},{}) -> {:?}   [configs: {}, E[remaining]: {:.2}]",
                trial, mv.row, mv.col, res, mv.configs, mv.expected_shots
            );
            view.observe(mv.row, mv.col, res);
            shots += 1;
        }
        println!(
            "  trial {}: submarine sunk in {} shots (optimal bound: 5)",
            trial, shots
        );
        println!();
    }
    println!("The worst case for a touched end-cell is 3 disambiguating misses");
    println!("plus the sinking hit — 5 shots. The solver never exceeds it.");
}

// ─────────────────────────────────────────────────────────────────────────────
// sprt / ladder / verify-release (0.3)
// ─────────────────────────────────────────────────────────────────────────────

fn run_sprt(max_games: u64) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   SPRT strength match — sequential probability ratio test        ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    let a = sonar::sprt::Level::new("hybrid-512", 512, true);
    let b = sonar::sprt::Level::new("hybrid-64", 64, false);
    println!(
        "A: {} | B: {} | H0: p<=0.464 | H1: p>=0.536 (+-25 Elo)",
        a.name, b.name
    );
    println!("alpha=0.05 beta=0.05 | max games: {}", max_games);
    println!();

    let report = sonar::sprt::run_sprt(
        &a,
        &b,
        sonar::sprt::SprtConfig::default(),
        0x5EED_1234,
        max_games,
        false,
    );
    println!("games    : {}", report.games);
    println!("A wins   : {}", report.wins);
    println!("B wins   : {}", report.losses);
    println!("draws    : {}", report.draws);
    println!("LLR      : {:.3}", report.llr);
    println!("score    : {:.3}", report.score);
    println!("Elo      : {:+.1}", report.elo);
    println!("decision : {}", report.decision);
}

fn run_ladder(max_games: u64) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   SPRT strength ladder — hypothesis budgets 16 -> 8192           ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    let levels = sonar::sprt::release_ladder();
    let ladder = sonar::sprt::run_ladder(&levels, 0x1ADD_0001, max_games);
    for rung in &ladder {
        let r = &rung.report;
        println!(
            "{:>12} vs {:<12} games={:<5} score={:.3} Elo={:+7.1}  {}",
            rung.challenger, rung.incumbent, r.games, r.score, r.elo, r.decision
        );
    }
    println!();
    println!("Every rung is an independent SPRT (solo mode, seeded). A rung passes");
    println!("when the challenger's superiority is statistically proven.");
}

fn run_verify_release(games: u32) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Seed-verified release numbers — bit-reproducible evidence      ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    let v = sonar::sprt::verify_release(games, 0x5EED_CAFE);
    for m in &v.matchups {
        println!(
            "{:>18}: win rate {:5.1}%  sonar {:.1} shots  opponent {:.1} shots  ({} games)",
            m.name, m.win_rate, m.avg_shots_sonar, m.avg_shots_opponent, m.games
        );
    }
    println!();
    println!("version    : {}", v.version);
    println!("seed       : 0x{:X}", v.seed);
    println!("digest     : {}", v.digest);
    println!(
        "elapsed    : {} ms (informational — not part of the digest)",
        v.elapsed_ms
    );
    println!();
    println!(
        "Re-run `sonar verify-release {}` on any machine: the same seed",
        games
    );
    println!("reproduces these numbers and this digest bit-for-bit.");
}
