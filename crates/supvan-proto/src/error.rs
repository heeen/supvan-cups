use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("compression failed: {0}")]
    Compression(String),

    #[error("invalid parameter: {0}")]
    InvalidParam(String),

    #[error("timed out waiting for {0}")]
    Timeout(&'static str),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[cfg(feature = "ble")]
    #[error("BLE error: {0}")]
    Ble(#[from] bluer::Error),

    /// A BLE link that came up but isn't usable (no advertisement, no Supvan
    /// GATT service, connect retries exhausted).
    #[error("BLE: {0}")]
    BleSetup(String),
}

pub type Result<T> = std::result::Result<T, Error>;
