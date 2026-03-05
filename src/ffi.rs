//! C-ABI FFI bindings for ALICE-Text
//!
//! 20 `extern "C"` functions for text compression, dialogue, and entropy estimation.
//!
//! Author: Moroya Sakamoto

use crate::dialogue::{DialogueEntry, DialogueTable};
use crate::entropy_estimator::EntropyEstimator;
use crate::tuned_compressor::CompressionMode;
use crate::{ALICEText, EncodingMode};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// ============================================================================
// Opaque handles
// ============================================================================

/// Opaque handle to ALICEText compressor
pub type AliceTextHandle = *mut ALICEText;

/// Opaque handle to DialogueTable
pub type AliceDialogueTableHandle = *mut DialogueTable;

// ============================================================================
// C-compatible result for compressed data
// ============================================================================

/// Compressed data result returned from FFI
#[repr(C)]
pub struct AliceTextCompressedData {
    pub data: *mut u8,
    pub len: u32,
}

/// Compression statistics returned from FFI
#[repr(C)]
pub struct AliceTextStats {
    pub original_size: u64,
    pub compressed_size: u64,
    pub compression_ratio: f64,
    pub space_savings: f64,
}

/// Entropy estimation result returned from FFI
#[repr(C)]
pub struct AliceTextEntropy {
    pub shannon_entropy: f64,
    pub estimated_ratio: f64,
    pub estimated_size: u64,
    pub original_size: u64,
    pub space_savings: f64,
    pub pattern_coverage: f64,
    pub unique_bytes: u32,
    pub repetition_score: f64,
    pub is_compressible: u8,
}

// ============================================================================
// World lifecycle
// ============================================================================

/// Create a new ALICEText compressor instance.
#[no_mangle]
pub extern "C" fn alice_text_create() -> AliceTextHandle {
    let instance = Box::new(ALICEText::new(EncodingMode::Pattern));
    Box::into_raw(instance)
}

/// Destroy an ALICEText compressor instance.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `alice_text_create`.
#[no_mangle]
pub unsafe extern "C" fn alice_text_destroy(handle: AliceTextHandle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

// ============================================================================
// Compress / Decompress
// ============================================================================

/// Compress a UTF-8 string. Returns compressed data.
/// Caller must free with `alice_text_data_free`.
///
/// # Safety
///
/// `handle` must be valid. `text` must be a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn alice_text_compress(
    handle: AliceTextHandle,
    text: *const c_char,
) -> AliceTextCompressedData {
    let empty = AliceTextCompressedData {
        data: std::ptr::null_mut(),
        len: 0,
    };
    if handle.is_null() || text.is_null() {
        return empty;
    }
    let alice = unsafe { &mut *handle };
    let c_str = unsafe { CStr::from_ptr(text) };
    let text_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return empty,
    };
    match alice.compress(text_str) {
        Ok(mut data) => {
            let len = data.len() as u32;
            let ptr = data.as_mut_ptr();
            std::mem::forget(data);
            AliceTextCompressedData { data: ptr, len }
        }
        Err(_) => empty,
    }
}

/// Decompress data back to a UTF-8 string.
/// Caller must free the returned string with `alice_text_string_free`.
///
/// # Safety
///
/// `handle` must be valid. `data`/`len` must be a valid compressed buffer.
#[no_mangle]
pub unsafe extern "C" fn alice_text_decompress(
    handle: AliceTextHandle,
    data: *const u8,
    len: u32,
) -> *mut c_char {
    if handle.is_null() || data.is_null() {
        return std::ptr::null_mut();
    }
    let alice = unsafe { &*handle };
    let slice = unsafe { std::slice::from_raw_parts(data, len as usize) };
    match alice.decompress(slice) {
        Ok(text) => match CString::new(text) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        Err(_) => std::ptr::null_mut(),
    }
}

/// Compress with a specified mode (0=Fast, 1=Balanced, 2=Best).
/// Caller must free with `alice_text_data_free`.
///
/// # Safety
///
/// `text` must be a null-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn alice_text_compress_tuned(
    text: *const c_char,
    mode: u8,
) -> AliceTextCompressedData {
    let empty = AliceTextCompressedData {
        data: std::ptr::null_mut(),
        len: 0,
    };
    if text.is_null() {
        return empty;
    }
    let c_str = unsafe { CStr::from_ptr(text) };
    let text_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return empty,
    };
    let compression_mode = match mode {
        0 => CompressionMode::Fast,
        2 => CompressionMode::Best,
        _ => CompressionMode::Balanced,
    };
    match crate::compress_tuned(text_str, compression_mode) {
        Ok(mut data) => {
            let len = data.len() as u32;
            let ptr = data.as_mut_ptr();
            std::mem::forget(data);
            AliceTextCompressedData { data: ptr, len }
        }
        Err(_) => empty,
    }
}

/// Decompress tuned-compressed data.
/// Caller must free the returned string with `alice_text_string_free`.
///
/// # Safety
///
/// `data`/`len` must be a valid compressed buffer.
#[no_mangle]
pub unsafe extern "C" fn alice_text_decompress_tuned(data: *const u8, len: u32) -> *mut c_char {
    if data.is_null() {
        return std::ptr::null_mut();
    }
    let slice = unsafe { std::slice::from_raw_parts(data, len as usize) };
    match crate::decompress_tuned(slice) {
        Ok(text) => match CString::new(text) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        Err(_) => std::ptr::null_mut(),
    }
}

// ============================================================================
// Stats / Entropy
// ============================================================================

/// Get last compression statistics.
///
/// # Safety
///
/// `handle` must be valid. `out` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn alice_text_get_stats(
    handle: AliceTextHandle,
    out: *mut AliceTextStats,
) -> u8 {
    if handle.is_null() || out.is_null() {
        return 0;
    }
    let alice = unsafe { &*handle };
    match alice.last_stats() {
        Some(stats) => {
            unsafe {
                (*out).original_size = stats.original_size as u64;
                (*out).compressed_size = stats.compressed_size as u64;
                (*out).compression_ratio = stats.compression_ratio();
                (*out).space_savings = stats.space_savings();
            }
            1
        }
        None => 0,
    }
}

/// Estimate entropy and compression quality for text.
///
/// # Safety
///
/// `text` must be a null-terminated UTF-8 string. `out` must be valid.
#[no_mangle]
pub unsafe extern "C" fn alice_text_estimate_entropy(
    text: *const c_char,
    out: *mut AliceTextEntropy,
) -> u8 {
    if text.is_null() || out.is_null() {
        return 0;
    }
    let c_str = unsafe { CStr::from_ptr(text) };
    let text_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let estimator = EntropyEstimator::new();
    let est = estimator.estimate(text_str);
    unsafe {
        (*out).shannon_entropy = est.shannon_entropy;
        (*out).estimated_ratio = est.estimated_ratio;
        (*out).estimated_size = est.estimated_size as u64;
        (*out).original_size = est.original_size as u64;
        (*out).space_savings = est.space_savings;
        (*out).pattern_coverage = est.pattern_coverage;
        (*out).unique_bytes = est.unique_bytes as u32;
        (*out).repetition_score = est.repetition_score;
        (*out).is_compressible = est.is_compressible() as u8;
    }
    1
}

// ============================================================================
// Dialogue table
// ============================================================================

/// Create a new dialogue table.
#[no_mangle]
pub extern "C" fn alice_text_dialogue_create() -> AliceDialogueTableHandle {
    Box::into_raw(Box::new(DialogueTable::new()))
}

/// Destroy a dialogue table.
///
/// # Safety
///
/// `handle` must be a valid pointer returned by `alice_text_dialogue_create`.
#[no_mangle]
pub unsafe extern "C" fn alice_text_dialogue_destroy(handle: AliceDialogueTableHandle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Add a dialogue entry to the table.
///
/// # Safety
///
/// `handle` must be valid. `speaker`/`text` must be null-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn alice_text_dialogue_add(
    handle: AliceDialogueTableHandle,
    id: u32,
    speaker: *const c_char,
    text: *const c_char,
) -> u8 {
    if handle.is_null() || speaker.is_null() || text.is_null() {
        return 0;
    }
    let table = unsafe { &mut *handle };
    let speaker_str = match unsafe { CStr::from_ptr(speaker) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let text_str = match unsafe { CStr::from_ptr(text) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let speaker_idx = table.speakers.insert(speaker_str);
    table.add(DialogueEntry {
        id,
        speaker: speaker_idx,
        text: text_str.to_string(),
        ruby: None,
    });
    1
}

/// Get dialogue text by ID.
/// Caller must free the returned string with `alice_text_string_free`.
///
/// # Safety
///
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn alice_text_dialogue_get(
    handle: AliceDialogueTableHandle,
    id: u32,
) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    let table = unsafe { &*handle };
    match table.get(id) {
        Some(entry) => match CString::new(entry.text.clone()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

/// Get dialogue entry count.
///
/// # Safety
///
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn alice_text_dialogue_count(handle: AliceDialogueTableHandle) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let table = unsafe { &*handle };
    table.len() as u32
}

/// Get unique character count across all dialogue (useful for font atlas).
///
/// # Safety
///
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn alice_text_dialogue_unique_chars(handle: AliceDialogueTableHandle) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let table = unsafe { &*handle };
    table.unique_chars().len() as u32
}

// ============================================================================
// Memory management
// ============================================================================

/// Free compressed data returned by compress functions.
///
/// # Safety
///
/// `data`/`len` must be from a previous compress call.
#[no_mangle]
pub unsafe extern "C" fn alice_text_data_free(data: *mut u8, len: u32) {
    if !data.is_null() {
        drop(unsafe { Vec::from_raw_parts(data, len as usize, len as usize) });
    }
}

/// Free a string returned by decompress/dialogue functions.
///
/// # Safety
///
/// `s` must be a pointer returned by an alice_text FFI function.
#[no_mangle]
pub unsafe extern "C" fn alice_text_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

// ============================================================================
// Version
// ============================================================================

/// Get library version string. Returns a static null-terminated string.
#[no_mangle]
pub extern "C" fn alice_text_version() -> *const c_char {
    c"1.0.0".as_ptr()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_create_destroy() {
        let handle = alice_text_create();
        assert!(!handle.is_null());
        unsafe { alice_text_destroy(handle) };
    }

    #[test]
    fn test_compress_decompress_roundtrip() {
        let handle = alice_text_create();
        let text = CString::new("Hello, ALICE-Text FFI!").unwrap();
        let compressed = unsafe { alice_text_compress(handle, text.as_ptr()) };
        assert!(!compressed.data.is_null());
        assert!(compressed.len > 0);

        let decompressed =
            unsafe { alice_text_decompress(handle, compressed.data, compressed.len) };
        assert!(!decompressed.is_null());
        let result = unsafe { CStr::from_ptr(decompressed) }.to_str().unwrap();
        assert_eq!(result, "Hello, ALICE-Text FFI!");

        unsafe {
            alice_text_string_free(decompressed);
            alice_text_data_free(compressed.data, compressed.len);
            alice_text_destroy(handle);
        }
    }

    #[test]
    fn test_compress_tuned_roundtrip() {
        let text = CString::new("Tuned compression test data").unwrap();
        let compressed = unsafe { alice_text_compress_tuned(text.as_ptr(), 1) };
        assert!(!compressed.data.is_null());

        let decompressed = unsafe { alice_text_decompress_tuned(compressed.data, compressed.len) };
        assert!(!decompressed.is_null());
        let result = unsafe { CStr::from_ptr(decompressed) }.to_str().unwrap();
        assert_eq!(result, "Tuned compression test data");

        unsafe {
            alice_text_string_free(decompressed);
            alice_text_data_free(compressed.data, compressed.len);
        }
    }

    #[test]
    fn test_stats() {
        let handle = alice_text_create();
        let text = CString::new("Test stats".repeat(50)).unwrap();
        let compressed = unsafe { alice_text_compress(handle, text.as_ptr()) };
        let mut stats = AliceTextStats {
            original_size: 0,
            compressed_size: 0,
            compression_ratio: 0.0,
            space_savings: 0.0,
        };
        let ok = unsafe { alice_text_get_stats(handle, &mut stats) };
        assert_eq!(ok, 1);
        assert!(stats.original_size > 0);

        unsafe {
            alice_text_data_free(compressed.data, compressed.len);
            alice_text_destroy(handle);
        }
    }

    #[test]
    fn test_estimate_entropy() {
        let text = CString::new("Hello world ".repeat(100)).unwrap();
        let mut entropy = AliceTextEntropy {
            shannon_entropy: 0.0,
            estimated_ratio: 0.0,
            estimated_size: 0,
            original_size: 0,
            space_savings: 0.0,
            pattern_coverage: 0.0,
            unique_bytes: 0,
            repetition_score: 0.0,
            is_compressible: 0,
        };
        let ok = unsafe { alice_text_estimate_entropy(text.as_ptr(), &mut entropy) };
        assert_eq!(ok, 1);
        assert!(entropy.shannon_entropy > 0.0);
        assert!(entropy.original_size > 0);
    }

    #[test]
    fn test_dialogue_lifecycle() {
        let handle = alice_text_dialogue_create();
        assert!(!handle.is_null());

        let speaker = CString::new("Alice").unwrap();
        let text = CString::new("Hello world!").unwrap();
        let ok = unsafe { alice_text_dialogue_add(handle, 0, speaker.as_ptr(), text.as_ptr()) };
        assert_eq!(ok, 1);
        assert_eq!(unsafe { alice_text_dialogue_count(handle) }, 1);

        let got = unsafe { alice_text_dialogue_get(handle, 0) };
        assert!(!got.is_null());
        let result = unsafe { CStr::from_ptr(got) }.to_str().unwrap();
        assert_eq!(result, "Hello world!");

        unsafe {
            alice_text_string_free(got);
            alice_text_dialogue_destroy(handle);
        }
    }

    #[test]
    fn test_dialogue_unique_chars() {
        let handle = alice_text_dialogue_create();
        let speaker = CString::new("Bob").unwrap();
        let text = CString::new("ABCABC").unwrap();
        unsafe { alice_text_dialogue_add(handle, 0, speaker.as_ptr(), text.as_ptr()) };
        let count = unsafe { alice_text_dialogue_unique_chars(handle) };
        assert_eq!(count, 3); // A, B, C
        unsafe { alice_text_dialogue_destroy(handle) };
    }

    #[test]
    fn test_null_safety() {
        let empty = unsafe { alice_text_compress(std::ptr::null_mut(), std::ptr::null()) };
        assert!(empty.data.is_null());
        let null_str = unsafe { alice_text_decompress(std::ptr::null_mut(), std::ptr::null(), 0) };
        assert!(null_str.is_null());
        unsafe { alice_text_destroy(std::ptr::null_mut()) };
        unsafe { alice_text_dialogue_destroy(std::ptr::null_mut()) };
    }

    #[test]
    fn test_version() {
        let v = alice_text_version();
        assert!(!v.is_null());
        let version = unsafe { CStr::from_ptr(v) }.to_str().unwrap();
        assert_eq!(version, "1.0.0");
    }

    #[test]
    fn test_compress_modes() {
        let text = CString::new("Mode test data for compression").unwrap();
        for mode in [0u8, 1, 2] {
            let compressed = unsafe { alice_text_compress_tuned(text.as_ptr(), mode) };
            assert!(!compressed.data.is_null());
            unsafe { alice_text_data_free(compressed.data, compressed.len) };
        }
    }
}
