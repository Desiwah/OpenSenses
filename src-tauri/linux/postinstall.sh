#!/bin/sh
# Sets up the OpenSenses background service and builds its kernel driver
# with DKMS. Never fails the package install: the app explains what's
# missing if the driver couldn't be built.
SVC=$(ls /usr/lib/*/opensenses-service 2>/dev/null | head -n 1)
if [ -n "$SVC" ] && [ -x "$SVC" ]; then
    "$SVC" install || true
fi
exit 0
