//! Adapter do zewnętrznych silników statków pobranych z internetu.
//!
//! Aktualnie wspierane:
//!  - mitchelljy/battleships_ai (Python, Monte Carlo) - przez subprocess
//!
//! Protokół komunikacji (line-based JSON przez stdin/stdout):
//!  - Inicjalizacja: bot wypisuje "READY"
//!  - Pytanie o ruch: stdin: {"cmd":"move","board":[100 ints]}
//!    Pola: 0=unknown, 1=miss, 2=hit, 3=sunk
//!  - Odpowiedź: stdout: {"move":[r,c]}
//!  - Wynik ruchu: stdin: {"cmd":"result","r":R,"c":C,"result":"miss|hit|sunk"}
//!  - Koniec gry: stdin: {"cmd":"end","won":true}
//!
//! Jeśli silnik zewnętrzny nie jest dostępny, używamy fallback do ReferencePlayer.

use crate::board::{Board, ShotResult};
use crate::player::Player;
use crate::placement::PlacementConfig;
use crate::rng::Xoshiro256;
use crate::targeting::EnemyView;
use crate::time_limit::Deadline;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

/// Silnik zewnętrzny uruchamiany jako subprocess (Python)
pub struct ExternalEngine {
    pub name: String,
    pub board: Board,
    pub view: EnemyView,
    pub rng: Xoshiro256,
    pub child: Option<Mutex<Child>>,
    pub path: PathBuf,
    pub initialized: bool,
}

impl ExternalEngine {
    /// Tworzy adapter. `path` to ścieżka do skryptu Python.
    pub fn new(name: impl Into<String>, path: PathBuf) -> Self {
        let mut rng = Xoshiro256::from_seed(crate::rng::random_u64());
        let board = crate::placement::place_best_fleet(&mut rng, &PlacementConfig::default());
        Self {
            name: name.into(),
            board,
            view: EnemyView::new(),
            rng,
            child: None,
            path,
            initialized: false,
        }
    }

    /// Sprawdź czy silnik jest dostępny (skrypt istnieje, python3 działa)
    pub fn is_available(&self) -> bool {
        self.path.exists() && std::process::Command::new("python3")
            .arg("--version").stdout(Stdio::null()).stderr(Stdio::null())
            .status().is_ok()
    }

    fn ensure_started(&mut self) -> bool {
        if self.initialized { return true; }
        if !self.is_available() { return false; }

        // Wrapper skryptu - używamy "protocol.py" (jeśli istnieje) w przeciwnym razie
        // wywołujemy skrypt przez protokół JSON line-based.
        let wrapper = self.path.with_file_name("protocol.py");
        let script = if wrapper.exists() { wrapper } else { self.path.clone() };

        // -u = unbuffered (ważne dla komunikacji przez stdin/stdout)
        let result = Command::new("python3")
            .arg("-u").arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())  // stderr dziedziczy - widzimy błędy Pythona
            .spawn();

        match result {
            Ok(mut child) => {
                // Czytamy linię "READY" z stdout Pythona
                use std::io::Read;
                let mut ready_buf = [0u8; 6];
                if let Some(stdout) = child.stdout.as_mut() {
                    // Czytamy "READY\n" bajt po bajcie
                    let mut got = 0;
                    while got < 6 {
                        let mut b = [0u8; 1];
                        match stdout.read(&mut b) {
                            Ok(0) => break,
                            Ok(_) => {
                                ready_buf[got] = b[0];
                                got += 1;
                                if b[0] == b'\n' { break; }
                            }
                            Err(_) => break,
                        }
                    }
                    let _ready_str = String::from_utf8_lossy(&ready_buf[..got]);
                }
                self.child = Some(Mutex::new(child));
                self.initialized = true;
                true
            }
            Err(_) => false,
        }
    }

    /// Wyślij komendę JSON i odbierz odpowiedź
    fn send_command(&mut self, cmd: &str) -> Option<String> {
        if !self.ensure_started() { return None; }
        let child_lock = self.child.as_ref()?;
        let mut child = child_lock.lock().ok()?;

        // Pobierz stdin i stdout jako osobne referencje - to pozwala uniknąć
        // double mutable borrow przez rozdzielenie pól struktur
        let stdin_opt = child.stdin.take();
        let stdout_opt = child.stdout.take();
        let mut stdin = stdin_opt?;
        let mut stdout = stdout_opt?;

        if writeln!(stdin, "{}", cmd).is_err() {
            child.stdin = Some(stdin);
            child.stdout = Some(stdout);
            return None;
        }
        if stdin.flush().is_err() {
            child.stdin = Some(stdin);
            child.stdout = Some(stdout);
            return None;
        }

        // Czytanie bajt-po-bajcie aż do newline (bez bufora który mógłby pochłonąć dane)
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        let mut result = None;
        // Timeout: czytamy maksymalnie 60 sekund (Python jest powolny)
        let start = std::time::Instant::now();
        loop {
            if start.elapsed().as_secs() > 55 {
                break;
            }
            match stdout.read(&mut byte) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if byte[0] == b'\n' {
                        let s = String::from_utf8_lossy(&line).to_string();
                        let trimmed = s.trim().to_string();
                        if !trimmed.is_empty() {
                            result = Some(trimmed);
                            break;
                        }
                        line.clear();
                    } else {
                        line.push(byte[0]);
                    }
                }
                Err(_) => break,
            }
        }
        // Przywróć stdin/stdout do child
        child.stdin = Some(stdin);
        child.stdout = Some(stdout);
        result
    }

    /// Wyślij komendę bez oczekiwania na odpowiedź (np. result, reset, end)
    fn send_no_response(&mut self, cmd: &str) -> Option<()> {
        if !self.ensure_started() { return None; }
        let child_lock = self.child.as_ref()?;
        let mut child = child_lock.lock().ok()?;
        let mut stdin = child.stdin.take()?;
        let r = writeln!(stdin, "{}", cmd).ok()?;
        let _ = stdin.flush();
        child.stdin = Some(stdin);
        Some(r)
    }
}

impl Player for ExternalEngine {
    fn name(&self) -> &str { &self.name }
    fn board(&self) -> &Board { &self.board }
    fn board_mut(&mut self) -> &mut Board { &mut self.board }

    fn choose_move(&mut self, _deadline: Deadline) -> (usize, usize) {
        // Buduj JSON z planszą z perspektywy wroga (100 intów)
        let mut board_arr = [0u8; 100];
        for i in 0..100 {
            let r = i / 10;
            let c = i % 10;
            if !self.view.shots.test(r, c) {
                board_arr[i] = 0; // unknown
            } else if self.view.sunk.test(r, c) {
                board_arr[i] = 3; // sunk
            } else if self.view.hits.test(r, c) {
                board_arr[i] = 2; // hit
            } else {
                board_arr[i] = 1; // miss
            }
        }
        let board_str = format!("{:?}", board_arr);
        let cmd = format!(r#"{{"cmd":"move","board":{}}}"#, board_str);

        if let Some(resp) = self.send_command(&cmd) {
            // Parsuj {"move":[r,c]}
            if let Some(start) = resp.find('[') {
                if let Some(end) = resp.find(']') {
                    let arr = &resp[start+1..end];
                    let parts: Vec<&str> = arr.split(',').collect();
                    if parts.len() == 2 {
                        let r: usize = parts[0].trim().parse().unwrap_or(0);
                        let c: usize = parts[1].trim().parse().unwrap_or(0);
                        return (r, c);
                    }
                }
            }
        }
        // Fallback - losowo
        let un = self.view.unknown();
        let cells: Vec<_> = un.iter_cells().collect();
        if cells.is_empty() { return (0, 0); }
        let i = self.rng.gen_range(cells.len() as u64) as usize;
        cells[i]
    }

    fn observe_result(&mut self, r: usize, c: usize, result: ShotResult) {
        self.view.observe(r, c, result);
        let res_str = match result {
            ShotResult::Miss => "miss",
            ShotResult::Hit => "hit",
            ShotResult::Sunk(_) => "sunk",
            _ => "miss",
        };
        let cmd = format!(r#"{{"cmd":"result","r":{},"c":{},"result":"{}"}}"#, r, c, res_str);
        // "result" nie wymaga odpowiedzi - tylko wysyłamy, nie czytamy
        let _ = self.send_no_response(&cmd);
    }

    fn reset(&mut self) {
        self.view = EnemyView::new();
        let mut rng = Xoshiro256::from_seed(crate::rng::random_u64());
        self.board = crate::placement::place_best_fleet(&mut rng, &PlacementConfig::default());
        let _ = self.send_no_response(r#"{"cmd":"reset"}"#);
    }
}

impl Drop for ExternalEngine {
    fn drop(&mut self) {
        if let Some(child_lock) = &self.child {
            if let Ok(mut child) = child_lock.lock() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// Wrapper dla silnika mitchelljy/battleships_ai (Monte Carlo).
/// Wymaga zainstalowanego Python3 + numpy.
pub fn mitchelljy_engine() -> Option<ExternalEngine> {
    let candidates = [
        PathBuf::from("external/mitchelljy/protocol.py"),
        PathBuf::from("external/mitchelljy/ai.py"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("external/mitchelljy/protocol.py"),
    ];
    for p in candidates {
        let e = ExternalEngine::new("mitchelljy-MC", p.clone());
        if e.is_available() { return Some(e); }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_availability() {
        let e = mitchelljy_engine();
        // Engine może być lub nie być dostępny - test tylko że funkcja nie panics
        let _ = e;
    }
}
