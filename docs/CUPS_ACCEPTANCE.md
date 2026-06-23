# CUPS acceptance (Rust IPP server)

Prerequisites: `cups`, `cups-client`, built `supvan-printer-app`, printer visible on USB or BT.

## 1. Start the server

```sh
cargo build --release
SUPVAN_MODELS=data/models.toml ./target/release/supvan-printer-app
```

Open http://localhost:8631/ and note the printer name and `ipp://…` URI.

## 2. Discover the printer (CUPS makes the queue)

**There's nothing to register.** The app is a self-contained IPP Everywhere
printer: it advertises itself over DNS-SD (`_ipp._tcp.local.`) and CUPS creates
a **temporary on-demand queue** when you print to it (auto-removed when idle) —
the AirPrint path. We do not create or own a CUPS queue.

Verify discovery:

```sh
avahi-browse -rt _ipp._tcp | grep -i supvan    # advertised
lpstat -e | grep -i supvan                     # cupsd has discovered it
```

`<name>` below is the logical printer name from the web index
(`http://localhost:8631/`), e.g. `supvan_t50_series_<serial>`.

### Coexistence with cups-browsed

Set `OnlyUnsupportedByCUPS Yes` in `/etc/cups/cups-browsed.conf` so a co-resident
`cups-browsed` defers driverless printers (us) to `cupsd` instead of building a
duplicate `implicitclass://` queue. See
**[DEPLOY.md](DEPLOY.md#cups-browsed-coexistence)** for the rationale.

### Pin a permanent queue (optional / other hosts)

```sh
lpadmin -p supvan -E -v "ipp://localhost:8631/ipp/print/<name>" -m everywhere
```

Useful to add the printer permanently, from a *different* machine on the LAN
(the mDNS advert makes it discoverable there too), or to debug attribute errors
directly. Disable mDNS at compile time with `--no-default-features` on
`ipp-printer-app` if you don't want any advertising.

## 3. Print

```sh
# lp to the discovered printer — CUPS spins up the temporary queue on demand.
# Any file CUPS can rasterize (text/PDF/image), or a ready .pwg:
lp -d "<name>" /path/to/file
```

Check server logs for `KsJob::transfer_page` and job completion. To print a
test pattern straight to the hardware (bypassing CUPS), use
`supvan-cli test-print <target>`.

`lpstat -W all -o "<name>"` lists jobs (each gets a distinct job-id). Cancel
in-flight jobs via `cancel <jobid>`. If the printer is powered off the job is
**held** (not dropped) and prints when it comes back.

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

There's nothing to clean up: we don't own a queue. The temporary queue CUPS
creates on demand auto-expires when idle. If you *pinned* a permanent queue
(section 2), remove it by hand:

```sh
lpadmin -x supvan
```
