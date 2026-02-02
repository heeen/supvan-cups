#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

VERSION="0.1.0"
ARCH="$(dpkg --print-architecture)"
PKG="katasymbol"
DEB_NAME="${PKG}_${VERSION}_${ARCH}"
STAGE="$SCRIPT_DIR/target/deb/$DEB_NAME"

# --- Build release binaries ---
echo "Building release binaries..."
cargo build --workspace --release

# --- Build PAPPL printer application ---
echo "Building PAPPL printer application..."
make -C pappl

BACKEND="target/release/katasymbol"
FILTER="target/release/rastertokatasymbol"
CLI="target/release/katasymbol-cli"
PRINTER_APP="pappl/katasymbol-printer-app"

for bin in "$BACKEND" "$FILTER" "$CLI" "$PRINTER_APP"; do
    [ -f "$bin" ] || { echo "Missing $bin — build failed?"; exit 1; }
done

# --- Strip binaries ---
echo "Stripping binaries..."
strip "$BACKEND" "$FILTER" "$CLI" "$PRINTER_APP"

# --- Collect sizes for Installed-Size (in KiB) ---
INSTALLED_KB=$(( ( $(stat -c'%s' "$BACKEND") + $(stat -c'%s' "$FILTER") + $(stat -c'%s' "$CLI") + $(stat -c'%s' "$PRINTER_APP") + $(stat -c'%s' katasymbol.ppd) + $(stat -c'%s' katasymbol-printer-app.service) ) / 1024 ))

# --- Stage directory tree ---
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN"
mkdir -p "$STAGE/usr/lib/cups/backend"
mkdir -p "$STAGE/usr/lib/cups/filter"
mkdir -p "$STAGE/usr/share/ppd/katasymbol"
mkdir -p "$STAGE/usr/bin"
mkdir -p "$STAGE/usr/lib/systemd/user"

# Legacy CUPS backend/filter/PPD (still functional, kept for compatibility)
install -m 0700 "$BACKEND" "$STAGE/usr/lib/cups/backend/katasymbol"
install -m 0755 "$FILTER"  "$STAGE/usr/lib/cups/filter/rastertokatasymbol"
install -m 0644 katasymbol.ppd "$STAGE/usr/share/ppd/katasymbol/katasymbol.ppd"

# PAPPL printer application + CLI
install -m 0755 "$PRINTER_APP" "$STAGE/usr/bin/katasymbol-printer-app"
install -m 0755 "$CLI"         "$STAGE/usr/bin/katasymbol-cli"

# systemd user service
install -m 0644 katasymbol-printer-app.service "$STAGE/usr/lib/systemd/user/katasymbol-printer-app.service"

# --- DEBIAN/control ---
cat > "$STAGE/DEBIAN/control" <<EOF
Package: $PKG
Version: $VERSION
Section: printing
Priority: optional
Architecture: $ARCH
Depends: libpappl1t64, libcups2t64, libcupsimage2t64, libdbus-1-3, liblzma5, libbluetooth3, cups
Installed-Size: $INSTALLED_KB
Maintainer: Florian <florian@localhost>
Description: Katasymbol M50 Pro / Supvan T50 Pro thermal label printer driver
 Includes a PAPPL printer application (IPP Everywhere, auto-discovered via
 DNS-SD in KDE/GNOME/macOS), a standalone CLI tool, and legacy CUPS
 backend/filter/PPD for compatibility.
EOF

# --- DEBIAN/postinst ---
cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e

# Restart CUPS so it picks up the legacy backend/filter
if [ -d /run/systemd/system ]; then
    systemctl restart cups 2>/dev/null || true
    # Reload user service files for all logged-in users
    systemctl daemon-reload 2>/dev/null || true
fi

echo ""
echo "Katasymbol printer driver installed."
echo ""
echo "Recommended (PAPPL - works with KDE/GNOME print dialogs):"
echo "  systemctl --user enable --now katasymbol-printer-app"
echo "  # Printer auto-discovered via DNS-SD"
echo ""
echo "Legacy (CUPS direct):"
echo "  sudo lpadmin -p katasymbol_m50_pro -E \\"
echo "    -v katasymbol://A4:93:40:A0:87:57 \\"
echo "    -P /usr/share/ppd/katasymbol/katasymbol.ppd"
EOF
chmod 0755 "$STAGE/DEBIAN/postinst"

# --- DEBIAN/postrm ---
cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
    if [ -d /run/systemd/system ]; then
        systemctl restart cups 2>/dev/null || true
    fi
fi
EOF
chmod 0755 "$STAGE/DEBIAN/postrm"

# --- DEBIAN/prerm ---
cat > "$STAGE/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
# Stop the printer application if running (best effort for all users)
if [ -d /run/systemd/system ]; then
    # Try to stop for the current user context
    systemctl --user stop katasymbol-printer-app 2>/dev/null || true
    systemctl --user disable katasymbol-printer-app 2>/dev/null || true
fi
EOF
chmod 0755 "$STAGE/DEBIAN/prerm"

# --- Build .deb ---
echo "Building ${DEB_NAME}.deb ..."
# fakeroot so the backend gets root ownership inside the archive
fakeroot dpkg-deb --build "$STAGE" "target/deb/${DEB_NAME}.deb"

echo ""
echo "Package: target/deb/${DEB_NAME}.deb"
dpkg-deb --info "target/deb/${DEB_NAME}.deb"
echo ""
echo "Contents:"
dpkg-deb --contents "target/deb/${DEB_NAME}.deb" | grep -v '^\./\.$'
echo ""
echo "Install with:  sudo dpkg -i target/deb/${DEB_NAME}.deb"
