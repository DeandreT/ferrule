#!/usr/bin/env bash
# Run on Vecta; GitHub credentials remain in its existing gh credential store.
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
controller_dir=$HOME/.local/lib/ci-runner-failover
unit_dir=$HOME/.config/systemd/user
mkdir -p "$controller_dir" "$unit_dir"
install -m 0755 "$script_dir/runner-failover.py" "$controller_dir/runner-failover.py"
cat > "$unit_dir/ci-runner-failover.service" <<UNIT
[Unit]
Description=Prefer Cleo CI workers and enable Over during confirmed outages
[Service]
Type=oneshot
ExecStart=/usr/bin/python3 -I $controller_dir/runner-failover.py --state-dir $HOME/.local/state/ci-runner-failover
TimeoutStartSec=240
UMask=0077
UNIT
cat > "$unit_dir/ci-runner-failover.timer" <<UNIT
[Unit]
Description=Check CI runner availability every 30 seconds
[Timer]
OnBootSec=30
OnUnitInactiveSec=30
AccuracySec=1
[Install]
WantedBy=timers.target
UNIT
systemctl --user daemon-reload
systemctl --user enable --now ci-runner-failover.timer
