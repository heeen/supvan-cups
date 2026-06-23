# Deploying supvan-printer-app

The app runs as a **user** systemd service. The binary needs no privileges: it
binds `0.0.0.0:8631` (unprivileged) and embeds `models.toml` (so it's
self-contained). It is a **self-contained IPP Everywhere printer** — it
advertises itself over DNS-SD and lets CUPS create an on-demand queue; it does
not create or manage any CUPS queue itself. Prefer the user-scoped install; a
system option is below.

## User-scoped (default, no sudo)

```sh
make deploy
```

That's it: `cargo install`s the binary to `~/.cargo/bin`, installs a
self-contained user unit (`etc/supvan-printer-app.user.service`, `%h`-relative)
and the cleanup hook under `~/.local/lib`, `daemon-reload`s, and enables +
(re)starts the service. Re-run it after any code change. `make uninstall-user`
reverses it.

Verify (the app serves IPP directly; there is no standing CUPS queue — the
printer is discovered over DNS-SD and CUPS spins up a temporary queue on use):

```sh
systemctl --user is-active supvan-printer-app
avahi-browse -rt _ipp._tcp | grep -i supvan          # discoverable over DNS-SD
ipptool -tv ipp://localhost:8631/ipp/print/<name> \
    /usr/share/cups/ipptool/get-printer-attributes.test | grep document-format
```

The `<name>` is the logical printer name (lowercase, e.g. `supvan_t50_series_<serial>`);
the web index at `http://localhost:8631/` lists each printer's display name and
logical name.

## System option (sudo, FHS)

```sh
make build
sudo make install      # binary -> /usr/bin, unit -> /usr/lib/systemd/user, data, udev, dbus
make uninstall-user    # drop the user override so the /usr binary is used
systemctl --user daemon-reload && systemctl --user enable --now supvan-printer-app
```

`make install` honours `DESTDIR`/`PREFIX` (default `/usr`) for packaging. The
user unit and the system unit are mutually exclusive — a user unit in
`~/.config/systemd/user` shadows the `/usr/lib/systemd/user` one, so pick one.
For a true root-managed service (no login session needed), add an
`/etc/systemd/system/supvan-printer-app.service` with the same `ExecStart` and
`systemctl enable --now` it — but note discovery uses your session's BlueZ/Avahi.

## How printing works (CUPS-managed queue)

We're a self-contained IPP Everywhere service. When you print, CUPS discovers us
over DNS-SD and creates a **temporary on-demand queue** (`printer-is-temporary`,
auto-removed when idle) — exactly the AirPrint path. The printer is always
visible in print dialogs while the service runs; there's no standing queue to
manage. To pin a permanent queue by hand if you ever want one:

```sh
lpadmin -p supvan -E -v "ipp://localhost:8631/ipp/print/<name>" -m everywhere
```

## cups-browsed coexistence

`cups-browsed` is the legacy daemon that turns DNS-SD adverts into local CUPS
queues. Modern CUPS (`cupsd`) already does that for driverless IPP Everywhere
printers like us, so the two overlap — and for a *same-host* service
`cups-browsed`'s `implicitclass://` backend can't route, so its queue is broken.

The fix is one line in **`/etc/cups/cups-browsed.conf`**:

```conf
OnlyUnsupportedByCUPS Yes
```

This is the recommended, forward-facing setting: `cups-browsed` then only sets
up printers `cupsd` *can't* handle itself (legacy remote-CUPS broadcasts), and
**defers** driverless printers like us to `cupsd`'s temporary queue. It logs
`… is already supported by CUPS, skipping` and creates no duplicate. Nothing is
disabled — `cups-browsed` keeps doing its real job. (It's the direction CUPS is
heading; some distributions already default to this.)

Without it, the default `cups-browsed` ignores `cupsd`'s temporary queue
entirely (it skips `printer-is-temporary` destinations) and builds its own
broken `implicitclass://` duplicate. If you can't change `cups-browsed.conf`,
the alternative is to stop it (`systemctl disable --now cups-browsed`).

The advert is restricted to **physical** interfaces (`ipp-printer-app` ≥ 0.6.1):
on a host with Docker/VM bridges, advertising over every `veth*`/`br-*` link
made avahi hand `cups-browsed` a *null* host name for some resolves, multiplying
its chances to misfire. Loopback, link-local and container/VM virtual bridges
(`veth`, `docker`, `br-`, `virbr`, `vnet`, `vmnet`, `vboxnet`) are skipped.

## Offline behavior

The status poller tracks the device. When it's powered off / unplugged:

- `printer-state` goes **stopped** with `offline-report`, and the DNS-SD advert
  is **withdrawn** — the printer drops out of print dialogs.
- A job submitted while it's offline is **held** (`processing-stopped`) and
  retried — like a printer holding a job through a paper jam — then prints once
  the device is back. It is not dropped. The same applies to a media jam or an
  out-of-paper condition.
