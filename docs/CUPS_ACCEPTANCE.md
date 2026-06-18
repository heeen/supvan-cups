# CUPS acceptance (Rust IPP server)

Prerequisites: `cups`, `cups-client`, built `supvan-printer-app`, printer visible on USB or BT.

## 1. Start the server

```sh
cargo build --release
SUPVAN_MODELS=data/models.toml ./target/release/supvan-printer-app
```

Open http://localhost:8631/ and note the printer name and `ipp://…` URI.

## 2. Register a driverless queue

**The app self-registers — normally there's nothing to do here.** On startup
the in-process registrar (`crates/supvan-app/src/registrar.rs`) waits for the
IPP server to bind, then for each discovered printer:

1. creates a direct queue `lpadmin -p <name> -E -v
   ipp://localhost:8631/ipp/print/<name> -m everywhere`,
2. reads back the CUPS-assigned `printer-uuid`,
3. advertises mDNS (`_ipp._tcp.local.`) with `UUID=<that uuid>`.

Verify:

```sh
lpstat -v            # one queue, device-uri ipp://localhost:8631/...
avahi-browse -rpt _ipp._tcp | grep UUID=   # advert carries the queue uuid
```

### Coexistence with cups-browsed (automatic)

Because the advertised `UUID=` matches the local queue's `printer-uuid`,
a co-resident `cups-browsed` dedupes the service and stands down (its debug
log shows *"is from local CUPS, ignored"*) rather than building a broken
`implicitclass://` queue that can't route to a same-host service. No
`cups-browsed` config change or disabling is needed — it works whether
`cups-browsed` is enabled or not. If `cups-browsed` was already running and
races in a duplicate before its local-queue cache updates, the registrar
sweeps it within ~20 s.

### Manual `lpadmin` (fallback / other hosts)

```sh
PRINTER=supvan-YOURID   # from the web index
sudo lpadmin -p "$PRINTER" -E \
  -v "ipp://localhost:8631/ipp/print/$PRINTER" \
  -m everywhere
```

Useful when adding the printer from a *different* machine on the LAN (the
mDNS advert makes it discoverable there too), or to debug attribute errors
directly. Disable mDNS at compile time with `--no-default-features` on
`ipp-printer-app` if you don't want any advertising.

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
