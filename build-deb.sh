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

CLI="target/release/katasymbol-cli"
PRINTER_APP="pappl/katasymbol-printer-app"

for bin in "$CLI" "$PRINTER_APP"; do
    [ -f "$bin" ] || { echo "Missing $bin — build failed?"; exit 1; }
done

# --- Strip binaries ---
echo "Stripping binaries..."
strip "$CLI" "$PRINTER_APP"

# --- Collect sizes for Installed-Size (in KiB) ---
INSTALLED_KB=$(( ( $(stat -c'%s' "$CLI") + $(stat -c'%s' "$PRINTER_APP") + $(stat -c'%s' katasymbol-printer-app.service) ) / 1024 ))

# --- Stage directory tree ---
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN"
mkdir -p "$STAGE/usr/bin"
mkdir -p "$STAGE/usr/lib/systemd/user"

install -m 0755 "$PRINTER_APP" "$STAGE/usr/bin/katasymbol-printer-app"
install -m 0755 "$CLI"         "$STAGE/usr/bin/katasymbol-cli"
install -m 0644 katasymbol-printer-app.service "$STAGE/usr/lib/systemd/user/katasymbol-printer-app.service"

# --- DEBIAN/control ---
cat > "$STAGE/DEBIAN/control" <<EOF
Package: $PKG
Version: $VERSION
Section: printing
Priority: optional
Architecture: $ARCH
Depends: libpappl1t64, libcups2t64, libdbus-1-3, liblzma5, libbluetooth3
Installed-Size: $INSTALLED_KB
Maintainer: Florian <florian@localhost>
Description: Katasymbol M50 Pro / Supvan T50 Pro thermal label printer
 PAPPL printer application providing IPP Everywhere support with DNS-SD
 auto-discovery for KDE/GNOME/macOS print dialogs. Includes a standalone
 CLI tool for diagnostics.
EOF

# --- DEBIAN/postinst ---
cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload 2>/dev/null || true
fi

echo ""
echo "Katasymbol printer driver installed."
echo ""
echo "Enable the printer application:"
echo "  systemctl --user enable --now katasymbol-printer-app"
echo "  # Printer auto-discovered via DNS-SD"
EOF
chmod 0755 "$STAGE/DEBIAN/postinst"

# --- DEBIAN/prerm ---
cat > "$STAGE/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ -d /run/systemd/system ]; then
    systemctl --user stop katasymbol-printer-app 2>/dev/null || true
    systemctl --user disable katasymbol-printer-app 2>/dev/null || true
fi
EOF
chmod 0755 "$STAGE/DEBIAN/prerm"

# --- Build .deb ---
echo "Building ${DEB_NAME}.deb ..."
fakeroot dpkg-deb --build "$STAGE" "target/deb/${DEB_NAME}.deb"

echo ""
echo "Package: target/deb/${DEB_NAME}.deb"
dpkg-deb --info "target/deb/${DEB_NAME}.deb"
echo ""
echo "Contents:"
dpkg-deb --contents "target/deb/${DEB_NAME}.deb"
echo ""
echo "Install with:  sudo dpkg -i target/deb/${DEB_NAME}.deb"
