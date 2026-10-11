#!/usr/bin/env bash
# Run as Cleo's ordinary SSH account. Supply a registration token on stdin.
set +x
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if (( EUID == 0 )) || [[ $(uname -m) != x86_64 ]] ||
   [[ $(podman info --format '{{.Host.Security.Rootless}}') != true ]]; then
  echo 'Cleo provisioning requires rootless Podman on Linux x64.' >&2
  exit 1
fi
if podman container exists ferrule-cleo; then
  echo 'The existing Cleo runner must be idle and explicitly stopped before reprovisioning.' >&2
  exit 1
fi
available_kib=$(df -Pk "$HOME" | awk 'NR == 2 { print $4 }')
if (( available_kib < 32 * 1024 * 1024 )); then
  echo 'Cleo provisioning needs 32 GiB free; existing artifacts are preserved.' >&2
  exit 1
fi
IFS= read -r registration_token
test -n "$registration_token"
podman build --jobs 1 -f "$script_dir/cleo-runner.Containerfile" \
  -t localhost/ferrule-cleo:latest "$script_dir"
service_home=$HOME/services/ferrule
mkdir -p "$service_home/home"
unit_dir=$HOME/.config/containers/systemd
mkdir -p "$unit_dir"
cat > "$unit_dir/ferrule-cleo.container" <<UNIT
[Unit]
Description=Ferrule CI runner on Cleo
[Container]
Image=localhost/ferrule-cleo:latest
ContainerName=ferrule-cleo
Volume=$service_home/home:/home/ferrule-runner
PodmanArgs=--init --cpus 2 --memory 5g --pids-limit 1024
[Quadlet]
DefaultDependencies=false
[Service]
Restart=always
RestartSec=10
TimeoutStopSec=320
[Install]
WantedBy=default.target
UNIT
systemctl --user daemon-reload
systemctl --user start ferrule-cleo.service
ready=false
for (( attempt = 0; attempt < 600; attempt++ )); do
  if podman exec ferrule-cleo test -f /tmp/runner-ready 2>/dev/null; then
    ready=true; break
  fi
  sleep 2
done
[[ $ready == true ]] || { echo 'Inspect the Cleo runner service startup log.' >&2; exit 1; }
printf '%s\n' "$registration_token" | podman exec -i --user ferrule-runner ferrule-cleo \
  bash -c 'set +x; set -e; IFS= read -r token; cd /home/ferrule-runner/actions-runner; ./config.sh --unattended --url https://github.com/DeandreT/ferrule --token "$token" --name ferrule-cleo --labels ferrule-cleo --work _work'
unset registration_token
loginctl show-user "$(id -un)" -p Linger
