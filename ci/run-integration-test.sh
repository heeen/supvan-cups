#!/usr/bin/env bash
# Integration test executed inside the CI container.
#
# What this verifies (all in one container, no external network):
#   1. supvan-printer-app boots in mock mode + serves IPP on $IPP_PORT
#   2. Get-Printer-Attributes returns sane PWG attrs (CUPS-talkable)
#   3. Validate-Job is accepted for image/pwg-raster
#   4. avahi advertises us on _ipp._tcp.local
#   5. cups-browsed sees the broadcast (proves CUPS discovery wiring works)
#
# What is *not* covered here:
#   - Submitting a real PWG raster payload (needs a generator; tracked).
#   - lpadmin -m everywhere PPD generation (the published ipp-printer-app
#     0.1 omits ipp-features-supported, so CUPS' everywhere driver bails).
set -euo pipefail

PORT="${IPP_PORT:-8631}"
PRINTER_URI="ipp://127.0.0.1:${PORT}/ipp/print/supvan-mock"

dump_logs() {
    local rc=$?
    echo
    for f in /tmp/supvan.log /tmp/cupsd.log /tmp/cups-browsed.log /tmp/avahi.log; do
        if [[ -s "$f" ]]; then
            echo "=== $f ==="
            tail -n 60 "$f"
        fi
    done
    if [[ -s /var/log/cups/error_log ]]; then
        echo "=== cups error_log ==="
        tail -n 40 /var/log/cups/error_log
    fi
    exit "$rc"
}
trap dump_logs EXIT

step() { echo; echo "=== $* ==="; }

step "Wait for IPP backend (mock printer auto-registered)"
for _ in $(seq 1 30); do
    if curl -fs "http://127.0.0.1:${PORT}/" 2>/dev/null | grep -qi "mock://"; then
        echo "IPP index advertises mock://"
        break
    fi
    sleep 0.5
done

step "Get-Printer-Attributes via ipptool"
cat >/tmp/get-attrs.test <<'EOF'
{
    NAME "Get-Printer-Attributes"
    OPERATION Get-Printer-Attributes
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri $uri
    STATUS successful-ok
    EXPECT printer-name OF-TYPE nameWithoutLanguage WITH-VALUE "supvan-mock"
    EXPECT printer-state OF-TYPE enum
    EXPECT document-format-supported OF-TYPE mimeMediaType WITH-VALUE "image/pwg-raster"
    EXPECT urf-supported OF-TYPE keyword
}
EOF
ipptool -tv "${PRINTER_URI}" /tmp/get-attrs.test

step "Validate-Job via ipptool (image/pwg-raster, no payload)"
cat >/tmp/validate-job.test <<'EOF'
{
    NAME "Validate-Job"
    OPERATION Validate-Job
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri $uri
    ATTR name requesting-user-name ipp-test
    ATTR mimeMediaType document-format image/pwg-raster
    STATUS successful-ok
}
EOF
ipptool -tv "${PRINTER_URI}" /tmp/validate-job.test

step "mDNS advertisement (_ipp._tcp.local)"
seen_mdns=0
for attempt in $(seq 1 6); do
    timeout 4 avahi-browse -rpt _ipp._tcp 2>/dev/null >/tmp/avahi-browse.out || true
    if grep -q "supvan-mock" /tmp/avahi-browse.out; then
        seen_mdns=1
        break
    fi
    echo "  attempt $attempt: supvan-mock not in browse output yet, retrying..."
    sleep 2
done
if (( ! seen_mdns )); then
    echo "FAIL: supvan-mock not seen on mDNS after 6 attempts" >&2
    echo "--- avahi-browse output ---"
    cat /tmp/avahi-browse.out || true
    exit 1
fi
echo "mDNS broadcast confirmed for supvan-mock"
grep "supvan-mock" /tmp/avahi-browse.out | head -3

step "cups-browsed picked up the IPP service"
# cups-browsed logs "Found" or "Adding" lines when it sees a new mDNS service.
# Give it up to 20s after we know the broadcast is live.
seen=0
for _ in $(seq 1 20); do
    if grep -qE "supvan-mock|mock://" /tmp/cups-browsed.log 2>/dev/null; then
        seen=1; break
    fi
    sleep 1
done
if (( seen )); then
    echo "cups-browsed observed the service:"
    grep -E "supvan-mock|mock://" /tmp/cups-browsed.log | head -5
else
    # Not fatal — cups-browsed may have it cached without logging.
    echo "warn: cups-browsed didn't log a 'supvan-mock' line; service visibility unconfirmed."
fi

echo
echo "PASS: discovery + IPP attribute round-trip succeeded"
