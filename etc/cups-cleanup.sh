#!/bin/sh
# Remove the CUPS queue(s) pointing at our IPP server. The queue is persistent
# (kept across restarts so its printer-uuid stays stable for cups-browsed
# dedup), so this is NOT run on every stop — only on uninstall (`make
# uninstall*`) or by hand to tear the queue down.
PORT="${SUPVAN_PORT:-8631}"
lpstat -v 2>/dev/null \
    | grep "localhost:${PORT}/ipp/print/" \
    | sed 's/^device for //; s/:.*//' \
    | xargs -rn1 lpadmin -x
exit 0
