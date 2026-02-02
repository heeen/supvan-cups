#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BACKEND_SRC="$SCRIPT_DIR/target/release/katasymbol"
FILTER_SRC="$SCRIPT_DIR/target/release/rastertokatasymbol"
PPD_SRC="$SCRIPT_DIR/katasymbol.ppd"

BACKEND_DST="/usr/lib/cups/backend/katasymbol"
FILTER_DST="/usr/lib/cups/filter/rastertokatasymbol"
PPD_DIR="/usr/share/ppd/katasymbol"
PPD_DST="$PPD_DIR/katasymbol.ppd"

# Check binaries exist
if [ ! -f "$BACKEND_SRC" ] || [ ! -f "$FILTER_SRC" ]; then
    echo "Build first: cargo build --release"
    exit 1
fi

echo "Installing katasymbol CUPS driver..."

# Backend must be mode 0700 owned by root (CUPS requirement)
sudo install -m 0700 "$BACKEND_SRC" "$BACKEND_DST"
echo "  backend  -> $BACKEND_DST"

# Filter
sudo install -m 0755 "$FILTER_SRC" "$FILTER_DST"
echo "  filter   -> $FILTER_DST"

# PPD
sudo mkdir -p "$PPD_DIR"
sudo install -m 0644 "$PPD_SRC" "$PPD_DST"
echo "  ppd      -> $PPD_DST"

# Restart CUPS
sudo systemctl restart cups
echo "  cups restarted"

echo ""
echo "Done. Add printer via:"
echo "  lpadmin -p katasymbol_m50_pro -E -v katasymbol://A4:93:40:A0:87:57 -P $PPD_DST"
echo ""
echo "Or use CUPS web UI at http://localhost:631/admin"
