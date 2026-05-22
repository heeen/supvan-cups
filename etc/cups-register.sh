#!/bin/sh
# Wait for supvan-printer-app, then register its printers with CUPS.
sleep 2
name=$(curl -sf http://localhost:8631/ | sed -n 's/.*<b>\([^<]*\)<\/b>.*/\1/p' | head -1)
if [ -n "$name" ]; then
    lpadmin -p "$name" -E -v "ipp://localhost:8631/ipp/print/$name" -m everywhere 2>/dev/null
fi
exit 0
