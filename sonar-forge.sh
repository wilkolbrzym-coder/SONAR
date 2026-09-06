#!/usr/bin/env bash
# sonar-forge — one-command build & test orchestrator.
#
# Builds everything this OS can build, tests everything it built, and
# reports pass/fail. POSIX-ish bash; no PowerShell/Python glue to drift.
#
# Usage:
#   ./sonar-forge.sh              # full pipeline (native + wasm + web + tests)
#   ./sonar-forge.sh --quick      # skip long benchmarks, 20-game strength check
#   ./sonar-forge.sh --only <step>
#        steps: build, test, bench, wasm, web, all
set -euo pipefail
cd "$(dirname "$0")"

QUICK=0
ONLY=""
for arg in "$@"; do
    case "$arg" in
        --quick) QUICK=1 ;;
        --only) shift ;;
        *) if [[ "$PREV" == "--only" || "$PREV" == "only" ]]; then ONLY="$arg"; fi ;;
    esac
    PREV="$arg"
done
# Simpler: parse "--only <step>" directly.
for ((i=1; i<=$#; i++)); do
    if [[ "${!i}" == "--only" ]]; then
        j=$((i+1))
        ONLY="${!j}"
    fi
done

STEP=0
step() { STEP=$((STEP+1)); printf "\n\033[1;36m[step %d] %s\033[0m\n" "$STEP" "$1"; }
fail() { printf "\033[1;31m[FAIL] %s\033[0m\n" "$1"; FAILURES=$((FAILURES+1)); }
pass() { printf "\033[1;32m[ ok ]\033[0m %s\n" "$1"; }

FAILURES=0
STARTED=$(date +%s)

want() { [[ -z "$ONLY" || "$ONLY" == "all" || "$ONLY" == "$1" ]]; }

# ── 1. Native build ──────────────────────────────────────────────────────────
if want build; then
    step "Build native (release) — CLI + library"
    if cargo build --release; then pass "native build"; else fail "native build"; fi
fi

# ── 2. Test suite ────────────────────────────────────────────────────────────
if want test; then
    step "Test suite (cargo test --release)"
    if cargo test --release; then pass "rust test suite"; else fail "rust test suite"; fi
fi

# ── 3. Benchmarks (strength + throughput) ───────────────────────────────────
if want bench; then
    if [[ "$QUICK" == "1" ]]; then
        step "Quick strength check (20 games vs random)"
        if ./target/release/sonar bench-fast >/dev/null 2>&1; then
            pass "quick benchmark"
        else
            fail "quick benchmark"
        fi
    else
        step "Full benchmark pass (100 games, Wilson CIs)"
        if ./target/release/sonar bench; then pass "benchmarks"; else fail "benchmarks"; fi
    fi
fi

# ── 4. WebAssembly build ─────────────────────────────────────────────────────
if want wasm; then
    step "Build WebAssembly (sonar-wasm)"
    if ./scripts/build-wasm.sh; then pass "wasm build"; else fail "wasm build"; fi
fi

# ── 5. Web verification (Node) ───────────────────────────────────────────────
if want web; then
    step "Web E2E verification (Node)"
    if command -v node >/dev/null 2>&1; then
        OK=1
        node scripts/test-web.mjs >/dev/null 2>&1 || OK=0
        node scripts/test-suite.mjs >/dev/null 2>&1 || OK=0
        node scripts/test-mods.mjs >/dev/null 2>&1 || OK=0
        if [[ "$OK" == "1" ]]; then pass "web E2E (engine, suite, mods)"; else fail "web E2E"; fi
    else
        echo "Node not found — skipped (not a failure)."
    fi
fi

# ── Report ───────────────────────────────────────────────────────────────────
ELAPSED=$(( $(date +%s) - STARTED ))
echo ""
echo "════════════════════════════════════════════"
if [[ "$FAILURES" == "0" ]]; then
    echo "  sonar-forge: ALL GREEN ($ELAPSED s)"
    echo "════════════════════════════════════════════"
else
    echo "  sonar-forge: $FAILURES FAILURE(S) ($ELAPSED s)"
    echo "════════════════════════════════════════════"
    exit 1
fi
