#!/usr/bin/env bash
# Run as root on the OVH host. Supply a short-lived registration token on stdin.
set +x
set -euo pipefail

runner_version=2.337.0
runner_sha256=70920811a4f8ad4328818682bca5c6469c1c942fab52448868071d0063816613
runner_user=ferrule-runner
runner_home=/home/ferrule-runner
runner_dir="$runner_home/actions-runner"
service=ferrule-actions-runner.service

if (( EUID != 0 )) || [[ $(uname -m) != x86_64 ]]; then
  echo 'Provisioning requires root on an x86_64 Ubuntu host.' >&2
  exit 1
fi
# shellcheck source=/dev/null
source /etc/os-release
if [[ $ID != ubuntu ]]; then
  echo 'This provisioning script targets Ubuntu.' >&2
  exit 1
fi
if systemctl is-active --quiet "$service"; then
  echo 'Ferrule runner is already active; stop it before reprovisioning.' >&2
  exit 1
fi
available_kib=$(df -Pk /home | awk 'NR == 2 { print $4 }')
if [[ ! $available_kib =~ ^[0-9]+$ ]] || (( available_kib < 8 * 1024 * 1024 )); then
  echo 'Provisioning requires at least 8 GiB free; no existing artifacts are removed.' >&2
  exit 1
fi
if [[ ! -f $runner_dir/.runner ]]; then
  if ! IFS= read -r registration_token || [[ -z $registration_token ]]; then
    echo 'A short-lived GitHub runner registration token is required on stdin.' >&2
    exit 1
  fi
fi

if ! id "$runner_user" >/dev/null 2>&1; then
  useradd --create-home --user-group --shell /bin/bash "$runner_user"
fi
if [[ $(getent passwd "$runner_user" | cut -d: -f6) != "$runner_home" ||
      $(id -Gn "$runner_user") != "$runner_user" || -L $runner_home ]]; then
  echo 'Refusing an account with an unexpected home or supplemental groups.' >&2
  exit 1
fi
chmod 700 "$runner_home"

apt-get update
NEEDRESTART_MODE=l DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  build-essential pkg-config git curl ca-certificates unzip python3 \
  libssl-dev libx11-dev libxi-dev libxrandr-dev libxcursor-dev \
  libgl1-mesa-dev libegl1-mesa-dev libwayland-dev libxkbcommon-dev \
  libicu-dev xvfb xauth

install -d -o "$runner_user" -g "$runner_user" -m 700 \
  "$runner_dir" "$runner_home/ci-cache" "$runner_home/.dotnet"
if [[ ! -f $runner_dir/config.sh ]]; then
  archive=$(mktemp /tmp/ferrule-runner.XXXXXX.tar.gz)
  trap 'rm -f "$archive"' EXIT
  curl --fail --location --retry 3 --output "$archive" \
    "https://github.com/actions/runner/releases/download/v${runner_version}/actions-runner-linux-x64-${runner_version}.tar.gz"
  printf '%s  %s\n' "$runner_sha256" "$archive" | sha256sum --check --status
  tar -xzf "$archive" -C "$runner_dir" --no-same-owner
  chown -R "$runner_user:$runner_user" "$runner_dir"
fi

if [[ ! -f $runner_home/.cargo/bin/rustup ]]; then
  rustup_installer=$(mktemp /tmp/ferrule-rustup.XXXXXX.sh)
  curl --fail --location --retry 3 --output "$rustup_installer" https://sh.rustup.rs
  chmod 755 "$rustup_installer"
  runuser -u "$runner_user" -- env HOME="$runner_home" \
    bash "$rustup_installer" -y --profile minimal --default-toolchain nightly \
    --component rustfmt --component clippy
  rm -f "$rustup_installer"
fi

if [[ ! -f $runner_dir/.runner ]]; then
  (
    cd "$runner_dir"
    runuser -u "$runner_user" -- env HOME="$runner_home" \
      PATH="$runner_home/.cargo/bin:/usr/local/bin:/usr/bin:/bin" \
      ./config.sh --unattended --url https://github.com/DeandreT/ferrule \
      --token "$registration_token" --name "ferrule-ovh-$(hostname -s)" \
      --labels ferrule-ovh --work _work
  )
  unset registration_token
fi
install -o "$runner_user" -g "$runner_user" -m 755 \
  "$runner_dir/bin/runsvc.sh" "$runner_dir/runsvc.sh"
printf '%s\n' "$service" > "$runner_dir/.service"
chown "$runner_user:$runner_user" "$runner_dir/.service"

cat > "/etc/systemd/system/$service" <<'UNIT'
[Unit]
Description=Ferrule GitHub Actions runner on OVHcloud
After=network-online.target
Wants=network-online.target

[Service]
User=ferrule-runner
Group=ferrule-runner
WorkingDirectory=/home/ferrule-runner/actions-runner
ExecStart=/home/ferrule-runner/actions-runner/runsvc.sh
Environment=HOME=/home/ferrule-runner
Environment=PATH=/home/ferrule-runner/.cargo/bin:/home/ferrule-runner/.dotnet:/usr/local/bin:/usr/bin:/bin
Environment=DOTNET_INSTALL_DIR=/home/ferrule-runner/.dotnet
Environment=DOTNET_CLI_TELEMETRY_OPTOUT=1
Environment=DOTNET_NOLOGO=1
Environment=MSBUILDDISABLENODEREUSE=1
Restart=always
RestartSec=10
KillMode=mixed
TimeoutStopSec=300
CPUQuota=150%
CPUWeight=50
MemoryHigh=3G
MemoryMax=3500M
TasksMax=512
UMask=0077
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ReadWritePaths=/home/ferrule-runner
InaccessiblePaths=-/home/ubuntu -/home/wareboxes-runner -/opt/valheim -/opt/steamcmd -/var/lib/valheim
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable --now "$service"
# Catch immediate launcher failures before reporting a successful installation.
sleep 2
systemctl is-active "$service"
df -h "$runner_home"
