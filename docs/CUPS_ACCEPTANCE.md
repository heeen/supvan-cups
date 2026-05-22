# CUPS acceptance (Rust IPP server)

Prerequisites: `cups`, `cups-client`, built `supvan-printer-app`, printer visible on USB or BT.

## 1. Start the server

```sh
cargo build --release
SUPVAN_MODELS=data/models.toml ./target/release/supvan-printer-app
```

Open http://localhost:8631/ and note the printer name and `ipp://…` URI.

## 2. Register a driverless queue

Two paths depending on whether `cups-browsed` is running:

### Auto-import via mDNS (preferred when cups-browsed is active)

Default builds advertise each printer over mDNS (`_ipp._tcp.local.`). With
`cups-browsed` running it picks the printer up within ~10 seconds and creates
a CUPS queue automatically — no `lpadmin` step needed. Verify with:

```sh
avahi-browse -rt _ipp._tcp     # or: dns-sd -B _ipp._tcp
systemctl status cups-browsed
lpstat -p                       # the supvan-… queue should appear
```

Disable mDNS at compile time with `--no-default-features` on `ipp-printer-app`
if you don't want this (e.g. when running multiple servers on one host).

### Manual `lpadmin` (always works)

```sh
PRINTER=supvan-YOURID   # from the web index
sudo lpadmin -p "$PRINTER" -E \
  -v "ipp://localhost:8631/ipp/print/$PRINTER" \
  -m everywhere
```

This is the recommended first-install step — it works without `cups-browsed`,
without mDNS, and surfaces any printer-attribute errors directly. Once the
queue exists CUPS persists it across reboots; you don't need to repeat this.

## 3. Print

```sh
# Small CUPS raster from supvan-cli test pattern, or any .pwg/.ras job:
supvan-cli test-print --mock   # generates local PBM; convert separately if needed
lp -d "$PRINTER" /path/to/job.ras
```

Check server logs for `KsJob::transfer_page` and job completion.

`lpstat -W all -o "$PRINTER"` lists jobs (each gets a distinct job-id). Cancel
in-flight jobs via `cancel <jobid>`.

## 4. Compare attributes (optional)

```sh
./scripts/capture_ipp_golden.sh "$PRINTER"
ipptool -tv ipp://localhost:8631/ipp/print/"$PRINTER" \
    /usr/share/cups/ipptool/get-printer-attributes.test
```

Required attributes for `-m everywhere` (server already emits these):

- `document-format-supported` includes `image/pwg-raster`
- `print-color-mode-supported` includes `monochrome`
- `copies-supported` range 1–999
- `media-supported` / `media-default` from `models.toml`
- `media-col-default` / `media-col-supported` (PWG dimensions)
- `printer-uri-supported` as `uri`, not `keyword`

## 5. Cleanup

```sh
sudo lpadmin -x "$PRINTER"
```
