/*
 * Katasymbol M50 Pro - PAPPL Printer Application
 *
 * Implements an IPP Everywhere printer application for the Katasymbol M50 Pro
 * thermal label printer using PAPPL. Communicates with the printer over
 * Bluetooth RFCOMM via the Rust katasymbol-ffi library.
 */

#include <pappl/pappl.h>
#include <string.h>
#include <stdio.h>

/* ---------------------------------------------------------------------------
 * FFI declarations (from libkatasymbol_ffi.a)
 * ---------------------------------------------------------------------------*/

typedef struct KsDevice KsDevice;
typedef struct KsJob    KsJob;

typedef struct {
    uint8_t  width_mm;
    uint8_t  height_mm;
    uint8_t  gap_mm;
    uint8_t  label_type;
    int32_t  remaining;
} KsMaterial;

typedef bool (*KsDiscoverCb)(const char *device_info,
                              const char *device_uri,
                              const char *device_id,
                              void *cb_data);

extern void      ks_init_logging(void);
extern KsDevice *ks_device_open(const char *addr);
extern void      ks_device_close(KsDevice *dev);
extern ssize_t   ks_device_read(KsDevice *dev, void *buf, size_t len);
extern ssize_t   ks_device_write(KsDevice *dev, const void *buf, size_t len);
extern uint32_t  ks_device_status(KsDevice *dev);
extern bool      ks_device_material(KsDevice *dev, KsMaterial *out);
extern bool      ks_discover(KsDiscoverCb cb, void *cb_data);
extern KsJob    *ks_job_start(KsDevice *dev, uint32_t w, uint32_t h,
                               uint32_t bpl, uint8_t density);
extern bool      ks_job_write_line(KsJob *job, uint32_t y, const uint8_t *line,
                                    uint32_t len);
extern bool      ks_job_end_page(KsJob *job, KsDevice *dev);
extern void      ks_job_end(KsJob *job, KsDevice *dev);

/* ---------------------------------------------------------------------------
 * Forward declarations
 * ---------------------------------------------------------------------------*/

static int  get_darkness(pappl_job_t *job);

/* ---------------------------------------------------------------------------
 * Driver name and media table
 * ---------------------------------------------------------------------------*/

static const char *ks_driver_name = "katasymbol_m50pro";

/* PWG self-describing media size names */
static const char *ks_media_names[] = {
    "oe_40x30mm_40x30mm",
    "oe_40x40mm_40x40mm",
    "oe_40x50mm_40x50mm",
    "oe_40x60mm_40x60mm",
    "oe_40x70mm_40x70mm",
    "oe_40x80mm_40x80mm",
    "oe_30x15mm_30x15mm",
    "oe_30x20mm_30x20mm",
    "oe_30x30mm_30x30mm",
    "oe_30x40mm_30x40mm",
    "oe_48x30mm_48x30mm",
    "oe_48x45mm_48x45mm",
    "oe_48x70mm_48x70mm",
    "oe_25x25mm_25x25mm",
    "oe_50x30mm_50x30mm",
};

/* Dimensions in hundredths of mm (width, length) */
static const int ks_media_sizes[][2] = {
    { 4000, 3000 },
    { 4000, 4000 },
    { 4000, 5000 },
    { 4000, 6000 },
    { 4000, 7000 },
    { 4000, 8000 },
    { 3000, 1500 },
    { 3000, 2000 },
    { 3000, 3000 },
    { 3000, 4000 },
    { 4800, 3000 },
    { 4800, 4500 },
    { 4800, 7000 },
    { 2500, 2500 },
    { 5000, 3000 },
};

#define KS_NUM_MEDIA (int)(sizeof(ks_media_names) / sizeof(ks_media_names[0]))

/* ---------------------------------------------------------------------------
 * Discovery trampoline
 * ---------------------------------------------------------------------------*/

typedef struct {
    pappl_device_cb_t  pappl_cb;
    void              *pappl_data;
} discover_ctx_t;

static bool
discover_trampoline(const char *device_info, const char *device_uri,
                    const char *device_id, void *cb_data)
{
    discover_ctx_t *ctx = (discover_ctx_t *)cb_data;
    return ctx->pappl_cb(device_info, device_uri, device_id,
                         ctx->pappl_data);
}

/* ---------------------------------------------------------------------------
 * PAPPL device callbacks
 *
 * papplDeviceAddScheme expects:
 *   list_cb:   bool (*)(pappl_device_cb_t cb, void *data,
 *                        pappl_deverror_cb_t err_cb, void *err_data)
 *   open_cb:   bool (*)(pappl_device_t *device, const char *uri, const char *name)
 *   close_cb:  void (*)(pappl_device_t *device)
 *   read_cb:   ssize_t (*)(pappl_device_t *device, void *buf, size_t bytes)
 *   write_cb:  ssize_t (*)(pappl_device_t *device, const void *buf, size_t bytes)
 *   status_cb: pappl_preason_t (*)(pappl_device_t *device)
 *   id_cb:     char *(*)(pappl_device_t *device, char *buffer, size_t bufsize)
 * ---------------------------------------------------------------------------*/

static bool
bt_list_cb(pappl_device_cb_t cb, void *data,
           pappl_deverror_cb_t err_cb, void *err_data)
{
    (void)err_cb;
    (void)err_data;

    discover_ctx_t ctx = {
        .pappl_cb   = cb,
        .pappl_data = data,
    };
    return ks_discover(discover_trampoline, &ctx);
}

static bool
bt_open_cb(pappl_device_t *device, const char *device_uri,
           const char *name)
{
    (void)name;

    /* Extract address from btrfcomm://XX:XX:XX:XX:XX:XX */
    if (strncmp(device_uri, "btrfcomm://", 11) != 0)
        return false;

    const char *addr = device_uri + 11;
    KsDevice *dev = ks_device_open(addr);
    if (!dev)
        return false;

    papplDeviceSetData(device, dev);
    return true;
}

static void
bt_close_cb(pappl_device_t *device)
{
    KsDevice *dev = (KsDevice *)papplDeviceGetData(device);
    if (dev)
        ks_device_close(dev);
}

static ssize_t
bt_read_cb(pappl_device_t *device, void *buffer, size_t bytes)
{
    KsDevice *dev = (KsDevice *)papplDeviceGetData(device);
    if (!dev)
        return -1;
    return ks_device_read(dev, buffer, bytes);
}

static ssize_t
bt_write_cb(pappl_device_t *device, const void *buffer, size_t bytes)
{
    KsDevice *dev = (KsDevice *)papplDeviceGetData(device);
    if (!dev)
        return -1;
    return ks_device_write(dev, buffer, bytes);
}

static pappl_preason_t
bt_status_cb(pappl_device_t *device)
{
    KsDevice *dev = (KsDevice *)papplDeviceGetData(device);
    if (!dev)
        return PAPPL_PREASON_OTHER;
    return (pappl_preason_t)ks_device_status(dev);
}

/* ---------------------------------------------------------------------------
 * Raster callbacks
 * ---------------------------------------------------------------------------*/

static bool
ks_rstartjob_cb(pappl_job_t *job, pappl_pr_options_t *options,
                pappl_device_t *device)
{
    KsDevice *dev = (KsDevice *)papplDeviceGetData(device);
    if (!dev)
        return false;

    unsigned w   = options->header.cupsWidth;
    unsigned h   = options->header.cupsHeight;
    unsigned bpl = options->header.cupsBytesPerLine;

    /* Map darkness (0-100%) to density (0-15) */
    int darkness = get_darkness(job);
    uint8_t density = (uint8_t)((darkness * 15 + 50) / 100);

    KsJob *ks_job = ks_job_start(dev, w, h, bpl, density);
    if (!ks_job)
        return false;

    papplJobSetData(job, ks_job);
    return true;
}

static bool
ks_rstartpage_cb(pappl_job_t *job, pappl_pr_options_t *options,
                 pappl_device_t *device, unsigned page)
{
    /* No-op: buffer is cleared at end of previous page in ks_job_end_page */
    (void)job;
    (void)options;
    (void)device;
    (void)page;
    return true;
}

static bool
ks_rwriteline_cb(pappl_job_t *job, pappl_pr_options_t *options,
                 pappl_device_t *device, unsigned y,
                 const unsigned char *line)
{
    (void)device;

    KsJob *ks_job = (KsJob *)papplJobGetData(job);
    if (!ks_job)
        return false;

    return ks_job_write_line(ks_job, y, line, options->header.cupsBytesPerLine);
}

static bool
ks_rendpage_cb(pappl_job_t *job, pappl_pr_options_t *options,
               pappl_device_t *device, unsigned page)
{
    (void)options;
    (void)page;

    KsJob *ks_job = (KsJob *)papplJobGetData(job);
    KsDevice *dev = (KsDevice *)papplDeviceGetData(device);
    if (!ks_job || !dev)
        return false;

    return ks_job_end_page(ks_job, dev);
}

static bool
ks_rendjob_cb(pappl_job_t *job, pappl_pr_options_t *options,
              pappl_device_t *device)
{
    (void)options;

    KsJob *ks_job = (KsJob *)papplJobGetData(job);
    KsDevice *dev = (KsDevice *)papplDeviceGetData(device);

    ks_job_end(ks_job, dev);
    papplJobSetData(job, NULL);
    return true;
}

/* ---------------------------------------------------------------------------
 * Helper: get printer darkness setting
 * ---------------------------------------------------------------------------*/

static int
get_darkness(pappl_job_t *job)
{
    pappl_printer_t *printer = papplJobGetPrinter(job);
    if (!printer)
        return 50;

    pappl_pr_driver_data_t data;
    if (!papplPrinterGetDriverData(printer, &data))
        return 50;

    return data.darkness_configured;
}

/* ---------------------------------------------------------------------------
 * Driver callback
 * ---------------------------------------------------------------------------*/

static bool
ks_driver_cb(pappl_system_t *system, const char *driver_name,
             const char *device_uri, const char *device_id,
             pappl_pr_driver_data_t *data, ipp_t **attrs,
             void *cbdata)
{
    (void)system;
    (void)device_uri;
    (void)device_id;
    (void)attrs;
    (void)cbdata;

    if (strcmp(driver_name, ks_driver_name) != 0)
        return false;

    /* Basic printer info */
    strlcpy(data->make_and_model, "Katasymbol M50 Pro",
            sizeof(data->make_and_model));

    /* Format: PWG raster */
    data->format        = "application/vnd.cups-raster";
    data->orient_default = IPP_ORIENT_PORTRAIT;
    data->quality_default = IPP_QUALITY_NORMAL;

    /* Resolution: 203x203 DPI (8 dots/mm) */
    data->num_resolution    = 1;
    data->x_resolution[0]  = 203;
    data->y_resolution[0]  = 203;
    data->x_default         = 203;
    data->y_default         = 203;

    /* Raster type: 1bpp monochrome */
    data->raster_types    = PAPPL_PWG_RASTER_TYPE_BLACK_1;
    data->color_supported = PAPPL_COLOR_MODE_MONOCHROME;
    data->color_default   = PAPPL_COLOR_MODE_MONOCHROME;

    /* One-sided only */
    data->sides_supported = PAPPL_SIDES_ONE_SIDED;
    data->sides_default   = PAPPL_SIDES_ONE_SIDED;

    /* Label printer: borderless, no margins */
    data->borderless = true;
    data->left_right = 0;
    data->bottom_top = 0;
    data->kind       = PAPPL_KIND_LABEL;

    /* Darkness: 0-100% mapped to density 0-15 in rstartjob */
    data->darkness_configured = 50;
    data->darkness_supported  = 16;  /* 16 levels */

    /* Label mode */
    data->mode_configured = PAPPL_LABEL_MODE_TEAR_OFF;
    data->mode_supported  = PAPPL_LABEL_MODE_TEAR_OFF;

    /* Speed: not applicable */
    data->speed_default      = 0;
    data->speed_supported[0] = 0;
    data->speed_supported[1] = 0;

    /* Media sizes */
    data->num_media = KS_NUM_MEDIA;
    for (int i = 0; i < KS_NUM_MEDIA; i++)
        data->media[i] = ks_media_names[i];

    /* Default media: 40x30mm */
    strlcpy(data->media_default.size_name, ks_media_names[0],
            sizeof(data->media_default.size_name));
    data->media_default.size_width    = ks_media_sizes[0][0];
    data->media_default.size_length   = ks_media_sizes[0][1];
    data->media_default.left_margin   = 0;
    data->media_default.right_margin  = 0;
    data->media_default.top_margin    = 0;
    data->media_default.bottom_margin = 0;
    strlcpy(data->media_default.source, "main-roll",
            sizeof(data->media_default.source));
    strlcpy(data->media_default.type, "labels",
            sizeof(data->media_default.type));

    /* Media sources/types */
    data->num_source = 1;
    data->source[0]  = "main-roll";
    data->num_type   = 1;
    data->type[0]    = "labels";

    /* Media-ready: same as default */
    data->media_ready[0] = data->media_default;

    /* Raster callbacks */
    data->rstartjob_cb  = ks_rstartjob_cb;
    data->rstartpage_cb = ks_rstartpage_cb;
    data->rwriteline_cb = ks_rwriteline_cb;
    data->rendpage_cb   = ks_rendpage_cb;
    data->rendjob_cb    = ks_rendjob_cb;

    return true;
}

/* ---------------------------------------------------------------------------
 * Auto-add callback
 * ---------------------------------------------------------------------------*/

static const char *
ks_autoadd_cb(const char *device_info, const char *device_uri,
              const char *device_id, void *data)
{
    (void)device_info;
    (void)device_id;
    (void)data;

    if (device_uri && strncmp(device_uri, "btrfcomm://", 11) == 0)
        return ks_driver_name;
    return NULL;
}

/* ---------------------------------------------------------------------------
 * System callback
 * ---------------------------------------------------------------------------*/

static pappl_system_t *
ks_system_cb(int num_options, cups_option_t *options, void *data)
{
    (void)num_options;
    (void)options;
    (void)data;

    pappl_system_t *system = papplSystemCreate(
        PAPPL_SOPTIONS_MULTI_QUEUE | PAPPL_SOPTIONS_WEB_INTERFACE,
        "katasymbol-printer-app",
        /*port=*/0,
        /*subtypes=*/NULL,
        /*spooldir=*/NULL,
        /*logfile=*/NULL,
        PAPPL_LOGLEVEL_INFO,
        /*auth_service=*/NULL,
        /*tls_only=*/false);

    if (!system)
        return NULL;

    papplSystemSetFooterHTML(system,
        "Katasymbol M50 Pro Printer Application");

    pappl_version_t version = {
        .name     = "katasymbol-printer-app",
        .sversion = "1.0.0",
        .version  = { 1, 0, 0, 0 },
    };
    papplSystemSetVersions(system, 1, &version);

    /* Register btrfcomm:// device scheme */
    papplDeviceAddScheme("btrfcomm", PAPPL_DEVTYPE_CUSTOM_LOCAL,
        bt_list_cb, bt_open_cb, bt_close_cb,
        bt_read_cb, bt_write_cb, bt_status_cb,
        /*id_cb=*/NULL);

    /* Register printer driver */
    pappl_pr_driver_t driver = {
        .name        = ks_driver_name,
        .description = "Katasymbol M50 Pro",
        .device_id   = "MFG:Katasymbol;MDL:M50 Pro;CMD:KATASYMBOL;",
        .extension   = NULL,
    };
    papplSystemSetPrinterDrivers(system, 1, &driver,
        ks_autoadd_cb, /*create_cb=*/NULL, ks_driver_cb, NULL);

    return system;
}

/* ---------------------------------------------------------------------------
 * main
 *
 * papplMainloop signature (PAPPL 1.4):
 *   int papplMainloop(int argc, char *argv[],
 *       const char *version, const char *footer_html,
 *       int num_drivers, pappl_pr_driver_t *drivers,
 *       pappl_pr_autoadd_cb_t autoadd_cb,
 *       pappl_pr_driver_cb_t driver_cb,
 *       const char *subcmd_name, pappl_ml_subcmd_cb_t subcmd_cb,
 *       pappl_ml_system_cb_t system_cb,
 *       pappl_ml_usage_cb_t usage_cb,
 *       void *data)
 * ---------------------------------------------------------------------------*/

int
main(int argc, char *argv[])
{
    ks_init_logging();

    pappl_pr_driver_t driver = {
        .name        = ks_driver_name,
        .description = "Katasymbol M50 Pro",
        .device_id   = "MFG:Katasymbol;MDL:M50 Pro;CMD:KATASYMBOL;",
        .extension   = NULL,
    };

    return papplMainloop(argc, argv,
        /*version=*/"1.0.0",
        /*footer_html=*/"Katasymbol M50 Pro Printer Application",
        /*num_drivers=*/1,
        /*drivers=*/&driver,
        /*autoadd_cb=*/ks_autoadd_cb,
        /*driver_cb=*/ks_driver_cb,
        /*subcmd_name=*/NULL,
        /*subcmd_cb=*/NULL,
        /*system_cb=*/ks_system_cb,
        /*usage_cb=*/NULL,
        /*data=*/NULL);
}
