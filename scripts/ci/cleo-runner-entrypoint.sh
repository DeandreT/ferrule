#!/usr/bin/env bash
set -euo pipefail
runner_home=/home/ferrule-runner
runner_dir=$runner_home/actions-runner
install -d -o ferrule-runner -g ferrule-runner -m 700 "$runner_home" \
  "$runner_home/ci-tmp" "$runner_home/ci-cache" "$runner_home/.dotnet" "$runner_dir"
if [[ ! -f $runner_dir/config.sh ]]; then
  cp -a /opt/actions-runner/. "$runner_dir/"
  chown -R ferrule-runner:ferrule-runner "$runner_dir"
fi
if [[ ! -x $runner_home/.cargo/bin/rustup ]]; then
  curl -fsSL --retry 3 -o /tmp/rustup-init.sh https://sh.rustup.rs
  chmod 755 /tmp/rustup-init.sh
  runuser -u ferrule-runner -- env HOME="$runner_home" bash /tmp/rustup-init.sh \
    -y --profile minimal --default-toolchain nightly --component rustfmt --component clippy
  rm /tmp/rustup-init.sh
fi
# Registration happens separately with a short-lived token delivered over stdin.
touch /tmp/runner-ready
while [[ ! -f $runner_dir/.runner ]]; do sleep 2; done
cd "$runner_dir"
# The official service supervisor forwards stop signals and removes the server
# session before exit. Direct run.sh startup can leave a stale session on restart.
exec setpriv --reuid=ferrule-runner --regid=ferrule-runner --init-groups \
  ./externals/node20/bin/node ./bin/RunnerService.js
