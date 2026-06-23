#!/usr/bin/env bash
# Integration test executed inside the CI container.
#
# Model under test: the CUPS-managed (IPP Everywhere / Printer Application)
# design. supvan-printer-app does NOT own a CUPS queue — it's a self-contained
# IPP Everywhere server that advertises over DNS-SD; cupsd creates a temporary
# on-demand queue when something prints. cups-browsed runs alongside with
# `OnlyUnsupportedByCUPS Yes`, so it defers to cupsd and does not duplicate us.
#
# What this verifies (all in one container, no external network):
#   1. supvan-printer-app boots in mock mode + serves IPP on $IPP_PORT.
#   2. Get-Printer-Attributes: sane PWG attrs, a printer-uuid, the human display
#      name in printer-info, and the full IPP Everywhere document-format set
#      (incl. image/jpeg).
#   3. Validate-Job is accepted for image/pwg-raster.
#   4. DNS-SD: we advertise the printer (by its display name) on _ipp._tcp.local,
#      and a co-resident cups-browsed (OnlyUnsupportedByCUPS Yes) defers to cupsd
#      instead of building a duplicate implicitclass:// queue.
#   5. Print-Job (PWG raster, ghostscript) round-trips through ipp-printer-app →
#      run_cups_raster_job → KsJob mock backend, landing a .pbm.
#   6. Print-Job (image/jpeg) round-trips through run_jpeg_job (decode →
#      contain-fit → dither) to a .pbm — proving image/jpeg is honestly decoded.
#   7. CUPS-managed temp queue: cupsd auto-creates a temporary queue from our
#      advert and prints through it (best-effort; cupsd discovery is async).
#   8. Offline reporting: an unreachable device → printer-state=stopped +
#      offline-report.
#   9. Hold-on-device-unavailable (the jam/offline behavior): a job submitted
#      while the device is unreachable is HELD (processing-stopped) and retried,
#      then prints when the device recovers — it is NOT dropped.
#  10. A paper jam likewise holds the job (it prints once the jam clears), while
#      a genuine permanent failure aborts.
set -euo pipefail

PORT="${IPP_PORT:-8631}"
# config.name = slug("Supvan Mock") = "supvan_mock" (underscores); the display
# name (DNS-SD instance / printer-info) is "Supvan Mock".
PRINTER="supvan_mock"
PRINTER_URI="ipp://127.0.0.1:${PORT}/ipp/print/${PRINTER}"
DUMP_DIR="${SUPVAN_DUMP_DIR:-/var/lib/supvan/dumps}"
TEST_SCRIPT_DIR="$(dirname "$0")"

# Restart supvan-printer-app with extra env vars layered on top of the
# docker-entrypoint env. Used for the lifecycle scenarios (unreachable device,
# recovery timer, fail-next-print).
restart_supvan() {
    if [[ -s /run/supvan.pid ]]; then
        kill "$(cat /run/supvan.pid)" 2>/dev/null || true
        sleep 1
    fi
    # Wipe persisted JSON so the new env-driven scenario starts from clean state.
    rm -f /root/.local/state/supvan-printer-app/state.json 2>/dev/null || true
    env "$@" /usr/local/bin/supvan-printer-app >/tmp/supvan.log 2>&1 &
    echo $! > /run/supvan.pid
    for _ in $(seq 1 40); do
        if curl -fs "http://127.0.0.1:${PORT}/" 2>/dev/null | grep -qi mock://; then
            return 0
        fi
        sleep 0.25
    done
    echo "supvan failed to come up after restart" >&2
    return 1
}

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

# Echo the IPP job-state for $1 as the keyword ipptool prints (`pending`,
# `processing`, `processing-stopped`, `canceled`, `aborted`, `completed`).
job_state() {
    cat >/tmp/get-job.test <<EOF
{
    OPERATION Get-Job-Attributes
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri \$uri
    ATTR integer job-id $1
    STATUS successful-ok
}
EOF
    ipptool -tv "${PRINTER_URI}" /tmp/get-job.test 2>/dev/null \
        | awk '/job-state \(enum\) =/ {print $NF; exit}'
}

# Submit a Print-Job (PWG raster in /tmp/test.pwg) and echo the new job-id.
submit_pwg_job() {
    ipptool -tv "${PRINTER_URI}" "$TEST_SCRIPT_DIR/print-job.test" 2>&1 \
        | awk '/job-id \(integer\) =/ {print $NF; exit}'
}

# Wait until at least one .pbm exists in DUMP_DIR (the mock backend dumped a
# page). Returns 0 on success, 1 on timeout.
wait_pbm() {
    for _ in $(seq 1 "${1:-20}"); do
        if find "$DUMP_DIR" -type f -name '*.pbm' 2>/dev/null | grep -q .; then
            return 0
        fi
        sleep 0.5
    done
    return 1
}

cat >/tmp/expect-reason.tpl <<'EOF'
{
    OPERATION Get-Printer-Attributes
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri $uri
    STATUS successful-ok
    EXPECT printer-state-reasons WITH-VALUE "__REASON__"
}
EOF
expect_reason() {
    sed "s|__REASON__|$1|" /tmp/expect-reason.tpl > /tmp/expect-reason.test
    ipptool -tv "${PRINTER_URI}" /tmp/expect-reason.test
}

# Generate the PWG raster sample once (30×20mm @ 203dpi ≈ 240×160px, mono).
mkdir -p "$DUMP_DIR"
gs -q -dNOPAUSE -dBATCH -sDEVICE=pwgraster -sOutputFile=/tmp/test.pwg \
    -g240x160 -r203 -c "showpage" 2>&1 | head -5
test -s /tmp/test.pwg

##############################################################################
# 1–4. Discovery + IPP attributes + Validate-Job + DNS-SD advert.
##############################################################################
step "Wait for IPP backend (mock printer auto-registered)"
for _ in $(seq 1 30); do
    curl -fs "http://127.0.0.1:${PORT}/" 2>/dev/null | grep -qi "mock://" && break
    sleep 0.5
done
curl -fs "http://127.0.0.1:${PORT}/" 2>/dev/null | grep -qi "mock://" \
    || { echo "FAIL: IPP index never advertised mock://" >&2; exit 1; }

step "Get-Printer-Attributes via ipptool"
cat >/tmp/get-attrs.test <<'EOF'
{
    OPERATION Get-Printer-Attributes
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri $uri
    STATUS successful-ok
    EXPECT printer-name OF-TYPE nameWithoutLanguage WITH-VALUE "supvan_mock"
    EXPECT printer-info OF-TYPE textWithoutLanguage WITH-VALUE "Supvan Mock"
    EXPECT printer-state OF-TYPE enum
    EXPECT document-format-supported OF-TYPE mimeMediaType WITH-VALUE "image/pwg-raster"
    EXPECT document-format-supported OF-TYPE mimeMediaType WITH-VALUE "image/jpeg"
    EXPECT urf-supported OF-TYPE keyword
    EXPECT printer-uuid OF-TYPE uri WITH-VALUE "/^urn:uuid:/"
}
EOF
ipptool -tv "${PRINTER_URI}" /tmp/get-attrs.test

step "Validate-Job via ipptool (image/pwg-raster)"
cat >/tmp/validate-job.test <<'EOF'
{
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

step "DNS-SD advertisement (_ipp._tcp.local)"
# The DNS-SD service instance name is the display name ("Supvan Mock"); the
# `rp` TXT carries the logical resource path (ipp/print/supvan_mock).
seen_mdns=0
for attempt in $(seq 1 6); do
    timeout 4 avahi-browse -rpt _ipp._tcp 2>/dev/null >/tmp/avahi-browse.out || true
    if grep -qiE 'supvan.mock' /tmp/avahi-browse.out; then
        seen_mdns=1; break
    fi
    echo "  attempt $attempt: not in browse output yet, retrying..."
    sleep 2
done
if (( ! seen_mdns )); then
    echo "FAIL: printer not seen on mDNS after 6 attempts" >&2
    cat /tmp/avahi-browse.out || true
    exit 1
fi
echo "DNS-SD advert confirmed:"
grep -iE 'supvan.mock' /tmp/avahi-browse.out | head -3

step "cups-browsed coexistence: defers to cupsd, no duplicate implicitclass queue"
# cups-browsed runs with `OnlyUnsupportedByCUPS Yes` (Dockerfile) and was
# started after cupsd discovered our advert, so it must treat the printer as
# already-CUPS-supported and NOT build its own queue. Give it a couple of
# resolve cycles to settle, then assert no implicitclass queue exists for us.
sleep 8
lpstat -v 2>/dev/null > /tmp/queues.out || true
if grep -i implicitclass /tmp/queues.out | grep -qi supvan; then
    echo "FAIL: cups-browsed built a duplicate implicitclass queue for our printer" >&2
    grep -i implicitclass /tmp/queues.out >&2
    exit 1
fi
if grep -qi "already supported by CUPS, skipping" /tmp/cups-browsed.log 2>/dev/null; then
    echo "cups-browsed deferred to cupsd (logged 'already supported by CUPS, skipping') — OK"
    grep -i "already supported by CUPS, skipping" /tmp/cups-browsed.log | head -1
else
    echo "no implicitclass duplicate for our printer — OK (defer log not captured)"
fi

##############################################################################
# 5–6. Print-Job round-trips (direct IPP).
##############################################################################
step "Print-Job round-trip (PWG raster) via ipptool"
rm -rf "$DUMP_DIR" && mkdir -p "$DUMP_DIR"
ipptool -tv "${PRINTER_URI}" "$TEST_SCRIPT_DIR/print-job.test"
wait_pbm || { echo "FAIL: no .pbm landed in $DUMP_DIR" >&2; ls -la "$DUMP_DIR" || true; exit 1; }
echo "PWG render dumped:"; find "$DUMP_DIR" -type f | sort | head

step "Generate a JPEG sample with ghostscript"
gs -q -dNOPAUSE -dBATCH -sDEVICE=jpeg -sOutputFile=/tmp/test.jpg \
    -g400x300 -r203 -c "0.5 setgray clippath fill showpage" 2>&1 | head -5
test -s /tmp/test.jpg

step "Print-Job round-trip (image/jpeg) via ipptool"
rm -rf "$DUMP_DIR" && mkdir -p "$DUMP_DIR"
cat >/tmp/print-jpeg.test <<'EOF'
{
    NAME "Print-Job image/jpeg"
    OPERATION Print-Job
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri $uri
    ATTR name requesting-user-name ipp-test
    ATTR name job-name ci-jpeg
    ATTR mimeMediaType document-format image/jpeg
    FILE /tmp/test.jpg
    STATUS successful-ok
    EXPECT job-id OF-TYPE integer COUNT 1
    EXPECT job-state OF-TYPE enum WITH-VALUE >=3
}
EOF
ipptool -tv "${PRINTER_URI}" /tmp/print-jpeg.test
wait_pbm || { echo "FAIL: image/jpeg job produced no .pbm (decode/render failed)" >&2; tail -20 /tmp/supvan.log; exit 1; }
echo "image/jpeg decoded + rendered:"; find "$DUMP_DIR" -type f | sort | head

##############################################################################
# 7. CUPS-managed temporary queue (best-effort).
#
# With cups-browsed off, cupsd itself creates a temporary on-demand queue from
# our DNS-SD advert when we print to it. cupsd's DNS-SD discovery is async, so
# this is best-effort: a failure here is a warn, not a hard fail (the direct-IPP
# round-trips above already prove the print path).
##############################################################################
step "CUPS-managed temp queue prints through cupsd (best-effort)"
rm -rf "$DUMP_DIR" && mkdir -p "$DUMP_DIR"
echo "ci cups-managed smoke" > /tmp/lp-input.txt
if lp -d "Supvan Mock" /tmp/lp-input.txt 2>/tmp/lp.err; then
    if wait_pbm 20; then
        echo "cupsd temp queue printed through to the mock backend — OK"
        tmpq=$(lpstat -v 2>/dev/null | grep -i 'supvan' | head -1 || true)
        [[ -n "$tmpq" ]] && echo "  temp queue: $tmpq"
    else
        echo "warn: cupsd accepted the job but no .pbm landed (temp-queue discovery may lag)"
    fi
else
    echo "warn: lp to the discovered 'Supvan Mock' didn't create a temp queue (cupsd dnssd timing): $(cat /tmp/lp.err)"
fi

##############################################################################
# 8. Offline reporting: an unreachable device → stopped + offline-report.
##############################################################################
step "Offline reporting: unreachable device → printer-state=stopped + offline-report"
restart_supvan SUPVAN_MOCK_UNREACHABLE=1
sleep 3   # > poll interval so the status poller has run
expect_reason "offline-report"
cat >/tmp/expect-stopped.test <<'EOF'
{
    OPERATION Get-Printer-Attributes
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri $uri
    STATUS successful-ok
    EXPECT printer-state OF-TYPE enum WITH-VALUE 5
}
EOF
ipptool -tv "${PRINTER_URI}" /tmp/expect-stopped.test   # 5 = stopped

##############################################################################
# 9. Hold-on-device-unavailable: a job submitted while the device is
#    unreachable is HELD and retried, then prints when the device recovers.
##############################################################################
step "Hold-then-recover: job submitted while offline is held, prints on recovery"
rm -rf "$DUMP_DIR" && mkdir -p "$DUMP_DIR"
# Unreachable now, recovering after 6s; fast poll + retry so the test is snappy.
restart_supvan SUPVAN_MOCK_UNREACHABLE=1 SUPVAN_MOCK_RECOVER_AFTER_MS=6000 \
    IPP_PRINTER_APP_POLL_SECS=1 IPP_PRINTER_APP_RETRY_MS=300
sleep 2
job_id=$(submit_pwg_job)
echo "submitted job while offline: id=$job_id"
[[ -n "$job_id" ]] || { echo "FAIL: no job-id from Print-Job" >&2; exit 1; }

# Shortly after submit (still offline) the job must be held, NOT terminal.
sleep 1
held_state=$(job_state "$job_id")
echo "job-state while device offline: ${held_state:-<none>} (expect processing-stopped)"
if [[ "$held_state" == "aborted" || "$held_state" == "canceled" ]]; then
    echo "FAIL: job was $held_state instead of held while the device was offline" >&2
    exit 1
fi
grep -q "held: device unavailable" /tmp/supvan.log \
    && echo "framework logged the hold" || echo "note: hold log not captured"

# After recovery the held job must print: a .pbm lands and the job completes.
if ! wait_pbm 30; then
    echo "FAIL: held job never printed after the device recovered" >&2
    tail -20 /tmp/supvan.log; exit 1
fi
final_state=$(job_state "$job_id")
echo "job-state after recovery: ${final_state:-<none>} (expect completed)"
[[ "$final_state" == "completed" ]] || { echo "FAIL: recovered job did not complete (state=$final_state)" >&2; exit 1; }
echo "held job printed after recovery — OK"
expect_reason "none"   # back to ready

##############################################################################
# 10. A paper jam holds (then prints once cleared); a permanent failure aborts.
##############################################################################
step "Paper jam holds the job (clears on retry), does not abort"
rm -rf "$DUMP_DIR" && mkdir -p "$DUMP_DIR"
# SUPVAN_MOCK_FAIL is single-shot: the first attempt fails with media-jam (a
# recoverable condition → the framework holds + retries), the retry succeeds.
restart_supvan SUPVAN_MOCK_FAIL=media-jam IPP_PRINTER_APP_RETRY_MS=300
job_id=$(submit_pwg_job)
echo "submitted job into a jam: id=$job_id"
if ! wait_pbm 20; then
    echo "FAIL: jammed job never printed (should hold + retry, not abort)" >&2
    tail -20 /tmp/supvan.log; exit 1
fi
jam_state=$(job_state "$job_id")
echo "job-state after the jam cleared: ${jam_state:-<none>} (expect completed)"
[[ "$jam_state" == "completed" ]] || { echo "FAIL: jammed job did not complete (state=$jam_state)" >&2; exit 1; }
echo "jammed job held + printed on retry — OK"

step "Permanent failure aborts the job (no retry)"
# SUPVAN_MOCK_FAIL=other is NOT a recoverable condition → the framework aborts.
restart_supvan SUPVAN_MOCK_FAIL=other
job_id=$(submit_pwg_job)
echo "submitted job that fails permanently: id=$job_id"
aborted=0
for _ in $(seq 1 20); do
    s=$(job_state "$job_id")
    if [[ "$s" == "aborted" ]]; then aborted=1; break; fi
    [[ "$s" == "completed" ]] && { echo "FAIL: permanent failure unexpectedly completed" >&2; exit 1; }
    sleep 0.5
done
(( aborted )) || { echo "FAIL: permanent failure did not abort the job" >&2; exit 1; }
echo "permanent failure aborted the job — OK"

echo
echo "PASS: discovery + IPP attributes + cups-browsed coexistence + Print-Job"
echo "      (PWG + JPEG) + CUPS temp queue + offline reporting + hold-then-recover"
echo "      + jam-holds + permanent-abort"
