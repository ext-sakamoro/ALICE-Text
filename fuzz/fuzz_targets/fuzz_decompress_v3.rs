//! Untrusted bytes into the format v3 decoder: an error is fine, a panic or an
//! abort is a defect
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = alice_text::decompress_v3(data);
});
