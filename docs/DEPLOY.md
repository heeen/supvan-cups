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

`make install` honours `DESTDIR`/`PREFIX` (default `/usr`) for packaging. To run
the user unit and the system unit are mutually exclusive — a user unit in
`~/.config/systemd/user` shadows the `/usr/lib/systemd/user` one, so pick one.
For a true root-managed service (no login session needed), add an
`/etc/systemd/system/supvan-printer-app.service` with the same `ExecStart` and
`systemctl enable --now` it — but note discovery uses your session's BlueZ/Avahi.

## cups-browsed coexistence

The in-process registrar creates a direct `ipp://localhost:8631/...` queue and
advertises it over mDNS with `UUID=<queue printer-uuid>`, which cups-browsed
uses to dedupe (it then ignores our service rather than building a broken
`implicitclass://` queue). If cups-browsed was already running before the
service first created its queue, its local-queue cache is stale and it may
recreate `implicitclass://` duplicates. Rebuild that cache once:

```sh
sudo systemctl restart cups-browsed
```

After that the duplicates stay gone (the UUID dedup holds across restarts since
CUPS persists the queue uuid).
