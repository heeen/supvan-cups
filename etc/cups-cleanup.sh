#!/bin/sh
# Remove CUPS queues pointing to our PAPPL instance.
lpstat -v 2>/dev/null | grep localhost:8631 | cut -d: -f1 | sed 's/device for //' | xargs -rn1 lpadmin -x
exit 0
