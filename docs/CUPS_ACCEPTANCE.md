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

The matching `UUID=` makes a co-resident `cups-browsed` dedupe our advert and
stand down instead of building a broken `implicitclass://` queue — no config
change or disabling needed. See **[DEPLOY.md](DEPLOY.md#cups-browsed-coexistence)**
for the full mechanism and the one-time cache-rebuild caveat.

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
# Any file CUPS can rasterize (text/PDF/image), or a ready .pwg:
lp -d "$PRINTER" /path/to/file
```

Check server logs for `KsJob::transfer_page` and job completion. To print a
test pattern straight to the hardware (bypassing CUPS), use
`supvan-cli test-print <target>`.

`lpstat -W all -o "$PRINTER"` lists jobs (each gets a distinct job-id). Cancel
in-flight jobs via `cancel <jobid>`.

## 4. Compare attributes (optional)

```sh
./scripts/capture_ipp_golden.sh "$PRINTER"
ipptool -tv ipp://localhost:8631/ipp/print/"$PRINTER" \
    /usr/share/cups/ipptool/get-printer-attributes.test
```

The full set of attributes required for `-m everywhere` (and the IPP Everywhere
`ipptool` audit) is documented in **[CONFORMANCE.md](CONFORMANCE.md)** — the
server passes the suite 32/0.

## 5. Cleanup

The auto-created queue is **persistent** — it's deliberately kept across
restarts so its `printer-uuid` stays stable (see
[DEPLOY.md](DEPLOY.md#cups-browsed-coexistence)). It's removed on uninstall
(`make uninstall` / `make uninstall-user`), or by hand:

```sh
sudo lpadmin -x "$PRINTER"
```
