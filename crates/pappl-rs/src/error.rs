use thiserror::Error;

/// Errors that can arise calling into PAPPL.
#[derive(Debug, Error)]
pub enum Error {
    /// The linked PAPPL is too old for this operation (e.g. pre-1.4 for
    /// `papplSystemCreatePrinters`).
    #[error("operation not supported by linked PAPPL version")]
    Unsupported,

    /// A `*const c_char` PAPPL handed us wasn't valid UTF-8.
    #[error("PAPPL returned non-UTF-8 string: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// A string we tried to pass into PAPPL contained an interior NUL.
    #[error("string contained interior NUL: {0}")]
    InteriorNul(#[from] std::ffi::NulError),

    /// A NULL pointer where PAPPL was expected to return a valid handle.
    #[error("PAPPL returned NULL: {0}")]
    Null(&'static str),

    /// A library-level error not covered above. Boxed so this enum stays
    /// cheap to clone and the variant list stable.
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
