#!/bin/sh
# Capture IPP golden traces from a running supvan-printer-app instance.
set -eu

PRINTER="${1:-}"
HOST="${SUPVAN_HOST:-localhost}"
PORT="${SUPVAN_PORT:-8631}"
OUT_DIR="$(dirname "$0")/../tests/fixtures/ipp"

if [ -z "$PRINTER" ]; then
  PRINTER=$(curl -sf "http://${HOST}:${PORT}/" | sed -n 's/.*<b>\([^<]*\)<\/b>.*/\1/p' | head -1)
fi

if [ -z "$PRINTER" ]; then
  echo "Usage: $0 <printer-name>" >&2
  echo "Or start the server and ensure the index page lists a printer." >&2
  exit 1
fi

URI="http://${HOST}:${PORT}/ipp/print/${PRINTER}"
mkdir -p "$OUT_DIR"

if command -v ipptool >/dev/null 2>&1; then
  TMP=$(mktemp)
  cat >"$TMP" <<EOF
{
    OPERATION Get-Printer-Attributes
    GROUP operation-attributes-tag
    ATTR charset attributes-charset (utf-8)
    ATTR language attributes-natural-language (en)
    ATTR uri printer-uri ($URI)
    ATTR name requesting-user-name (root)
    GROUP printer-attributes-tag
    ATTR keyword requested-attributes (all)
}
EOF
  ipptool -tv "$URI" "$TMP" >"$OUT_DIR/get-printer-attributes.trace" 2>&1 || true
  rm -f "$TMP"
  echo "Wrote $OUT_DIR/get-printer-attributes.trace (ipptool)"
else
  echo "ipptool not installed; using curl-only capture" >&2
fi

# Minimal Get-Printer-Attributes via curl (IPP binary from Rust test helper if present)
HELPER="$OUT_DIR/get-printer-attributes.req.bin"
if [ -f "$HELPER" ]; then
  curl -sf -X POST -H 'Content-Type: application/ipp' --data-binary "@$HELPER" \
    "$URI" -o "$OUT_DIR/get-printer-attributes.resp.bin" || true
  echo "Wrote $OUT_DIR/get-printer-attributes.resp.bin"
fi

echo "Printer: $PRINTER"
echo "URI: ipp://${HOST}:${PORT}/ipp/print/${PRINTER}"
