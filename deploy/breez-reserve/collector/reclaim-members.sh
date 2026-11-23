#!/usr/bin/env bash
# Only data needed to attest the failed allocation crosses service identities.
set -euo pipefail
base=/opt/lightning-goats-member-reclaim
stage() {
    local user=$1
    shift
    /usr/sbin/runuser -u "$user" -- /usr/bin/python3 -IB "$base/member_reclaim.py" "$@"
}
stage cyberherd notices |
    stage cyberherd-payout plan |
    stage lightning-goats authorize |
    stage cyberherd-payout seal |
    /usr/sbin/runuser -u goats-project-collector -- env \
      PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 \
      PYTHONPATH=/opt/lightning-goats-project-collector/modules:/opt/lightning-goats-project-collector/collector \
      /opt/lightning-goats-project-collector/python/bin/python3.11 "$base/member_reclaim.py" issue
