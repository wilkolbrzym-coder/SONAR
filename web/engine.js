/**
 * Sonar WebAssembly engine glue.
 *
 * Environment-agnostic: works in browsers and Node.js (both expose the
 * `WebAssembly` API). The only host requirement is a `fetchBytes` loader
 * supplied by the caller (see `worker.js` for the browser version and
 * `scripts/test-web.mjs` for the Node version).
 *
 * The engine exposes a tiny C-ABI (see crates/sonar-wasm/src/lib.rs):
 *
 *   sonar_version() -> u32
 *   sonar_alloc(len) -> ptr
 *   sonar_free(ptr, len)
 *   sonar_request(ptr, len) -> (len << 32) | offset
 *
 * `createEngine` wraps that into a friendly `request(jsonString)` API and
 * adds a typed `cmd` helper. All game semantics live in the JSON protocol
 * (`sonar::json_server`) — one source of truth shared with the CLI.
 */

/** Random 64-bit seed from the host's cryptographic RNG. */
export function cryptoSeed() {
  const g =
    globalThis.crypto ||
    (globalThis.require && globalThis.require("crypto")) ||
    null;
  if (g && typeof g.getRandomValues === "function") {
    const buf = new Uint8Array(8);
    g.getRandomValues(buf);
    let v = 0n;
    for (const b of buf) v = (v << 8n) | BigInt(b);
    return v;
  }
  // Fallback: time + counter (not cryptographic, but always available).
  let v = BigInt(Date.now() & 0xffffffff) << 32n;
  v ^= BigInt(Math.floor(Math.random() * 0xffffffff));
  return v & 0xffffffffffffffffn;
}

/**
 * Instantiate the Sonar engine from WASM bytes.
 *
 * @param {Uint8Array | ArrayBuffer} wasmBytes - the compiled sonar_wasm.wasm
 * @returns {Promise<object>} engine handle with:
 *   - `version(): {major, minor, patch, packed}`
 *   - `request(line: string): object` — send one JSON protocol line, get
 *     the parsed reply
 *   - `requestRaw(line: string): string` — same, raw string reply
 *   - `cmd(name, extra?): object` — convenience wrapper
 */
export async function createEngine(wasmBytes) {
  /** Host imports: the monotonic clock used by `sonar::clock`. */
  const imports = {
    sonar_env: {
      now_ms: () =>
        typeof performance !== "undefined" && performance.now
          ? performance.now()
          : Date.now(),
    },
  };

  const { instance } = await WebAssembly.instantiate(wasmBytes, imports);
  const ex = instance.exports;

  // ── Buffer dance ────────────────────────────────────────────────────────
  // Write a request string into engine memory, call sonar_request, read
  // the reply from the shared buffer, then free our request buffer.

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  function requestRaw(line) {
    const bytes = encoder.encode(line);
    const ptr = ex.sonar_alloc(bytes.length);
    if (ptr === 0) {
      throw new Error("sonar_alloc failed");
    }
    const mem = new Uint8Array(ex.memory.buffer, ptr, bytes.length);
    mem.set(bytes);

    const packed = ex.sonar_request(ptr, bytes.length);
    ex.sonar_free(ptr, bytes.length);

    const len = Number(packed >> 32n);
    const offset = Number(packed & 0xffffffffn);
    if (len === 0 && offset === 0) {
      throw new Error("sonar_request failed (engine busy?)");
    }
    // Read the response immediately — it is only valid until the next
    // call (see the ABI contract).
    const out = new Uint8Array(ex.memory.buffer, offset, len);
    const text = decoder.decode(out);
    return text;
  }

  function request(line) {
    return JSON.parse(requestRaw(line));
  }

  function cmd(name, extra = {}) {
    return request(JSON.stringify({ cmd: name, ...extra }));
  }

  const packedVersion = ex.sonar_version() >>> 0;
  const version = {
    major: (packedVersion >>> 16) & 0xff,
    minor: (packedVersion >>> 8) & 0xff,
    patch: packedVersion & 0xff,
    packed: packedVersion,
  };

  // Protocol handshake: verify the engine reports the beta channel and
  // wire protocol 1.
  const hello = cmd("version");
  if (hello.protocol !== 1) {
    throw new Error(`protocol mismatch: engine speaks v${hello.protocol}`);
  }

  return {
    version,
    hello,
    request,
    requestRaw,
    cmd,
    memory: ex.memory,
    exports: ex,
  };
}
