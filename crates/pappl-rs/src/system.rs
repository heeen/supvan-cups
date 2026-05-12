//! Safe wrapper around `pappl_system_t` and a builder for constructing
//! configured PAPPL systems without raw FFI calls.

use std::ffi::{c_int, CStr};
use std::marker::PhantomData;

use pappl_sys::*;

use crate::device::DeviceScheme;
use crate::flags::{DeviceType, LogLevel, SystemOptions};
use crate::{Error, Result};

/// Borrowed handle to a PAPPL system. Lifetime `'a` ties it to whatever
/// produced the pointer (typically a `papplMainloop`-managed system).
#[repr(transparent)]
pub struct System<'a> {
    raw: *mut pappl_system_t,
    _marker: PhantomData<&'a pappl_system_t>,
}

impl<'a> System<'a> {
    /// Wrap a raw PAPPL system pointer.
    ///
    /// # Safety
    /// `raw` must be a valid `*mut pappl_system_t` that lives at least
    /// as long as `'a`. Typically obtained from a PAPPL callback
    /// parameter.
    pub unsafe fn from_raw(raw: *mut pappl_system_t) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    /// Underlying raw pointer for callers still using `pappl-sys`
    /// directly during migration.
    pub fn as_raw(&self) -> *mut pappl_system_t {
        self.raw
    }

    /// Register a custom device scheme. The scheme's URI prefix
    /// (`S::NAME`), device type, and callback set are installed via
    /// `papplDeviceAddScheme`. Generic thunks in `device::install`
    /// bridge the trait methods to PAPPL's C ABI.
    pub fn register_scheme<S: DeviceScheme>(&self) {
        unsafe { crate::device::install::<S>(self.raw) };
    }

    /// Discover and auto-add printers across the registered device
    /// schemes of the given types. Wraps `papplSystemCreatePrinters`
    /// (PAPPL ≥ 1.4). On older PAPPL returns `Err(Error::Unsupported)`.
    pub fn auto_add_printers(&self, types: DeviceType) -> Result<()> {
        let ok = unsafe {
            pappl_sys::try_system_create_printers(
                self.raw,
                types.into(),
                None,
                std::ptr::null_mut(),
            )
        };
        if ok {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }
}

// ---------------------------------------------------------------------------
// SystemBuilder
// ---------------------------------------------------------------------------

/// Chainable builder that creates and configures a PAPPL system.
///
/// Collects constructor arguments and post-creation property setters,
/// then materializes the system in `build()`.
///
/// ```ignore
/// SystemBuilder::new(c"my-printer-app")
///     .port(8631)
///     .options(SystemOptions::MULTI_QUEUE | SystemOptions::WEB_INTERFACE)
///     .log_level(LogLevel::Debug)
///     .log_file(c"-")
///     .footer_html(c"My Printer App")
///     .version(b"my-printer-app", b"1.0.0", [1, 0, 0, 0])
///     .scheme::<BtScheme>()
///     .scheme::<UsbScheme>()
///     .build()
/// ```
pub struct SystemBuilder<'a> {
    // Constructor args for papplSystemCreate
    name: &'a CStr,
    port: c_int,
    options: SystemOptions,
    log_level: LogLevel,
    log_file: &'a CStr,
    auth_service: Option<&'a CStr>,
    tls_only: bool,

    // Post-creation setters
    footer_html: Option<&'a CStr>,
    version: Option<VersionInfo>,
    save_cb: Option<pappl_save_cb_t>,
    state_file: Option<&'a CStr>,
    listeners: bool,
    scheme_installers: Vec<fn(*mut pappl_system_t)>,
    drivers: Option<DriverRegistration>,
}

struct VersionInfo {
    name: Vec<u8>,
    sversion: Vec<u8>,
    version: [u16; 4],
}

struct DriverRegistration {
    drivers: Vec<pappl_pr_driver_t>,
    autoadd_cb: pappl_pr_autoadd_cb_t,
    driver_cb: pappl_pr_driver_cb_t,
}

impl<'a> SystemBuilder<'a> {
    /// Start building a new system with the given application name.
    pub fn new(name: &'a CStr) -> Self {
        Self {
            name,
            port: 0,
            options: SystemOptions::NONE,
            log_level: LogLevel::Info,
            log_file: c"-",
            auth_service: None,
            tls_only: false,
            footer_html: None,
            version: None,
            save_cb: None,
            state_file: None,
            listeners: false,
            scheme_installers: Vec::new(),
            drivers: None,
        }
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port as c_int;
        self
    }

    pub fn options(mut self, opts: SystemOptions) -> Self {
        self.options = opts;
        self
    }

    pub fn log_level(mut self, level: LogLevel) -> Self {
        self.log_level = level;
        self
    }

    pub fn log_file(mut self, file: &'a CStr) -> Self {
        self.log_file = file;
        self
    }

    pub fn auth_service(mut self, svc: &'a CStr) -> Self {
        self.auth_service = Some(svc);
        self
    }

    pub fn tls_only(mut self, tls: bool) -> Self {
        self.tls_only = tls;
        self
    }

    pub fn footer_html(mut self, html: &'a CStr) -> Self {
        self.footer_html = Some(html);
        self
    }

    /// Register application version info.
    pub fn version(mut self, name: &[u8], sversion: &[u8], ver: [u16; 4]) -> Self {
        self.version = Some(VersionInfo {
            name: name.to_vec(),
            sversion: sversion.to_vec(),
            version: ver,
        });
        self
    }

    /// Register a save callback for persisting system state.
    pub fn save_callback(mut self, cb: pappl_save_cb_t) -> Self {
        self.save_cb = Some(cb);
        self
    }

    /// Set the state file path for `papplSystemLoadState` / save.
    pub fn state_file(mut self, path: &'a CStr) -> Self {
        self.state_file = Some(path);
        self
    }

    /// Add default TCP listeners via `papplSystemAddListeners(NULL)`.
    pub fn default_listeners(mut self) -> Self {
        self.listeners = true;
        self
    }

    /// Register a custom device scheme. May be called multiple times.
    pub fn scheme<S: DeviceScheme>(mut self) -> Self {
        self.scheme_installers.push(|sys| {
            unsafe { crate::device::install::<S>(sys) };
        });
        self
    }

    /// Register printer drivers, auto-add and driver callbacks.
    ///
    /// The `drivers` Vec is leaked (PAPPL stores the pointer directly),
    /// so the memory lives until process exit.
    pub fn printer_drivers(
        mut self,
        drivers: Vec<pappl_pr_driver_t>,
        autoadd_cb: pappl_pr_autoadd_cb_t,
        driver_cb: pappl_pr_driver_cb_t,
    ) -> Self {
        self.drivers = Some(DriverRegistration {
            drivers,
            autoadd_cb,
            driver_cb,
        });
        self
    }

    /// Create the PAPPL system, apply all configuration, and return the
    /// wrapped handle. Returns `None` if `papplSystemCreate` fails.
    pub fn build(self) -> Option<System<'static>> {
        let auth = self
            .auth_service
            .map_or(std::ptr::null(), |c| c.as_ptr());

        let system = unsafe {
            papplSystemCreate(
                self.options.into(),
                self.name.as_ptr(),
                self.port,
                std::ptr::null(), // subtypes
                std::ptr::null(), // spooldir
                self.log_file.as_ptr(),
                self.log_level.into(),
                auth,
                self.tls_only,
            )
        };

        if system.is_null() {
            return None;
        }

        // TCP listeners
        if self.listeners {
            unsafe { papplSystemAddListeners(system, std::ptr::null()) };
        }

        // Footer
        if let Some(html) = self.footer_html {
            unsafe { papplSystemSetFooterHTML(system, html.as_ptr()) };
        }

        // Version
        if let Some(vi) = self.version {
            let mut ver: pappl_version_t = Default::default();
            crate::util::copy_to_c_buf(&mut ver.name, &vi.name);
            crate::util::copy_to_c_buf(&mut ver.sversion, &vi.sversion);
            ver.version = vi.version;
            unsafe { papplSystemSetVersions(system, 1, &mut ver) };
        }

        // Device schemes
        for install_fn in &self.scheme_installers {
            install_fn(system);
        }

        // Printer drivers
        if let Some(mut reg) = self.drivers {
            let num = reg.drivers.len() as i32;
            let ptr = reg.drivers.as_mut_ptr();
            std::mem::forget(reg.drivers);
            unsafe {
                papplSystemSetPrinterDrivers(
                    system,
                    num,
                    ptr,
                    reg.autoadd_cb,
                    None, // create_cb
                    reg.driver_cb,
                    std::ptr::null_mut(),
                );
            }
        }

        // Save callback
        if let Some(cb) = self.save_cb {
            unsafe { papplSystemSetSaveCallback(system, cb, std::ptr::null_mut()) };
        }

        // State file: load existing state
        if let Some(path) = self.state_file {
            unsafe { papplSystemLoadState(system, path.as_ptr()) };
        }

        Some(unsafe { System::from_raw(system) })
    }
}
