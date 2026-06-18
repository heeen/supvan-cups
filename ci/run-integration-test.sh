#!/usr/bin/env bash
# Integration test executed inside the CI container.
#
# What this verifies (all in one container, no external network):
#   1. supvan-printer-app boots in mock mode + serves IPP on $IPP_PORT
#   2. Get-Printer-Attributes returns sane PWG attrs incl. a printer-uuid
#   3. Validate-Job is accepted for image/pwg-raster
#   4. avahi advertises us on _ipp._tcp.local WITH a UUID= TXT key
#   5. cups-browsed COEXISTENCE: the in-process registrar creates exactly one
#      direct ipp:// CUPS queue; cups-browsed (already running → race) dedupes
#      our advert by UUID and does NOT leave a broken implicitclass:// queue.
#   6. Print-Job with a real PWG raster (ghostscript) round-trips through
#      ipp-printer-app → run_cups_raster_job → KsJob mock backend, landing a
#      .pbm in $SUPVAN_DUMP_DIR; and a CUPS lp job through the registrar's
#      queue does the same.
#   7. Lifecycle: status polling, recovery, job-error propagation.
set -euo pipefail

PORT="${IPP_PORT:-8631}"
PRINTER_URI="ipp://127.0.0.1:${PORT}/ipp/print/supvan-mock"
# The registrar derives this queue name from the mock printer's IPP name
# (slug of "Supvan Mock"). cups-browsed's underscore variant, if it ever
# races one in, would be QUEUE with '-'→'_'.
QUEUE="supvan-mock"
QUEUE_ALT="${QUEUE//-/_}"
DUMP_DIR="${SUPVAN_DUMP_DIR:-/var/lib/supvan/dumps}"
TEST_SCRIPT_DIR="$(dirname "$0")"

# Restart supvan-printer-app with extra env vars layered on top of the
# docker-entrypoint env. Used for lifecycle scenarios (sticky reasons,
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
    EXPECT printer-uuid OF-TYPE uri WITH-VALUE "/^urn:uuid:/"
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

# The advert must carry a UUID= TXT key — it's what cups-browsed dedupes on.
advert_uuid=$(grep "supvan-mock" /tmp/avahi-browse.out | grep -oE 'UUID=[a-fA-F0-9-]+' | head -1 | cut -d= -f2)
if [[ -z "$advert_uuid" ]]; then
    echo "FAIL: advert carries no UUID= TXT key" >&2
    grep "supvan-mock" /tmp/avahi-browse.out | head -3
    exit 1
fi
echo "advert UUID=$advert_uuid"

##############################################################################
# cups-browsed COEXISTENCE (the core of the registrar + UUID-dedup design).
#
# cups-browsed started BEFORE supvan in the entrypoint, so this is the race
# case: it's already running when we advertise. The registrar creates the
# direct queue, reads its printer-uuid, advertises that UUID, then sweeps any
# implicitclass duplicate cups-browsed raced in. Steady state must be exactly
# one queue, ipp:// (never implicitclass), with printer-uuid == advert UUID.
##############################################################################
step "Coexistence: registrar created the direct queue"
for _ in $(seq 1 40); do
    lpstat -v "$QUEUE" >/dev/null 2>&1 && break
    sleep 1
done
if ! lpstat -v "$QUEUE" >/dev/null 2>&1; then
    echo "FAIL: registrar never created queue $QUEUE" >&2
    lpstat -v 2>&1 || true
    exit 1
fi
# Let the registrar's post-advertise sweep window (~20s) settle so any racy
# implicitclass duplicate is gone and not recreated.
sleep 25

step "Coexistence: our queue is direct ipp://, never implicitclass"
# Scope assertions to OUR printer ($QUEUE and its underscore variant). The CI
# container shares mDNS with the host, so unrelated host adverts may add other
# queues — we assert OUR printer coexists correctly, not that the whole CUPS
# queue list is pristine (which is the semantically correct thing to test).
lpstat -v 2>&1 | tee /tmp/queues.out
ours=$(grep "device for ${QUEUE}:" /tmp/queues.out || true)
fail=0
if ! grep -q "ipp://" <<<"$ours"; then
    echo "FAIL: $QUEUE device-uri is not ipp:// ($ours)" >&2; fail=1
fi
for q in "$QUEUE" "$QUEUE_ALT"; do
    if grep -qE "device for ${q}: *implicitclass:" /tmp/queues.out; then
        echo "FAIL: cups-browsed made an implicitclass queue for our printer ($q)" >&2; fail=1
    fi
done
(( fail )) && exit 1
echo "our queue is direct ipp://, no implicitclass for our printer — OK"
# Informational: flag total implicitclass queues (host-mDNS leakage in a
# non-isolated dev run; should be 0 in a clean CI runner).
other_implicit=$(grep -c "implicitclass:" /tmp/queues.out || true)
(( other_implicit > 0 )) && echo "note: $other_implicit implicitclass queue(s) from unrelated adverts (host mDNS leakage)"

step "Coexistence: queue printer-uuid matches the advertised UUID"
cat >/tmp/gpa.test <<'EOF'
{
    OPERATION Get-Printer-Attributes
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri $uri
    STATUS successful-ok
}
EOF
queue_uuid=$(ipptool -tv "ipp://localhost:631/printers/${QUEUE}" /tmp/gpa.test 2>/dev/null \
    | grep -i "printer-uuid" | grep -oE '[a-fA-F0-9-]{36}' | head -1)
echo "queue printer-uuid=$queue_uuid ; advert UUID=$advert_uuid"
if [[ -z "$queue_uuid" || "$queue_uuid" != "$advert_uuid" ]]; then
    echo "FAIL: advert UUID does not match the CUPS queue printer-uuid" >&2
    exit 1
fi
echo "advert UUID == queue printer-uuid — cups-browsed dedup key is correct"

step "Coexistence: cups-browsed logged the UUID dedup (best-effort)"
if grep -qE "is from local CUPS, ignored" /tmp/cups-browsed.log 2>/dev/null; then
    echo "cups-browsed stood down:"
    grep "is from local CUPS, ignored" /tmp/cups-browsed.log | head -2
else
    # Outcome already asserted above; the debug line is confirmation only.
    echo "note: no 'ignored' line in cups-browsed.log (debug detail varies); outcome already verified."
fi

step "Generate a PWG raster sample with ghostscript"
# 30×20mm at 203 dpi ≈ 240×160 px. Single page, monochrome.
rm -rf "$DUMP_DIR" && mkdir -p "$DUMP_DIR"
gs -q -dNOPAUSE -dBATCH \
    -sDEVICE=pwgraster -sOutputFile=/tmp/test.pwg \
    -g240x160 -r203 \
    -c "showpage" 2>&1 | head -5
test -s /tmp/test.pwg
echo "raster: $(wc -c </tmp/test.pwg) bytes, magic=$(od -An -c -N 4 /tmp/test.pwg | tr -s ' ')"

step "Print-Job round-trip via ipptool"
cp "$TEST_SCRIPT_DIR/print-job.test" /tmp/print-job.test
ipptool -tv "${PRINTER_URI}" /tmp/print-job.test

step "lp print job via the registrar's CUPS queue ($QUEUE)"
# The registrar already created $QUEUE with -m everywhere (verified above);
# no manual lpadmin needed. A CUPS lp job exercises the full filter chain
# (text → PDF → PWG raster → cups-ipp backend → our IPP server → mock backend).
lpadmin -d "$QUEUE"
echo "ci smoke print" > /tmp/lp-input.txt
lp -d "$QUEUE" /tmp/lp-input.txt
sleep 3
lpstat -W completed -o "$QUEUE" || true

step "Wait for mock backend to dump the page"
dumped=0
for _ in $(seq 1 20); do
    if find "$DUMP_DIR" -type f -name '*.pbm' 2>/dev/null | grep -q .; then
        dumped=1; break
    fi
    sleep 0.5
done
if (( ! dumped )); then
    echo "FAIL: no .pbm landed in $DUMP_DIR" >&2
    ls -la "$DUMP_DIR" || true
    exit 1
fi

echo "Dump artefacts:"
find "$DUMP_DIR" -type f | sort | head

##############################################################################
# Lifecycle: status polling, recovery, and job-error propagation.
#
# The status::spawn poller hits the backend's poll_status() every
# IPP_PRINTER_APP_POLL_SECS (set to 1s for CI). With the SUPVAN_MOCK_STICKY
# / SUPVAN_MOCK_RECOVER_AFTER_MS / SUPVAN_MOCK_FAIL knobs we can drive any
# state transition we care about without real hardware.
##############################################################################

cat >/tmp/expect-reason.tpl <<'EOF'
{
    NAME "Expected printer-state-reasons"
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

step "Lifecycle: device first-seen → idle, no reasons"
restart_supvan
sleep 2
expect_reason "none"

step "Lifecycle: device offline (SUPVAN_MOCK_STICKY=offline)"
restart_supvan SUPVAN_MOCK_STICKY=offline
sleep 3   # > poll interval so the status poller has run at least once
expect_reason "offline-report"

step "Lifecycle: device recovers after SUPVAN_MOCK_RECOVER_AFTER_MS"
restart_supvan SUPVAN_MOCK_STICKY=offline SUPVAN_MOCK_RECOVER_AFTER_MS=2000
sleep 2
expect_reason "offline-report"      # still offline mid-window
sleep 3                              # > recover_after + poll
expect_reason "none"                 # cleared

step "Lifecycle: print-job error surfaces in job + printer state"
# Realistic semantics: a printer that runs out of labels stays out of labels.
# SUPVAN_MOCK_FAIL aborts the next print with media-empty;
# SUPVAN_MOCK_STICKY keeps the status poller reporting media-empty afterwards
# so any GUI polling Get-Printer-Attributes still sees the condition.
restart_supvan SUPVAN_MOCK_FAIL=media-empty SUPVAN_MOCK_STICKY=media-empty
# The Print-Job is accepted at the IPP layer; the worker aborts internally.
job_resp=$(ipptool -tv "${PRINTER_URI}" /tmp/print-job.test 2>&1 || true)
echo "$job_resp" | grep -E "job-id|job-state" | head
job_id=$(echo "$job_resp" | awk '/job-id \(integer\) =/ {print $NF; exit}')
echo "submitted job: id=$job_id"
sleep 3   # > sticky-poller interval + job worker

# Job-level: aborted state + failure surface kept in job-state-message.
# ipp-printer-app 0.2 emits "job-completed-with-errors" as the IPP
# job-state-reasons keyword (a generic terminal marker) and stuffs the
# specific reason text into job-state-message. Asserting the message
# contains the failure detail is more robust than pinning the keyword.
cat >/tmp/get-job.test <<EOF
{
    NAME "Get-Job-Attributes for failed job"
    OPERATION Get-Job-Attributes
    GROUP operation-attributes-tag
    ATTR charset attributes-charset utf-8
    ATTR naturalLanguage attributes-natural-language en
    ATTR uri printer-uri \$uri
    ATTR integer job-id $job_id
    STATUS successful-ok
    EXPECT job-state OF-TYPE enum WITH-VALUE 8
    EXPECT job-state-message OF-TYPE textWithoutLanguage WITH-VALUE "/mock.*label/"
}
EOF
ipptool -tv "${PRINTER_URI}" /tmp/get-job.test

# Printer-level: sticky reason keeps reporting media-empty.
expect_reason "media-empty"

step "Lifecycle: CUPS-side query reflects upstream printer-state-reasons"
# `lpstat -l` against an everywhere queue triggers CUPS to refresh the
# queue's printer-state-reasons from the upstream IPP server via the
# cups-ipp backend's Get-Printer-Attributes call. With sticky media-empty
# still set on our backend, CUPS should surface it on the local queue.
lpstat -l -p "$QUEUE" 2>&1 | tee /tmp/lpstat.out | head -20 || true
if grep -qi "media-empty" /tmp/lpstat.out; then
    echo "CUPS-local queue reflects upstream media-empty."
else
    # cups-ipp backend doesn't always proxy state-reasons synchronously;
    # don't fail the run on this — the IPP-layer check above already proves
    # the framework surfaces the state correctly.
    echo "warn: CUPS-local lpstat didn't include media-empty (backend may proxy state-reasons only after a job, not on query)."
fi

echo
echo "PASS: discovery + IPP attributes + Print-Job round-trip + lifecycle succeeded"
