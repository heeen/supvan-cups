#!/usr/bin/env bash
#
# extract.sh — Recover original JavaScript source code from a Windows Electron
#               app installer via NSIS extraction, asar unpacking, and source
#               map reconstruction.
#
# Target: KatasymbolEditor Setup 1.1.1.exe  (a Katasymbol printer label editor)
#
# Prerequisites:
#   - 7z        (p7zip-full on Debian/Ubuntu)
#   - node/npx  (Node.js, any recent LTS)
#   - file      (usually pre-installed)
#
# Usage:
#   chmod +x extract.sh
#   ./extract.sh
#
# All output is written under the current working directory.
# ---------------------------------------------------------------------------

set -euo pipefail

INSTALLER="KatasymbolEditor Setup 1.1.1.exe"
BASEDIR="$(cd "$(dirname "$0")" && pwd)"
cd "$BASEDIR"

# ──────────────────────────────────────────────────────────────────────────────
# STEP 0 — Identify the installer format
# ──────────────────────────────────────────────────────────────────────────────
# The `file` command reads magic bytes at the start of the binary.  NSIS
# (Nullsoft Scriptable Install System) is a very common open-source Windows
# installer framework.  `file` reports:
#
#   PE32 executable ... Nullsoft Installer self-extracting archive
#
# Knowing it is NSIS tells us that the installer is essentially a self-
# extracting 7z/deflate archive with a small stub executable prepended,
# which means generic archive tools can open it.

echo "=== Step 0: Identifying installer format ==="
file "$INSTALLER"
echo

# ──────────────────────────────────────────────────────────────────────────────
# STEP 1 — Extract the NSIS installer with 7z
# ──────────────────────────────────────────────────────────────────────────────
# 7-Zip understands the NSIS container format natively.  It treats the .exe as
# an archive and extracts the embedded payload files.
#
# NSIS installers pack their real content inside a virtual directory called
# $PLUGINSDIR.  For Electron apps built with electron-builder, the important
# file in there is `app-64.7z` (or app-32.7z for 32-bit builds).  This inner
# archive contains the full Electron application directory — the Chromium
# runtime, Node.js, and the app's own resources.

echo "=== Step 1: Extracting NSIS installer ==="
7z x "$INSTALLER" -oextracted -y
echo
echo "Contents of \$PLUGINSDIR:"
ls -lh "extracted/\$PLUGINSDIR/"
echo

# ──────────────────────────────────────────────────────────────────────────────
# STEP 2 — Extract the inner Electron app archive
# ──────────────────────────────────────────────────────────────────────────────
# electron-builder compresses the Electron app into a secondary 7z archive
# (app-64.7z) for smaller download size.  Extracting it gives us a standard
# Electron directory layout:
#
#   app/
#   ├── KatasymbolEditor.exe      # Electron shell (renamed Chromium)
#   ├── resources/
#   │   ├── app.asar              # <-- the actual application code
#   │   └── ...
#   ├── locales/
#   └── ...
#
# The file we care about is resources/app.asar.

echo "=== Step 2: Extracting inner Electron archive (app-64.7z) ==="
7z x "extracted/\$PLUGINSDIR/app-64.7z" -oapp -y
echo
echo "Key file:"
ls -lh app/resources/app.asar
echo

# ──────────────────────────────────────────────────────────────────────────────
# STEP 3 — Unpack the Electron ASAR archive
# ──────────────────────────────────────────────────────────────────────────────
# ASAR (Atom Shell Archive) is Electron's custom archive format — essentially a
# tar-like concatenation of files with a JSON header that maps filenames to
# offsets and sizes.  It is NOT compressed; it exists so that Electron can
# memory-map the archive and read files directly from it at runtime without
# extracting to disk.
#
# The official @electron/asar npm package provides an `extract` command:
#
#   npx --yes @electron/asar extract <archive> <destination>
#
# After extraction we get the app's actual source files:
#
#   app-src/
#   ├── background.js             # Electron main process entry point
#   ├── index.html                # Renderer entry point
#   ├── js/
#   │   ├── app.77a7833b.js       # Minified/bundled Vue app (Webpack output)
#   │   ├── app.77a7833b.js.map   # <-- SOURCE MAP (this is the goldmine)
#   │   ├── chunk-vendors.*.js    # Third-party libraries bundle
#   │   └── chunk-vendors.*.js.map
#   ├── css/
#   └── ...

echo "=== Step 3: Unpacking ASAR archive ==="
npx --yes @electron/asar extract app/resources/app.asar app-src
echo
echo "Extracted app source tree:"
find app-src -maxdepth 2 -type f | sort
echo

# ──────────────────────────────────────────────────────────────────────────────
# STEP 4 — Recover original source files from the Webpack source map
# ──────────────────────────────────────────────────────────────────────────────
# Webpack (and most modern JS bundlers) can emit source maps alongside their
# minified output.  A source map is a JSON file that follows the Source Map
# Revision 3 specification.  The two fields we need are:
#
#   "sources":        An array of original file paths, e.g.
#                     ["webpack:///src/App.vue", "webpack:///src/main.js", ...]
#
#   "sourcesContent": A parallel array where each entry is the FULL original
#                     source text of the corresponding file.  When this field
#                     is present the source map is self-contained — we do not
#                     need access to the original project directory at all.
#
# The paths usually carry a "webpack:///" prefix which we strip.  We then
# recreate the original directory structure and write each file's content.
#
# This works because the developer left source maps enabled in the production
# build (the default for vue-cli-service unless explicitly disabled).  The
# .map file is ~5x larger than the minified JS, but it gives us a near-
# perfect reconstruction of the original src/ tree — Vue single-file
# components, JS modules, and all.

echo "=== Step 4: Extracting original sources from source map ==="

SOURCE_MAP="app-src/js/app.77a7833b.js.map"
OUTPUT_DIR="source-map-output"

node -e "
const fs = require('fs');
const path = require('path');

const mapFile = '$SOURCE_MAP';
const outDir  = '$OUTPUT_DIR';

const map = JSON.parse(fs.readFileSync(mapFile, 'utf8'));

console.log('Source map version:', map.version);
console.log('Total source entries:', map.sources.length);

let written = 0;
let skipped = 0;

map.sources.forEach((src, i) => {
  // Skip entries with no content (external/built-in modules)
  if (!map.sourcesContent[i]) {
    skipped++;
    return;
  }

  // Strip the webpack:/// URI prefix to get a real relative path.
  // Some entries may also start with './' or 'src/' — we keep those as-is.
  const cleaned = src.replace(/^webpack:\/\/\//, '');
  const outPath = path.join(outDir, cleaned);

  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, map.sourcesContent[i]);
  written++;
});

console.log('Files written:', written);
console.log('Entries skipped (no content):', skipped);
console.log('Output directory:', outDir);
"

echo
echo "=== Done ==="
echo
echo "Original source tree is now in: $BASEDIR/$OUTPUT_DIR"
echo
echo "Summary of what each extraction layer peeled away:"
echo "  .exe  (NSIS installer)    -> 7z x"
echo "  .7z   (electron-builder)  -> 7z x"
echo "  .asar (Electron archive)  -> @electron/asar extract"
echo "  .map  (Webpack source map) -> JSON parse + write files"
echo
echo "You can now browse the Vue/JS source under $OUTPUT_DIR/src/"
