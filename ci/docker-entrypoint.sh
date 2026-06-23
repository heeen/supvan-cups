#!/usr/bin/env bash
# Boot dbus → avahi-daemon → cupsd → supvan-printer-app → cups-browsed, then
# hand off to the configured CMD (default: run-integration-test.sh).
#
# supvan-printer-app follows the CUPS-managed (IPP Everywhere / Printer
# Application) model: it advertises over DNS-SD and lets cupsd create a
# temporary on-demand queue. cups-browsed runs with `OnlyUnsupportedByCUPS Yes`
# (set in the Dockerfile) so it DEFERS our driverless printer to cupsd instead
# of building a duplicate implicitclass:// queue — it's started last, after
# cupsd has discovered our advert, so it sees the printer as already-supported.
set -euo pipefail

mkdir -p /run/dbus /var/run/avahi-daemon /var/lib/supvan/dumps
chown -R messagebus:messagebus /run/dbus 2>/dev/null || true

start() {
    local name=$1; shift
    echo "[entrypoint] starting $name: $*"
    "$@" >"/tmp/${name}.log" 2>&1 &
    echo $! > "/run/$name.pid"
}

wait_port() {
    local port=$1 label=$2 deadline=$((SECONDS + 30))
    until nc -z 127.0.0.1 "$port" 2>/dev/null; do
        if (( SECONDS >= deadline )); then
            echo "[entrypoint] timeout waiting for $label on :$port" >&2
            return 1
        fi
        sleep 0.2
    done
    echo "[entrypoint] $label up on :$port"
}

# dbus is needed by avahi + cups
start dbus dbus-daemon --system --nofork --nopidfile &
sleep 0.5

# avahi for mDNS — disable rlimits in containers, run as root for /run perms
start avahi avahi-daemon --no-rlimits --no-drop-root --debug

# cupsd: foreground so we get logs in `docker logs`
start cupsd /usr/sbin/cupsd -f
wait_port 631 cupsd

# supvan-printer-app — mock backend, advertises over mDNS via ipp-printer-app.
# Short poll/retry intervals keep the lifecycle scenarios snappy in CI.
export SUPVAN_PORT="${IPP_PORT}"
export IPP_PRINTER_APP_POLL_SECS="${IPP_PRINTER_APP_POLL_SECS:-2}"
export IPP_PRINTER_APP_RETRY_MS="${IPP_PRINTER_APP_RETRY_MS:-500}"
start supvan /usr/local/bin/supvan-printer-app
wait_port "${IPP_PORT}" supvan-printer-app

# Wait for cupsd to discover our DNS-SD advert (so it can auto-create a temp
# queue), THEN start cups-browsed: with OnlyUnsupportedByCUPS it sees the
# printer as already-CUPS-supported and defers instead of duplicating.
for _ in $(seq 1 30); do
    lpstat -e 2>/dev/null | grep -qi supvan && break
    sleep 0.5
done
start cups-browsed /usr/sbin/cups-browsed --debug

# Hand off to the test command.
if [[ "${1:-}" == "shell" ]]; then
    exec bash
fi
exec "$@"
