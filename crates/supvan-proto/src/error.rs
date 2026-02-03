use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("compression failed: {0}")]
    Compression(String),

    #[error("invalid parameter: {0}")]
    InvalidParam(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
