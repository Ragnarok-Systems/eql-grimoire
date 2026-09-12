//! The engine, for the browser.
//!
//! No `wasm-bindgen`, no build tooling, no npm. The boundary is three C functions and a
//! forty-line loader, which keeps the whole client a static file on a CDN — the thing
//! `HOSTING.md` says must stay true.
//!
//! ```sh
//! rustup target add wasm32-unknown-unknown
//! cargo build --release -p grimoire-wasm --target wasm32-unknown-unknown
//! # -> target/wasm32-unknown-unknown/release/grimoire_wasm.wasm   (~200 KB)
//! ```
//!
//! Everything of substance lives in [`dispatch`], which is a plain Rust function and is
//! covered by the tests in this crate. What is left here is pointer arithmetic.

pub mod dispatch;

pub use dispatch::dispatch as call;

/// Hand the caller a buffer to write into.
///
/// # Safety
/// The pointer is only valid until it is passed back to [`grimoire_free`] or consumed by
/// [`grimoire_call`]. Length must match on the way back.
#[no_mangle]
pub extern "C" fn grimoire_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr
}

/// Give a buffer back.
///
/// # Safety
/// `ptr`/`len` must be exactly what [`grimoire_alloc`] returned and handed out.
#[no_mangle]
pub unsafe extern "C" fn grimoire_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(Vec::from_raw_parts(ptr, len, len));
    }
}

thread_local! {
    /// Where the last reply landed. A wasm module is single-threaded, and this is cheaper
    /// than packing a pointer and a length into one scalar — which is what the first version
    /// did, and it truncated the pointer to 32 bits, so it segfaulted the moment anyone ran
    /// the tests on a 64-bit host. Two accessors, correct everywhere.
    static LAST: core::cell::Cell<*mut u8> = const { core::cell::Cell::new(core::ptr::null_mut()) };
}

/// Run one request. Takes UTF-8 JSON at `ptr`/`len`, returns the reply's **length**; fetch the
/// pointer with [`grimoire_result_ptr`], then hand both to [`grimoire_free`].
///
/// **The input buffer is consumed.** Do not free it afterwards.
///
/// # Safety
/// `ptr`/`len` must describe a buffer from [`grimoire_alloc`].
#[no_mangle]
pub unsafe extern "C" fn grimoire_call(ptr: *mut u8, len: usize) -> usize {
    let input = if ptr.is_null() {
        Vec::new()
    } else {
        Vec::from_raw_parts(ptr, len, len)
    };
    // Invalid UTF-8 is a caller bug, but it must not abort the module.
    let text = String::from_utf8_lossy(&input).into_owned();
    let out = dispatch::dispatch(&text).into_bytes();

    let out_len = out.len();
    let out_ptr = Box::into_raw(out.into_boxed_slice()) as *mut u8;
    LAST.with(|c| c.set(out_ptr));
    out_len
}

/// Where the last [`grimoire_call`] put its reply.
#[no_mangle]
pub extern "C" fn grimoire_result_ptr() -> *mut u8 {
    LAST.with(|c| c.get())
}

/// So a loader can refuse a mismatched pair of files.
#[no_mangle]
pub extern "C" fn grimoire_abi_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercise the real allocate → call → free path on the host, which is the only place the
    /// pointer arithmetic can be checked at all.
    /// Exactly what the JS loader does, run on the host — the only place this pointer
    /// arithmetic can be checked at all.
    unsafe fn round_trip(req: &[u8]) -> String {
        let ptr = grimoire_alloc(req.len());
        core::ptr::copy_nonoverlapping(req.as_ptr(), ptr, req.len());
        let out_len = grimoire_call(ptr, req.len());
        let out_ptr = grimoire_result_ptr();
        let text =
            String::from_utf8_lossy(std::slice::from_raw_parts(out_ptr, out_len)).into_owned();
        grimoire_free(out_ptr, out_len);
        text
    }

    #[test]
    fn the_boundary_round_trips() {
        let text = unsafe { round_trip(br#"{"op":"chance","skill":146,"trivial":146}"#) };
        assert!(text.contains("\"con\":\"grey\""), "{text}");
    }

    #[test]
    fn a_zero_length_request_does_not_trap() {
        let text = unsafe { round_trip(b"") };
        assert!(text.contains("error"), "{text}");
    }

    /// Several calls in a row must not tread on each other's replies.
    #[test]
    fn repeated_calls_do_not_corrupt_one_another() {
        for trivial in [17u16, 83, 146, 255] {
            let req = format!(r#"{{"op":"chance","skill":100,"trivial":{trivial}}}"#);
            let text = unsafe { round_trip(req.as_bytes()) };
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            let expect = grimoire_core::combine::success_chance(100, trivial);
            assert!(
                (v["chance"].as_f64().unwrap() - expect).abs() < 1e-9,
                "{text}"
            );
        }
    }

    /// A big payload goes through the same door as a small one.
    #[test]
    fn a_large_request_survives_the_boundary() {
        let log = "[Mon Aug 03 01:12:13 2026] You have become better at Baking! (7)\n".repeat(5000);
        let req = serde_json::json!({"op": "harvest", "log": log}).to_string();
        let text = unsafe { round_trip(req.as_bytes()) };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["attempts"], 0);
        assert_eq!(v["skills"][0][1], 7);
    }

    #[test]
    fn freeing_nothing_is_harmless() {
        unsafe {
            grimoire_free(core::ptr::null_mut(), 0);
            grimoire_free(core::ptr::null_mut(), 10);
        }
    }
}
