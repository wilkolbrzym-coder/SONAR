//! Sonar WebAssembly cdylib — the browser entry point into the engine.
//!
//! Design: the WASM build exposes a *tiny* memory-centric ABI and lets all
//! semantics live in the JSON protocol (`sonar::json_server`). This keeps
//! a single source of truth for the command surface: the browser, the CLI
//! server, and the integration tests all speak the exact same protocol.
//!
//! ## ABI
//!
//! ```text
//! sonar_version() -> u32       // (major << 16) | (minor << 8) | patch
//! sonar_alloc(len) -> ptr      // allocate `len` bytes (1-byte aligned)
//! sonar_free(ptr, len)         // free a previous sonar_alloc(ptr, len)
//! sonar_request(ptr, len) -> u64
//!                              // bytes ptr..ptr+len = one UTF-8 JSON line;
//!                              // the reply is written to a shared buffer
//!                              // and the return value packs
//!                              //   (len << 32) | offset
//! ```
//!
//! The JS glue (see `web/engine.js`) wraps `sonar_request` into a
//! `request(jsonString) -> jsonString` helper that reads the response
//! immediately after each call and frees it.
//!
//! Safety: this crate is the only place in the Sonar project with `unsafe`,
//! and only at the FFI boundary. Pointer/length pairs are validated before
//! any dereference, allocations use the exact-layout allocator pattern,
//! and every failure path returns a well-formed protocol error instead of
//! panicking.

// The clock import (`sonar_env.now_ms`) comes from the host environment
// in the JS glue; `sonar::clock` uses it on this target.
#[allow(unsafe_code)]
mod ffi {
    use std::alloc::{Layout, alloc, dealloc};
    use std::cell::RefCell;

    // The engine instance owned by this WASM module. One module = one
    // engine (one game at a time); the `new_game` protocol command resets
    // it, `reseed` makes it deterministic.
    thread_local! {
        static ENGINE: RefCell<sonar::api::Engine> = RefCell::new(
            sonar::api::Engine::new(sonar::api::EngineConfig::default())
        );
    }

    // Shared response buffer. Replaced by every `sonar_request` call;
    // valid until the *next* call (the JS glue reads it immediately).
    thread_local! {
        static RESPONSE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    /// Allocate `len` bytes and return a pointer to them.
    ///
    /// Uses the exact-layout allocator pattern so `sonar_free(ptr, len)`
    /// reconstructs the identical layout — no capacity/length mismatch is
    /// possible.
    #[unsafe(no_mangle)]
    pub extern "C" fn sonar_alloc(len: usize) -> *mut u8 {
        let n = len.max(1);
        // SAFETY: `from_size_align(n, 1)` is always a valid layout
        // (1-byte alignment, size bounded by the caller's 16 MiB cap).
        match Layout::from_size_align(n, 1) {
            Ok(l) => unsafe { alloc(l) },
            Err(_) => std::ptr::null_mut(),
        }
    }

    /// Free a buffer previously allocated by `sonar_alloc`.
    ///
    /// # Safety
    /// `ptr` must originate from `sonar_alloc(len)` with the same `len`,
    /// and must not be freed twice.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn sonar_free(ptr: *mut u8, len: usize) {
        if ptr.is_null() {
            return;
        }
        let n = len.max(1);
        // SAFETY: the caller guarantees the pointer/length pair matches a
        // prior sonar_alloc(n) exactly.
        if let Ok(l) = Layout::from_size_align(n, 1) {
            unsafe { dealloc(ptr, l) };
        }
    }

    /// Handle one JSON request (bytes at `ptr..ptr+len`) and return
    /// `(response_len << 32) | response_offset`.
    ///
    /// The response bytes live at
    /// `response_offset..response_offset+response_len` in linear memory
    /// and stay valid until the next `sonar_request` call.
    ///
    /// # Safety
    /// `ptr..ptr+len` must be a valid readable region.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn sonar_request(ptr: *const u8, len: usize) -> u64 {
        // Validate before touching memory.
        if ptr.is_null() || len == 0 || len > 16 * 1024 * 1024 {
            return store_error_and_pack();
        }
        // SAFETY: caller guarantees the region is valid for `len` bytes.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        let line = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return store_error_and_pack(),
        };

        let reply: String = ENGINE.with(|e| {
            let Ok(mut e) = e.try_borrow_mut() else {
                return r#"{"ok":false,"error":"engine busy (reentrant call?)"}"#.to_string();
            };
            let v = sonar::json_server::handle_line(&mut e, line);
            v.to_string()
        });

        store_response_and_pack(reply.as_bytes())
    }

    fn store_error_and_pack() -> u64 {
        let msg: &[u8] = br#"{"ok":false,"error":"malformed request"}"#;
        store_response_and_pack(msg)
    }

    fn store_response_and_pack(bytes: &[u8]) -> u64 {
        // Write into the shared buffer, then pack (len, offset).
        RESPONSE.with(|r| {
            let Ok(mut buf) = r.try_borrow_mut() else {
                return 0; // cannot happen in single-threaded JS, but never UB
            };
            buf.clear();
            buf.extend_from_slice(bytes);
            let len = buf.len() as u64;
            let offset = buf.as_ptr() as u64;
            (len << 32) | offset
        })
    }
}

/// Packed version: `(major << 16) | (minor << 8) | patch`.
#[unsafe(no_mangle)]
pub extern "C" fn sonar_version() -> u32 {
    let (major, minor, patch) = parse_version(env!("CARGO_PKG_VERSION"));
    (major << 16) | (minor << 8) | patch
}

fn parse_version(v: &str) -> (u32, u32, u32) {
    // Format: "0.2.0-beta.1" → (0, 2, 0).
    let mut parts = v.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts
        .next()
        .map(|s| s.split('-').next().unwrap_or("0"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (major, minor, patch)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_version() {
        assert_eq!(super::parse_version("0.2.0-beta.1"), (0, 2, 0));
        assert_eq!(super::parse_version("1.10.3"), (1, 10, 3));
        assert_eq!(super::parse_version("weird"), (0, 0, 0));
    }
}
