#!/usr/bin/env bash
# Xentrium health probe. Exits non-zero (restarting the service via the
# healthcheck unit's OnFailure-less design: the timer's service restarts
# xentrium directly when this exits 1).
#
# Checks:
#   1. process alive
#   2. TCP listener bound on the configured port
#   3. log file written to within the last 3 minutes
#
# Configure via environment or edit defaults:
XENTRIUM_PORT="${XENTRIUM_PORT:-443}"
XENTRIUM_LOG="${XENTRIUM_LOG:-/root/xentrium/xentrium-core/data/node1/xentrium.log}"
XENTRIUM_SERVICE="${XENTRIUM_SERVICE:-xentrium.service}"

fail() {
    echo "HEALTH FAIL: $1" >&2
    systemctl restart "$XENTRIUM_SERVICE"
    exit 1
}

# 1. process
pgrep -f 'xentrium --config' >/dev/null || fail "no xentrium process"

# 2. listener
ss -lnt | grep -q ":${XENTRIUM_PORT} " || fail "nothing listening on :${XENTRIUM_PORT}"

# 3. log freshness (node logs heartbeat/gossip/maintenance every <=60s)
[ -f "$XENTRIUM_LOG" ] || fail "log file missing: $XENTRIUM_LOG"
find "$XENTRIUM_LOG" -mmin -3 | grep -q . || fail "log stale (>3min without writes)"

echo "HEALTH OK: process alive, :${XENTRIUM_PORT} listening, log fresh"
exit 0
