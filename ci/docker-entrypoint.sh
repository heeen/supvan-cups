#!/usr/bin/env bash
# Boot dbus → avahi-daemon → cupsd → cups-browsed → supvan-printer-app, then
# hand off to the configured CMD (default: run-integration-test.sh).
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

# cups-browsed picks up mDNS-advertised IPP services and creates queues
start cups-browsed /usr/sbin/cups-browsed

# supvan-printer-app — mock backend, advertises over mDNS via ipp-printer-app
export SUPVAN_PORT="${IPP_PORT}"
start supvan /usr/local/bin/supvan-printer-app
wait_port "${IPP_PORT}" supvan-printer-app

# Hand off to the test command.
if [[ "${1:-}" == "shell" ]]; then
    exec bash
fi
exec "$@"
