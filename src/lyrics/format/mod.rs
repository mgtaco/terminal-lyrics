//! The document formats providers and local files answer in.
//!
//! Each parser takes untrusted bytes and returns [`Lyrics`]. They are ports of
//! DankMaterialShell's parsers rather than reuses of [`crate::lrc`], because the
//! point of the port is to read every document the way DMS reads it: the same
//! text, the same stamps, the same voices.
//!
//! All of them expect hostile input and cap it: [`MAX_SOURCE_BYTES`] of input,
//! [`MAX_LINES`] lines and [`MAX_WORDS`] words, past which they return
//! [`Error::TooLarge`].

pub mod lrc;
pub mod lyricsfile;
pub mod ttml;

use super::error::{Error, Result};
use super::model::Lyrics;

pub const MAX_SOURCE_BYTES: usize = 512 << 10;
pub const MAX_SIDECAR_BYTES: u64 = 256 << 10;
pub const MAX_LINES: usize = 2000;
pub const MAX_WORDS: usize = 16000;

/// The shape every parser has.
pub type Parser = fn(&[u8]) -> Result<Lyrics>;

/// Bytes to text: bounded, BOM dropped, UTF-8 or nothing.
pub(crate) fn decode(data: &[u8]) -> Result<&str> {
    if data.len() > MAX_SOURCE_BYTES {
        return Err(Error::TooLarge);
    }
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    std::str::from_utf8(data).map_err(|_| Error::Encoding)
}
