# Deploying supvan-printer-app

The app runs as a **user** systemd service. The binary needs no privileges: it
binds `0.0.0.0:8631` (unprivileged), drives `lpadmin` as your user, and embeds
`models.toml` (so it's self-contained). Prefer the user-scoped install; a
system option is below.

## User-scoped (default, no sudo)

```sh
# 1. Build + install to ~/.cargo/bin (user-owned).
cargo install --path crates/supvan-app --force

# 2. Point the service at it via a user drop-in (already in the repo history;
#    create once). Removing this file reverts to the system binary.
mkdir -p ~/.config/systemd/user/supvan-printer-app.service.d
cat > ~/.config/systemd/user/supvan-printer-app.service.d/override.conf <<'EOF'
[Service]
ExecStart=
ExecStart=%h/.cargo/bin/supvan-printer-app server
EOF

# 3. Reload + (re)start.
systemctl --user daemon-reload
systemctl --user enable --now supvan-printer-app
```

Redeploy after code changes = just `cargo install --path crates/supvan-app
--force && systemctl --user restart supvan-printer-app`.

Verify:

```sh
systemctl --user is-active supvan-printer-app
lpstat -v | grep supvan                 # direct ipp://localhost:8631 queue(s)
ipptool -tv ipp://localhost:8631/ipp/print/<queue> \
    /usr/share/cups/ipptool/get-printer-attributes.test | grep document-format
```

## System option (sudo, session-independent)

The base unit `/usr/lib/systemd/user/supvan-printer-app.service` ships an
`ExecStart=/usr/bin/supvan-printer-app server`. To run that instead, remove the
user drop-in above and install the binary system-wide:

```sh
sudo install -m755 target/release/supvan-printer-app /usr/bin/supvan-printer-app
sudo install -Dm644 data/models.toml /usr/share/supvan-printer-app/models.toml  # optional; binary also embeds it
systemctl --user daemon-reload && systemctl --user restart supvan-printer-app
```

For a true root-managed service (running without your login session), add an
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
