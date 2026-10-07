//! Any UTF-8 text survives compress → decompress unchanged
#![no_main]

use alice_text::{ALICEText, EncodingMode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let mut alice = ALICEText::new(EncodingMode::Pattern);
    let compressed = alice.compress(text).expect("compress");
    let restored = alice.decompress(&compressed).expect("decompress");
    assert_eq!(restored, text);
});
