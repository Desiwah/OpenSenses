#!/bin/sh
# Only on a real removal, not an upgrade (deb passes "remove"/"purge",
# rpm passes 0).
case "$1" in
    remove|purge|0)
        SVC=$(ls /usr/lib/*/opensenses-service 2>/dev/null | head -n 1)
        # Puts the laptop's settings back first, then removes the service.
        [ -n "$SVC" ] && "$SVC" uninstall || true
        dkms remove -m opensenses -v 0.1.0 --all >/dev/null 2>&1 || true
        rm -rf /usr/src/opensenses-0.1.0 /usr/local/lib/opensenses
        rmmod opensenses >/dev/null 2>&1 || true
        ;;
esac
exit 0
