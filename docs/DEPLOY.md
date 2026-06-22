# Deploying supvan-printer-app

The app runs as a **user** systemd service. The binary needs no privileges: it
binds `0.0.0.0:8631` (unprivileged), drives `lpadmin` as your user, and embeds
`models.toml` (so it's self-contained). Prefer the user-scoped install; a
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

Verify:

```sh
systemctl --user is-active supvan-printer-app
lpstat -v | grep supvan                 # direct ipp://localhost:8631 queue(s)
ipptool -tv ipp://localhost:8631/ipp/print/<queue> \
    /usr/share/cups/ipptool/get-printer-attributes.test | grep document-format
```

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

## cups-browsed coexistence

The in-process registrar creates a direct `ipp://localhost:8631/...` queue and
advertises it over mDNS with `UUID=<queue printer-uuid>`. A co-resident
`cups-browsed` matches that `UUID=` against the local queue's `printer-uuid`,
recognises it as a local queue, and stands down — so it never builds a
duplicate (a broken `implicitclass://` cluster or a numbered `name_N` copy).

The catch is that the dedup needs a **stable** `printer-uuid`. CUPS assigns a
fresh random uuid to every newly-*created* queue, so the queue must **persist**
across restarts: we never delete it on stop, and on start the registrar
reconciles it in place (`lpadmin -p`, which preserves the uuid). The queue is
removed only on uninstall (`make uninstall` / `make uninstall-user`). A genuine
name change (model re-detection) is handled by the registrar's startup sweep,
which removes the old-named orphan.

No `cups-browsed` config change is needed. If `cups-browsed` had already
accumulated duplicates from before this fix (a stale cache), clear them once —
either restart the service (the startup sweep removes leftover dupes) or
`sudo systemctl restart cups-browsed` to rebuild its cache. Going forward the
stable uuid keeps it deduped.

The advert is also restricted to **physical** interfaces (`ipp-printer-app`
≥ 0.6.1). Previously it published on every interface, so on a host with Docker
bridges `cups-browsed` resolved us over each `veth*`/`br-*` link; the racy /
duplicate address answers there made avahi hand it a *null* host name for some
resolves, which fails its `is_local_hostname()` check, bypasses the `UUID=`
dedup, and builds a spurious `implicitclass://` cluster. Skipping loopback,
link-local and container/VM virtual bridges (`veth`, `docker`, `br-`, `virbr`,
`vnet`, `vmnet`, `vboxnet`) removes those resolves at the source. What remains
is at most a `printer-is-temporary` on-demand queue CUPS itself spins up from
the advert — which auto-expires — not a `cups-browsed` duplicate.
