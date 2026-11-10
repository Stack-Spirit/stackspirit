#!/bin/sh
# Run one command inside a transient systemd user scope when the node supports it.
#
# The scope is what gives the measurement an exact cgroup.  When user scopes are
# not available (macOS, a container without a user manager, or CAP_SCOPE=0) the
# command runs directly and job_measure.py records that cgroup accounting was
# unavailable and falls back to /proc.
#
# Usage:
#   run_one.sh python3 job_measure.py --label ...
set -eu

if [ "${CAP_SCOPE:-auto}" != "0" ] && command -v systemd-run >/dev/null 2>&1 \
    && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    label=${CAP_SCOPE_LABEL:-job}
    # Unit names are limited; keep them recognisable and unique.
    unit=$(printf 'vowp-cap-%s-%s' "$label" "$$" | tr -cd 'A-Za-z0-9_.-' | cut -c1-120)
    exec systemd-run \
        --user \
        --scope \
        --quiet \
        --collect \
        --unit="$unit" \
        env CAP_SCOPE=1 CAP_SCOPE_UNIT="$unit" "$@"
fi

exec env CAP_SCOPE=0 "$@"
