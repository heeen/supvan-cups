#!/bin/sh
# Advisory cups-browsed coexistence check, run from the Makefile install/deploy
# targets. supvan-printer-app is a self-contained IPP Everywhere printer: CUPS
# auto-creates a temporary on-demand queue for it. A co-resident cups-browsed
# WITHOUT `OnlyUnsupportedByCUPS Yes` will instead build a duplicate, broken
# `implicitclass://` queue (its backend can't route to a same-host service).
#
# This detects exactly that pathological case and prints the one-line fix. It is
# advisory only: it never edits system config and never fails the install.
set -eu

# Override for testing.
CONF="${SUPVAN_CUPS_BROWSED_CONF:-/etc/cups/cups-browsed.conf}"

# Is cups-browsed actually running? (systemd, or a bare process.)
active=no
if command -v systemctl >/dev/null 2>&1 && systemctl is-active --quiet cups-browsed 2>/dev/null; then
    active=yes
elif pgrep -x cups-browsed >/dev/null 2>&1; then
    active=yes
fi
[ "$active" = yes ] || exit 0   # not running → nothing to coexist with

# Already configured to defer to CUPS? (OnlyUnsupportedByCUPS Yes/On/True/1)
if [ -r "$CONF" ] && \
   grep -iqE '^[[:space:]]*OnlyUnsupportedByCUPS[[:space:]]+(yes|on|true|1)' "$CONF"; then
    exit 0   # correct config → no warning
fi

cat >&2 <<EOF

  ⚠  cups-browsed is running without 'OnlyUnsupportedByCUPS Yes'.

     supvan-printer-app advertises as an IPP Everywhere printer and CUPS creates
     an on-demand queue for it. cups-browsed will ALSO create one and, for a
     same-host printer, it builds a broken implicitclass:// duplicate.

     Fix (one line; keeps cups-browsed running for everything else):

         echo 'OnlyUnsupportedByCUPS Yes' | sudo tee -a $CONF
         sudo systemctl restart cups-browsed

     See docs/DEPLOY.md ("cups-browsed coexistence") for the rationale.

EOF
exit 0   # advisory only
