//! Typed wrappers for PAPPL's integer flag enums.
//!
//! The `pappl-sys` bindings expose these as plain integer constants
//! (`pappl_preason_e_PAPPL_PREASON_*`, etc.). This module turns them into
//! bitflags so callers don't have to write `0x0800 | 0x0002` to mean
//! "offline + cover open".
//!
//! `From`/`Into` conversions are provided in both directions, so raw
//! integers from PAPPL callbacks can be wrapped, and the underlying
//! integer can be retrieved via `.bits()` or `.into()` to pass back to
//! PAPPL functions that still take the raw type.

use bitflags::bitflags;
use pappl_sys::*;

bitflags! {
    /// Printer state reason flags (pappl_preason_t).
    ///
    /// Describes why a printer is in its current state (offline, cover open, etc.).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct PrinterReason: pappl_preason_t {
        const NONE = 0x0000;
        const OTHER = 0x0001;
        const COVER_OPEN = 0x0002;
        const INPUT_TRAY_MISSING = 0x0004;
        const MARKER_SUPPLY_EMPTY = 0x0008;
        const MARKER_SUPPLY_LOW = 0x0010;
        const MARKER_WASTE_ALMOST_FULL = 0x0020;
        const MARKER_WASTE_FULL = 0x0040;
        const MEDIA_EMPTY = 0x0080;
        const MEDIA_JAM = 0x0100;
        const MEDIA_LOW = 0x0200;
        const MEDIA_NEEDED = 0x0400;
        const OFFLINE = 0x0800;
        const SPOOL_AREA_FULL = 0x1000;
        const TONER_EMPTY = 0x2000;
        const TONER_LOW = 0x4000;
        const DOOR_OPEN = 0x8000;
        const IDENTIFY_PRINTER_REQUESTED = 0x10000;
        const DEVICE_STATUS = 0xF01F;
    }
}

impl From<PrinterReason> for pappl_preason_t {
    fn from(v: PrinterReason) -> Self {
        v.bits()
    }
}

impl From<pappl_preason_t> for PrinterReason {
    fn from(v: pappl_preason_t) -> Self {
        Self::from_bits_truncate(v)
    }
}

bitflags! {
    /// Job state reason flags (pappl_jreason_t).
    ///
    /// Describes why a job is in its current state (queued, printing, completed, etc.).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct JobReason: pappl_jreason_t {
        const NONE = 0x0000_0000;
        const ABORTED_BY_SYSTEM = 0x0000_0001;
        const COMPRESSION_ERROR = 0x0000_0002;
        const DOCUMENT_FORMAT_ERROR = 0x0000_0004;
        const DOCUMENT_PASSWORD_ERROR = 0x0000_0008;
        const DOCUMENT_PERMISSION_ERROR = 0x0000_0010;
        const DOCUMENT_UNPRINTABLE_ERROR = 0x0000_0020;
        const ERRORS_DETECTED = 0x0000_0040;
        const JOB_CANCELED_AT_DEVICE = 0x0000_0080;
        const JOB_CANCELED_BY_USER = 0x0000_0100;
        const JOB_COMPLETED_SUCCESSFULLY = 0x0000_0200;
        const JOB_COMPLETED_WITH_ERRORS = 0x0000_0400;
        const JOB_COMPLETED_WITH_WARNINGS = 0x0000_0800;
        const JOB_DATA_INSUFFICIENT = 0x0000_1000;
        const JOB_INCOMING = 0x0000_2000;
        const JOB_PRINTING = 0x0000_4000;
        const JOB_QUEUED = 0x0000_8000;
        const JOB_SPOOLING = 0x0001_0000;
        const PRINTER_STOPPED = 0x0002_0000;
        const PRINTER_STOPPED_PARTLY = 0x0004_0000;
        const PROCESSING_TO_STOP_POINT = 0x0008_0000;
        const QUEUED_IN_DEVICE = 0x0010_0000;
        const WARNINGS_DETECTED = 0x0020_0000;
        const JOB_HOLD_UNTIL_SPECIFIED = 0x0040_0000;
        const JOB_CANCELED_AFTER_TIMEOUT = 0x0080_0000;
        const JOB_FETCHABLE = 0x0100_0000;
        const JOB_SUSPENDED_FOR_APPROVAL = 0x0200_0000;
    }
}

impl From<JobReason> for pappl_jreason_t {
    fn from(v: JobReason) -> Self {
        v.bits()
    }
}

impl From<pappl_jreason_t> for JobReason {
    fn from(v: pappl_jreason_t) -> Self {
        Self::from_bits_truncate(v)
    }
}

bitflags! {
    /// System options flags (pappl_soptions_t).
    ///
    /// Describes which system features are enabled.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct SystemOptions: pappl_soptions_t {
        const NONE = 0x0000;
        const DNSSD_HOST = 0x0001;
        const MULTI_QUEUE = 0x0002;
        const RAW_SOCKET = 0x0004;
        const USB_PRINTER = 0x0008;
        const WEB_INTERFACE = 0x0010;
        const WEB_LOG = 0x0020;
        const WEB_NETWORK = 0x0040;
        const WEB_REMOTE = 0x0080;
        const WEB_SECURITY = 0x0100;
        const WEB_TLS = 0x0200;
        const NO_TLS = 0x0400;
    }
}

impl From<SystemOptions> for pappl_soptions_t {
    fn from(v: SystemOptions) -> Self {
        v.bits()
    }
}

impl From<pappl_soptions_t> for SystemOptions {
    fn from(v: pappl_soptions_t) -> Self {
        Self::from_bits_truncate(v)
    }
}

bitflags! {
    /// Device type flags (pappl_devtype_t).
    ///
    /// Describes what type of device connection is used.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct DeviceType: pappl_devtype_t {
        const FILE = 0x01;
        const USB = 0x02;
        const SERIAL = 0x04;
        const CUSTOM_LOCAL = 0x08;
        const SOCKET = 0x10;
        const DNS_SD = 0x20;
        const SNMP = 0x40;
        const CUSTOM_NETWORK = 0x80;
        const LOCAL = 0x0F;
        const NETWORK = 0xF0;
        const ALL = 0xFF;
    }
}

impl From<DeviceType> for pappl_devtype_t {
    fn from(v: DeviceType) -> Self {
        v.bits()
    }
}

impl From<pappl_devtype_t> for DeviceType {
    fn from(v: pappl_devtype_t) -> Self {
        Self::from_bits_truncate(v)
    }
}

/// Log level enum (pappl_loglevel_t).
///
/// Represents the severity level for log messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(i32)]
pub enum LogLevel {
    /// Unspecified level.
    Unspec = -1,
    /// Debug-level messages.
    Debug = 0,
    /// Informational messages.
    #[default]
    Info = 1,
    /// Warning messages.
    Warn = 2,
    /// Error messages.
    Error = 3,
    /// Fatal error messages.
    Fatal = 4,
}

impl From<LogLevel> for pappl_loglevel_t {
    fn from(level: LogLevel) -> Self {
        level as pappl_loglevel_t
    }
}

/// Returned by `LogLevel::try_from` when the integer is outside the
/// known PAPPL log level range. Kept distinct from `LogLevel::Error`
/// (the level variant) to avoid the ambiguous-associated-items lint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLogLevel(pub pappl_loglevel_t);

impl std::fmt::Display for InvalidLogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid pappl_loglevel_t value: {}", self.0)
    }
}

impl std::error::Error for InvalidLogLevel {}

impl TryFrom<pappl_loglevel_t> for LogLevel {
    type Error = InvalidLogLevel;

    fn try_from(v: pappl_loglevel_t) -> Result<Self, InvalidLogLevel> {
        match v {
            -1 => Ok(LogLevel::Unspec),
            0 => Ok(LogLevel::Debug),
            1 => Ok(LogLevel::Info),
            2 => Ok(LogLevel::Warn),
            3 => Ok(LogLevel::Error),
            4 => Ok(LogLevel::Fatal),
            _ => Err(InvalidLogLevel(v)),
        }
    }
}

