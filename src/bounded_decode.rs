//! Zstd decoding with an upper bound on the decompressed size
//!
//! Every Zstd stream read by this crate comes from input that may be
//! untrusted. A frame of a few kilobytes can expand to gigabytes, so the
//! decoders never read past a limit:
//!
//! * format v3 records the decompressed size of each column; that size is the
//!   limit and the stream must expand to exactly it
//! * format v2, dialogue / localization tables and v3 columns written by
//!   earlier versions (recorded size 0) carry no size, so a fixed limit of
//!   256 MiB per stream applies. A stream holds one bincode payload of one
//!   `compress` call; 256 MiB keeps a single decode within the memory of a
//!   small machine while leaving room for logs of a few hundred megabytes.
//!   The writers refuse to produce a stream above this limit, so every file
//!   this version writes can be read back.

use crate::{ALICETextError, Result};
use std::io::Read;

/// Limit for streams whose decompressed size is not recorded (256 MiB)
pub const UNRECORDED_LIMIT: u64 = 256 * 1024 * 1024;

/// Decodes `compressed`, failing as soon as the output would exceed `limit`
///
/// # Errors
///
/// [`ALICETextError::DecompressionError`] for a stream that expands past
/// `limit` or is not valid Zstd.
pub fn decode(compressed: &[u8], limit: u64, what: &str) -> Result<Vec<u8>> {
    let decoder = zstd::stream::read::Decoder::new(compressed)
        .map_err(|e| ALICETextError::DecompressionError(format!("Zstd error in {what}: {e}")))?;
    read_bounded(decoder, limit, what)
}

/// Reads `reader` to the end, pulling at most `limit + 1` bytes from it
fn read_bounded<R: Read>(reader: R, limit: u64, what: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|e| ALICETextError::DecompressionError(format!("Zstd error in {what}: {e}")))?;
    if out.len() as u64 > limit {
        return Err(ALICETextError::DecompressionError(format!(
            "{what} expands past {limit} bytes"
        )));
    }
    Ok(out)
}

/// Decodes `compressed`, which must expand to exactly `recorded` bytes
///
/// # Errors
///
/// [`ALICETextError::DecompressionError`] when the stream expands past
/// `recorded`, to fewer bytes, or is not valid Zstd.
pub fn decode_exact(compressed: &[u8], recorded: u64, what: &str) -> Result<Vec<u8>> {
    let out = decode(compressed, recorded, what)?;
    if out.len() as u64 != recorded {
        return Err(ALICETextError::DecompressionError(format!(
            "{what} expands to {} bytes, which does not match the recorded {recorded}",
            out.len()
        )));
    }
    Ok(out)
}

/// Rejects a payload that a reader would refuse under the fixed limit
///
/// # Errors
///
/// [`ALICETextError::EncodingError`] when `len` exceeds [`UNRECORDED_LIMIT`].
pub fn check_unrecorded_payload(len: usize, what: &str) -> Result<()> {
    if len as u64 > UNRECORDED_LIMIT {
        return Err(ALICETextError::EncodingError(format!(
            "{what} of {len} bytes exceeds the {UNRECORDED_LIMIT}-byte limit of streams without a recorded size"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(n: usize) -> Vec<u8> {
        zstd::stream::encode_all(std::io::Cursor::new(vec![7u8; n]), 1).unwrap()
    }

    fn message(r: Result<Vec<u8>>) -> String {
        match r {
            Err(ALICETextError::DecompressionError(m)) => m,
            other => panic!("expected DecompressionError, got {other:?}"),
        }
    }

    #[test]
    fn limit_equal_to_the_size_decodes_and_one_less_is_rejected() {
        let f = frame(1000);
        assert_eq!(decode(&f, 1000, "t").unwrap(), vec![7u8; 1000]);
        assert_eq!(message(decode(&f, 999, "t")), "t expands past 999 bytes");
    }

    #[test]
    fn exact_size_must_match() {
        let f = frame(1000);
        assert_eq!(decode_exact(&f, 1000, "t").unwrap().len(), 1000);
        assert!(message(decode_exact(&f, 1001, "t")).contains("does not match the recorded 1001"));
        assert!(message(decode_exact(&f, 999, "t")).contains("expands past 999"));
    }

    /// Serves `total` zero bytes and counts how many were pulled
    struct Counting {
        total: u64,
        served: u64,
    }

    impl Read for Counting {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = (self.total - self.served).min(buf.len() as u64) as usize;
            buf[..n].fill(0);
            self.served += n as u64;
            Ok(n)
        }
    }

    #[test]
    fn reading_stops_one_byte_past_the_limit() {
        // a source 1000 times larger than the limit: the decoder output is
        // never pulled further than limit + 1 bytes
        let mut src = Counting {
            total: 4_000_000,
            served: 0,
        };
        let r = read_bounded(&mut src, 4000, "t");
        assert_eq!(message(r), "t expands past 4000 bytes");
        assert_eq!(src.served, 4001);
    }

    #[test]
    fn invalid_stream_is_an_error() {
        assert!(message(decode(b"not zstd", 1000, "t")).contains("Zstd error in t"));
    }

    #[test]
    fn writers_refuse_payloads_above_the_unrecorded_limit() {
        // 256 MiB = 268 435 456 bytes
        assert_eq!(UNRECORDED_LIMIT, 268_435_456);
        assert!(check_unrecorded_payload(268_435_456, "p").is_ok());
        assert!(matches!(
            check_unrecorded_payload(268_435_457, "p"),
            Err(ALICETextError::EncodingError(_))
        ));
    }
}
