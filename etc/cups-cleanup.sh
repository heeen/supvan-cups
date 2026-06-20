#!/bin/sh
# SIGKILL safety net (ExecStopPost): remove CUPS queues pointing at our IPP
# server. The app already cleans up its own queues on graceful exit and sweeps
# orphans on start; this only matters when the process is killed uncatchably.
PORT="${SUPVAN_PORT:-8631}"
lpstat -v 2>/dev/null \
    | grep "localhost:${PORT}/ipp/print/" \
    | sed 's/^device for //; s/:.*//' \
    | xargs -rn1 lpadmin -x
exit 0
