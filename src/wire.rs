//! The bincode 1 layout of the payloads this crate stores, through `alice_serial::bincode1`
//!
//! The v2 payload, the v3 columns and the dialogue / localization tables are bincode 1.x
//! encodings on disk. `alice_serial::bincode1` reads and writes the same bytes without the
//! bincode crate (`src/bincode1_fixtures.rs` pins them against bytes bincode 1.3.3 wrote).
//! Each stored stream holds exactly one payload, so a decode refuses bytes after it.

use alice_serial::bincode1::{from_slice, to_vec, Bincode1Error};
use serde::{Deserialize, Serialize};

use crate::bounded_decode::UNRECORDED_LIMIT;

/// The largest payload one stream may hold (as the readers bound it).
const LIMIT: usize = UNRECORDED_LIMIT as usize;

/// `value` in the bincode 1 layout, at most [`UNRECORDED_LIMIT`] bytes.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Bincode1Error> {
    to_vec(value, LIMIT)
}

/// The one value `bytes` holds; bytes after it are refused.
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, Bincode1Error> {
    from_slice(bytes, bytes.len())
}
