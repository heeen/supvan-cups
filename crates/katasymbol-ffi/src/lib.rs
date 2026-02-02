//! C FFI bridge for katasymbol-proto.
//!
//! Provides a C-compatible API for PAPPL printer application integration.
//! All exported functions use `extern "C"` and raw pointers for interop.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};

use katasymbol_proto::bitmap::{center_in_printhead, raster_to_column_major, PRINTHEAD_WIDTH_MM};
use katasymbol_proto::buffer::split_into_buffers;
use katasymbol_proto::compress::compress_buffers;
use katasymbol_proto::printer::Printer;
use katasymbol_proto::rfcomm::RfcommSocket;
use katasymbol_proto::speed::calc_speed;

// ---------------------------------------------------------------------------
// Opaque types (heap-allocated, passed as raw pointers to C)
// ---------------------------------------------------------------------------

/// Opaque device handle wrapping a connected Printer.
pub struct KsDevice {
    printer: Printer,
    /// Guards status queries during active raster transfer.
    printing: AtomicBool,
}

/// Opaque job handle accumulating raster scanlines.
pub struct KsJob {
    width: u32,
    height: u32,
    bytes_per_line: u32,
    raster_data: Vec<u8>,
    lines_received: u32,
    density: u8,
}

// ---------------------------------------------------------------------------
// C-compatible structs
// ---------------------------------------------------------------------------

/// Material/label info returned to C callers.
#[repr(C)]
pub struct KsMaterial {
    pub width_mm: u8,
    pub height_mm: u8,
    pub gap_mm: u8,
    pub label_type: u8,
    /// -1 if unknown
    pub remaining: i32,
}

/// Callback type for device discovery — matches what bt_list_cb trampoline expects.
pub type KsDiscoverCb =
    unsafe extern "C" fn(device_info: *const c_char, device_uri: *const c_char, device_id: *const c_char, cb_data: *mut c_void) -> bool;

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Initialize env_logger. Safe to call multiple times (subsequent calls are no-ops).
#[no_mangle]
pub extern "C" fn ks_init_logging() {
    let _ = env_logger::try_init();
}

// ---------------------------------------------------------------------------
// Device lifecycle
// ---------------------------------------------------------------------------

/// Open an RFCOMM connection to the printer at `addr` (e.g. "A4:93:40:A0:87:57").
///
/// Returns a heap-allocated KsDevice on success, or null on failure.
#[no_mangle]
pub unsafe extern "C" fn ks_device_open(addr: *const c_char) -> *mut KsDevice {
    if addr.is_null() {
        log::error!("ks_device_open: null address");
        return std::ptr::null_mut();
    }
    let addr_str = match CStr::from_ptr(addr).to_str() {
        Ok(s) => s,
        Err(e) => {
            log::error!("ks_device_open: invalid address string: {e}");
            return std::ptr::null_mut();
        }
    };

    log::info!("ks_device_open: connecting to {addr_str}");
    let sock = match RfcommSocket::connect_default(addr_str) {
        Ok(s) => s,
        Err(e) => {
            log::error!("ks_device_open: RFCOMM connect failed: {e}");
            return std::ptr::null_mut();
        }
    };

    let dev = Box::new(KsDevice {
        printer: Printer::new(sock),
        printing: AtomicBool::new(false),
    });
    Box::into_raw(dev)
}

/// Close the RFCOMM connection and free the KsDevice.
#[no_mangle]
pub unsafe extern "C" fn ks_device_close(dev: *mut KsDevice) {
    if !dev.is_null() {
        log::info!("ks_device_close");
        drop(Box::from_raw(dev));
    }
}

// ---------------------------------------------------------------------------
// Raw I/O (PAPPL device read/write callbacks)
// ---------------------------------------------------------------------------

/// Read raw bytes from the RFCOMM socket. Returns bytes read, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn ks_device_read(
    dev: *mut KsDevice,
    buf: *mut u8,
    len: usize,
) -> isize {
    if dev.is_null() || buf.is_null() {
        return -1;
    }
    let fd = (*dev).printer.raw_fd();
    libc::recv(fd, buf as *mut c_void, len, 0)
}

/// Write raw bytes to the RFCOMM socket. Returns bytes written, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn ks_device_write(
    dev: *mut KsDevice,
    buf: *const u8,
    len: usize,
) -> isize {
    if dev.is_null() || buf.is_null() {
        return -1;
    }
    let fd = (*dev).printer.raw_fd();
    libc::send(fd, buf as *const c_void, len, 0)
}

// ---------------------------------------------------------------------------
// Device status
// ---------------------------------------------------------------------------

// PAPPL pappl_preason_t bit flags (from pappl/printer.h).
const PAPPL_PREASON_NONE: u32 = 0x0000;
const PAPPL_PREASON_OTHER: u32 = 0x0001;
const PAPPL_PREASON_COVER_OPEN: u32 = 0x0002;
const PAPPL_PREASON_MEDIA_EMPTY: u32 = 0x0080;
const PAPPL_PREASON_MEDIA_JAM: u32 = 0x0100;
const PAPPL_PREASON_MEDIA_NEEDED: u32 = 0x0400;

/// Query printer status and return pappl_preason_t bit flags.
///
/// Returns PAPPL_PREASON_NONE while the printing flag is set (to avoid
/// disrupting active transfers with status commands).
#[no_mangle]
pub unsafe extern "C" fn ks_device_status(dev: *mut KsDevice) -> u32 {
    if dev.is_null() {
        return PAPPL_PREASON_OTHER;
    }
    let device = &*dev;

    // Don't send status queries during active transfer
    if device.printing.load(Ordering::Acquire) {
        return PAPPL_PREASON_NONE;
    }

    let status = match device.printer.query_status() {
        Ok(Some(s)) => s,
        Ok(None) => return PAPPL_PREASON_OTHER,
        Err(e) => {
            log::warn!("ks_device_status: query failed: {e}");
            return PAPPL_PREASON_OTHER;
        }
    };

    let mut reasons = PAPPL_PREASON_NONE;
    if status.cover_open {
        reasons |= PAPPL_PREASON_COVER_OPEN;
    }
    if status.label_end || status.label_not_installed {
        reasons |= PAPPL_PREASON_MEDIA_EMPTY;
    }
    if status.label_rw_error || status.label_mode_error {
        reasons |= PAPPL_PREASON_MEDIA_JAM;
    }
    if status.ribbon_end {
        reasons |= PAPPL_PREASON_MEDIA_NEEDED;
    }
    if status.head_temp_high {
        reasons |= PAPPL_PREASON_OTHER;
    }
    reasons
}

/// Query material info. Returns true on success with `out` filled in.
#[no_mangle]
pub unsafe extern "C" fn ks_device_material(dev: *mut KsDevice, out: *mut KsMaterial) -> bool {
    if dev.is_null() || out.is_null() {
        return false;
    }
    let device = &*dev;

    match device.printer.query_material() {
        Ok(Some(mat)) => {
            (*out).width_mm = mat.width_mm;
            (*out).height_mm = mat.height_mm;
            (*out).gap_mm = mat.gap_mm;
            (*out).label_type = mat.label_type;
            (*out).remaining = mat.remaining.map(|r| r as i32).unwrap_or(-1);
            true
        }
        Ok(None) => {
            log::warn!("ks_device_material: no response");
            false
        }
        Err(e) => {
            log::error!("ks_device_material: {e}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Device discovery (BlueZ D-Bus)
// ---------------------------------------------------------------------------

/// Discover paired Katasymbol printers via BlueZ D-Bus.
///
/// For each found device, calls `cb(device_info, device_uri, device_id, cb_data)`.
/// Returns true if enumeration succeeded (even if no devices found).
#[no_mangle]
pub unsafe extern "C" fn ks_discover(
    cb: KsDiscoverCb,
    cb_data: *mut c_void,
) -> bool {
    use dbus::blocking::Connection;
    use std::collections::HashMap;
    use std::time::Duration;

    type ManagedObjects = HashMap<
        dbus::Path<'static>,
        HashMap<String, HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>>,
    >;

    const SPP_UUID_PREFIX: &str = "00001101-";
    const BLUEZ_SERVICE: &str = "org.bluez";

    let conn = match Connection::new_system() {
        Ok(c) => c,
        Err(e) => {
            log::error!("ks_discover: D-Bus connection failed: {e}");
            return false;
        }
    };

    let proxy = conn.with_proxy(BLUEZ_SERVICE, "/", Duration::from_secs(5));

    use dbus::blocking::stdintf::org_freedesktop_dbus::ObjectManager;
    let objects: ManagedObjects = match proxy.get_managed_objects() {
        Ok(o) => o,
        Err(e) => {
            log::error!("ks_discover: GetManagedObjects failed: {e}");
            return false;
        }
    };

    for (path, interfaces) in &objects {
        let path_str = path.to_string();
        if !path_str.starts_with("/org/bluez/hci") || !path_str.contains("/dev_") {
            continue;
        }

        let props = match interfaces.get("org.bluez.Device1") {
            Some(p) => p,
            None => continue,
        };

        let address = match props.get("Address") {
            Some(v) => match v.0.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            },
            None => continue,
        };

        let name = props
            .get("Name")
            .and_then(|v| v.0.as_str().map(String::from))
            .unwrap_or_default();

        // Check for SPP UUID
        let has_spp = props
            .get("UUIDs")
            .map(|v| {
                if let Some(iter) = v.0.as_iter() {
                    for item in iter {
                        if let Some(uuid) = item.as_str() {
                            if uuid.to_lowercase().starts_with(SPP_UUID_PREFIX) {
                                return true;
                            }
                        }
                    }
                }
                false
            })
            .unwrap_or(false);

        if !has_spp {
            continue;
        }

        // Filter by name pattern
        let name_lower = name.to_lowercase();
        if !(name_lower.contains("t50")
            || name_lower.contains("t0117")
            || name_lower.contains("supvan")
            || name_lower.contains("katasymbol"))
        {
            continue;
        }

        log::info!("ks_discover: found {} ({})", name, address);

        let device_info =
            CString::new(format!("Katasymbol M50 Pro ({name})")).unwrap_or_default();
        let device_uri =
            CString::new(format!("btrfcomm://{address}")).unwrap_or_default();
        let device_id =
            CString::new("MFG:Katasymbol;MDL:M50 Pro;CMD:KATASYMBOL;").unwrap_or_default();

        let cont = cb(
            device_info.as_ptr(),
            device_uri.as_ptr(),
            device_id.as_ptr(),
            cb_data,
        );
        if !cont {
            break;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Job lifecycle
// ---------------------------------------------------------------------------

/// Start a print job: runs CHECK_DEVICE → wait_ready → START_PRINT → wait_printing.
///
/// Returns a heap-allocated KsJob on success, or null on failure.
/// `w`, `h`: raster dimensions in pixels. `bpl`: bytes per line. `density`: 0-15.
#[no_mangle]
pub unsafe extern "C" fn ks_job_start(
    dev: *mut KsDevice,
    w: u32,
    h: u32,
    bpl: u32,
    density: u8,
) -> *mut KsJob {
    if dev.is_null() {
        return std::ptr::null_mut();
    }
    let device = &*dev;

    log::info!("ks_job_start: {w}x{h}, bpl={bpl}, density={density}");

    // CHECK_DEVICE
    match device.printer.check_device() {
        Ok(true) => {}
        Ok(false) => {
            log::error!("ks_job_start: CHECK_DEVICE failed");
            return std::ptr::null_mut();
        }
        Err(e) => {
            log::error!("ks_job_start: CHECK_DEVICE error: {e}");
            return std::ptr::null_mut();
        }
    }

    // Wait ready
    match device.printer.wait_ready(60) {
        Ok(Some(s)) => {
            if s.has_error() {
                log::error!(
                    "ks_job_start: printer error: {}",
                    s.error_description().unwrap_or_default()
                );
                return std::ptr::null_mut();
            }
        }
        Ok(None) => {
            log::error!("ks_job_start: timeout waiting for device ready");
            return std::ptr::null_mut();
        }
        Err(e) => {
            log::error!("ks_job_start: wait_ready error: {e}");
            return std::ptr::null_mut();
        }
    }

    // START_PRINT
    if let Err(e) = device.printer.start_print() {
        log::error!("ks_job_start: START_PRINT error: {e}");
        return std::ptr::null_mut();
    }

    // Wait printing
    match device.printer.wait_printing(60) {
        Ok(Some(_)) => {}
        Ok(None) => {
            log::error!("ks_job_start: timeout waiting for printing station");
            return std::ptr::null_mut();
        }
        Err(e) => {
            log::error!("ks_job_start: wait_printing error: {e}");
            return std::ptr::null_mut();
        }
    }

    // Set printing flag
    device.printing.store(true, Ordering::Release);

    let job = Box::new(KsJob {
        width: w,
        height: h,
        bytes_per_line: bpl,
        raster_data: vec![0u8; (h * bpl) as usize],
        lines_received: 0,
        density,
    });
    Box::into_raw(job)
}

/// Append a single raster scanline at position `y`.
///
/// `line` points to `len` bytes of 1bpp MSB-first raster data.
#[no_mangle]
pub unsafe extern "C" fn ks_job_write_line(
    job: *mut KsJob,
    y: u32,
    line: *const u8,
    len: u32,
) -> bool {
    if job.is_null() || line.is_null() {
        return false;
    }
    let j = &mut *job;
    if y >= j.height {
        return false;
    }

    let copy_len = len.min(j.bytes_per_line) as usize;
    let offset = (y * j.bytes_per_line) as usize;
    let src = std::slice::from_raw_parts(line, copy_len);
    j.raster_data[offset..offset + copy_len].copy_from_slice(src);
    j.lines_received += 1;
    true
}

/// Process and transfer a completed page.
///
/// Performs: raster_to_column_major → center_in_printhead → split_into_buffers →
/// compress_buffers → calc_speed → wait_buffer_ready → transfer_compressed.
#[no_mangle]
pub unsafe extern "C" fn ks_job_end_page(job: *mut KsJob, dev: *mut KsDevice) -> bool {
    if job.is_null() || dev.is_null() {
        return false;
    }
    let j = &mut *job;
    let device = &*dev;

    log::info!(
        "ks_job_end_page: {}x{}, {} lines received",
        j.width,
        j.height,
        j.lines_received,
    );

    // 1. Rotate to column-major LSB-first
    let (col_data, num_cols, _col_bpl) =
        raster_to_column_major(&j.raster_data, j.width, j.height);

    // 2. Center in printhead canvas (384 dots = 48mm)
    let canvas_width_dots = PRINTHEAD_WIDTH_MM * 8; // 384
    let (canvas, canvas_bpl) =
        center_in_printhead(&col_data, num_cols, j.width, canvas_width_dots);

    // 3. Split into print buffers
    let buffers = split_into_buffers(
        &canvas,
        canvas_bpl as u8,
        num_cols as u16,
        8,  // margin_top
        8,  // margin_bottom
        j.density,
    );
    log::info!("ks_job_end_page: {} print buffers", buffers.len());

    // 4. Compress
    let (compressed, avg) = match compress_buffers(&buffers) {
        Ok(r) => r,
        Err(e) => {
            log::error!("ks_job_end_page: compression failed: {e}");
            return false;
        }
    };
    let speed = calc_speed(avg);
    log::info!(
        "ks_job_end_page: compressed {} bytes, avg={}, speed={}",
        compressed.len(),
        avg,
        speed,
    );

    // 5. Wait for buffer space
    match device.printer.wait_buffer_ready(200) {
        Ok(Some(s)) => {
            if s.has_error() {
                log::error!(
                    "ks_job_end_page: printer error: {}",
                    s.error_description().unwrap_or_default()
                );
                return false;
            }
        }
        Ok(None) => {
            log::error!("ks_job_end_page: timeout waiting for buffer space");
            return false;
        }
        Err(e) => {
            log::error!("ks_job_end_page: wait_buffer_ready error: {e}");
            return false;
        }
    }

    // 6. Transfer
    if let Err(e) = device.printer.transfer_compressed(&compressed, speed) {
        log::error!("ks_job_end_page: transfer failed: {e}");
        return false;
    }

    // Clear raster buffer for next page
    j.raster_data.fill(0);
    j.lines_received = 0;

    true
}

/// End a print job: wait for completion, free KsJob.
#[no_mangle]
pub unsafe extern "C" fn ks_job_end(job: *mut KsJob, dev: *mut KsDevice) {
    if dev.is_null() {
        if !job.is_null() {
            drop(Box::from_raw(job));
        }
        return;
    }
    let device = &*dev;

    log::info!("ks_job_end: waiting for completion");

    // Poll until not printing and not busy
    for _ in 0..300 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        match device.printer.query_status() {
            Ok(Some(s)) => {
                if !s.printing && !s.device_busy {
                    log::info!("ks_job_end: print complete");
                    break;
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("ks_job_end: status query error: {e}");
                break;
            }
        }
    }

    // Clear printing flag
    device.printing.store(false, Ordering::Release);

    if !job.is_null() {
        drop(Box::from_raw(job));
    }
}
