//! Why a lookup did not produce lyrics.
//!
//! The split that matters is "not found" against everything else: a provider
//! that says it has nothing is cached as a miss, while one that failed is asked
//! again next time.

/// One provider's failure, or the whole lookup's.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("lyrics not found")]
    NotFound,
    /// BetterLyrics answers 401 for a track its server has not cached yet,
    /// which another client can change at any moment, so this is never cached.
    #[error("lyrics not found: not cached by provider")]
    Uncached,
    /// Sync claimed but every stamp is zero; LyricsPlus does this for some
    /// Apple tracks, and the next provider usually has real timing.
    #[error("lyrics not found: synced lyrics without timing")]
    UntimedSync,
    /// A non-2xx answer that is not a plain "no such track".
    #[error("HTTP {0}")]
    Status(u16),
    #[error("lyrics too large")]
    TooLarge,
    #[error("lyrics are not valid utf-8")]
    Encoding,
    #[error("rate limited")]
    RateLimited,
    #[error("lyrics lookup timed out")]
    Timeout,
    /// A document or response that does not parse.
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

impl Error {
    /// The provider answered, and the answer was "nothing here".
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::NotFound | Error::Uncached | Error::UntimedSync)
    }

    pub(crate) fn invalid(what: impl std::fmt::Display) -> Self {
        Error::Invalid(what.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
