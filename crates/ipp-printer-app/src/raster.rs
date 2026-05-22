//! Raster print job trait and options (CUPS raster header semantics).

/// Failure of a print job with IPP-visible printer reasons.
#[derive(Debug, Clone)]
pub struct JobFailure {
    pub printer_reasons: crate::flags::PrinterReason,
    pub message: String,
}

impl JobFailure {
    pub fn new(
        printer_reasons: crate::flags::PrinterReason,
        message: impl Into<String>,
    ) -> Self {
        Self {
            printer_reasons,
            message: message.into(),
        }
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self::new(crate::flags::PrinterReason::OTHER, message)
    }
}

impl std::fmt::Display for JobFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for JobFailure {}

/// Page geometry from a CUPS/PWG raster page header.
#[derive(Debug, Clone)]
pub struct JobOptions {
    pub width: u32,
    pub height: u32,
    pub bits_per_pixel: u32,
    pub bytes_per_line: u32,
    pub copies: u32,
}

impl JobOptions {
    pub fn from_cups_v1(
        width: u32,
        height: u32,
        bits_per_pixel: u32,
        bytes_per_line: u32,
        num_copies: u32,
    ) -> Self {
        let copies = if num_copies < 1 { 1 } else { num_copies };
        Self {
            width,
            height,
            bits_per_pixel,
            bytes_per_line,
            copies,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bits_per_pixel(&self) -> u32 {
        self.bits_per_pixel
    }

    pub fn bytes_per_line(&self) -> u32 {
        self.bytes_per_line
    }

    pub fn copies(&self) -> u32 {
        self.copies
    }
}

/// Device-independent raster driver for label printers.
pub trait RasterDriver: Sized + Send + 'static {
    type Device: Send;

    fn start_job(
        printer: &crate::printer::PrinterHandle<'_>,
        options: &JobOptions,
        device: &Self::Device,
    ) -> Result<Self, JobFailure>;

    fn start_page(
        &mut self,
        _options: &JobOptions,
        _page: u32,
        _device: &Self::Device,
    ) -> Result<(), JobFailure> {
        Ok(())
    }

    fn write_line(
        &mut self,
        options: &JobOptions,
        y: u32,
        line: &[u8],
    ) -> Result<(), JobFailure>;

    fn end_page(
        &mut self,
        options: &JobOptions,
        page: u32,
        device: &Self::Device,
    ) -> Result<(), JobFailure>;

    fn end_job(self, device: &Self::Device);

    fn printer_status(_printer: &crate::printer::PrinterHandle<'_>) -> bool {
        true
    }
}
