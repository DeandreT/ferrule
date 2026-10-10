# OVHcloud CI runner

Ferrule's native CI and both generated-backend suites run on the repository's
`ferrule-ovh-quinta` runner when `OVH_RUNNER_ENABLED=true`. Pushes to `main`,
manual dispatches, and pull requests from branches in `DeandreT/ferrule` can
use OVH. Fork pull requests run on GitHub's `ubuntu-latest` machines. Existing
check names and qualification commands are preserved. Website deployment
continues through the separate Pages workflow.

## Host and isolation

The existing SSH alias `ovh` connects as `ubuntu` to the Ubuntu x86_64 host
`quinta`. Provisioning installs a dedicated `ferrule-runner` account, with no
supplemental groups, sudo privileges, Docker socket, or SSH credentials.
`ferrule-actions-runner.service` starts at boot and automatically restarts.
The service can write only its home and private temporary directory; other
application homes and game data are hidden. Runner application updates remain
enabled. Review write access to this public repository: contributors able to
create repository branches can execute jobs on this persistent runner.

A root-owned pre-job hook at `/opt/ferrule-runner-tools/guard-ovh-job.sh`
independently admits only Ferrule main pushes, repository-branch manual runs,
and same-repository pull requests. It rejects fork events even if their workflow
edits request the runner label directly. The service account cannot change
this hook. On rejection it aborts its parent job worker before checkout/action
steps, so even a workflow's `always()` steps cannot continue. An ordinary hook
exit is insufficient for that boundary: the runner's
[step condition handling](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Worker/StepsRunner.cs)
can continue after a failed step. Direct administrator/test invocations return
failure without signaling their caller.

Service startup checks archive creation/extraction and Xvfb under the actual
service restrictions. Ubuntu 26.04's tar needs `openat2`; the service leaves
`RestrictSUIDSGID` off to avoid its [documented syscall conflict](https://github.com/systemd/systemd/issues/43314).
`NoNewPrivileges` and an empty capability set prevent privilege escalation.

There is one runner slot, so native and codegen matrix jobs execute sequentially.
The service has a CPU quota of 150% (1.5 cores), memory high watermark of 3 GiB,
and hard limit of 3500 MiB. OVH jobs use one Cargo build job, two ordinary test
threads, and one generated-backend test thread. Graphical tests use Xvfb with
ambient Wayland handles removed. These limits leave capacity for the existing
Wareboxes runner and Valheim service; they do not reserve capacity from them.
The persistent Roslyn compiler process is disabled so C# builds release memory
before subsequent Rust host builds.
`TMPDIR=/home/ferrule-runner/ci-tmp` keeps compiler/test temporary files on disk.
Ubuntu 26.04's `/tmp` is a memory-backed filesystem: native GUI diagnostic
files reached 2.6 GiB and caused memory-pressure stalls in the first run.
Private `/tmp` remains enabled for programs that ignore `TMPDIR`.

`scripts/ci/prepare-ovh-build.sh` refuses an unexpected account, symlinked cache
paths, or less than 16 GiB free. The first native build used approximately
13 GiB, mostly workspace test executables. It checks Xvfb and native build prerequisites
before toolchain setup. Build outputs persist outside the cleaned checkout:

- `/home/ferrule-runner/ci-cache/workspace-target` — workspace builds. After a
  successful job, `cargo clean --workspace` removes workspace outputs while
  retaining third-party dependencies. Failed jobs retain their build artifacts
  for diagnosis. This prevents successive source/toolchain builds from retaining
  multiple complete sets of test executables on the shared disk.
- `/home/ferrule-runner/ci-cache/generated-host-target` — generated host builds
  that support `FERRULE_CODEGEN_HOST_TARGET_DIR`, separate from outer Cargo.
- `/home/ferrule-runner/ci-cache/compiler` — sccache, capped at 2 GB, reuses
  compatible Rust library compilation across otherwise isolated host builds.
- `/home/ferrule-runner/.cargo`, `.rustup`, and `.dotnet` — dependency/tool caches.
- `/home/ferrule-runner/ci-tmp` — temporary test/build outputs and retained
  failure diagnostics. Inspect these alongside caches; preserve required
  evidence before removing leftovers while the runner is idle.

GitHub-hosted jobs continue to use the existing Actions Rust cache. OVH skips
that archive cache to avoid duplicating persistent build outputs. Nightly Rust
and .NET 10 are maintained through the existing setup actions. No permanent
GitHub access token is installed on the server.
Only test/build steps use sccache; clippy runs directly. Incremental Rust
compilation stays disabled, as required by the compiler cache. Final runner
diagnostics report cache hits and misses. The cache tool is root-owned and its
download has a pinned SHA-256, following the [upstream Rust cache contract](https://github.com/mozilla/sccache/blob/main/docs/Rust.md).

## Provision or replace the runner

The administrator needs SSH sudo access and permission to administer repository
runners. The installer requires at least 8 GiB free, verifies the downloaded
runner archive against its pinned SHA-256, installs Ubuntu build prerequisites
and minimal nightly Rust, then registers the runner. Dependency installation
does not automatically restart unrelated services.

From a clean checkout with an authenticated GitHub CLI:

```sh
scp scripts/ci/provision-ovh-runner.sh ovh:/tmp/ferrule-provision-runner.sh
scp scripts/ci/guard-ovh-job.sh ovh:/tmp/guard-ovh-job.sh
gh api --method POST repos/DeandreT/ferrule/actions/runners/registration-token \
  --jq .token | ssh ovh 'sudo -n bash /tmp/ferrule-provision-runner.sh'
gh api repos/DeandreT/ferrule/actions/runners \
  --jq '.runners[] | {name, status, busy, labels}'
gh variable set OVH_RUNNER_ENABLED --body true --repo DeandreT/ferrule
```

Keep shell tracing disabled around the short-lived registration token. The
installer refuses an active Ferrule service; stop it only after the runner is
idle before reprovisioning. An existing registration is retained. If replacing
the machine, remove its old registration through GitHub's runner settings
first. Labels are `self-hosted`, `Linux`, `X64`, and `ferrule-ovh`. Builds require
16 GiB free after provisioning, even though installation itself requires less.

## Check and maintain

```sh
ssh ovh 'sudo systemctl status ferrule-actions-runner.service --no-pager'
ssh ovh 'sudo journalctl -u ferrule-actions-runner.service -n 100 --no-pager'
ssh ovh 'df -h /home; sudo du -h -d 1 /home/ferrule-runner/ci-cache'
gh workflow run ci.yml --repo DeandreT/ferrule
gh workflow run codegen.yml --repo DeandreT/ferrule
```

Watch the corresponding Actions jobs and confirm their runner name, conclusion,
and final disk/memory diagnostics. A memory-limit kill appears in the service
journal. Inspect cache size after substantial builds. The scripts never remove
other applications' artifacts. Cache maintenance must happen while this runner
is idle: stop its service, inspect the two Ferrule-owned target directories,
remove only generated outputs no longer needed, and restart the service.
Preserve failure logs or required qualification evidence before cleanup.

## Fall back to GitHub-hosted builds

```sh
gh variable set OVH_RUNNER_ENABLED --body false --repo DeandreT/ferrule
```

Newly scheduled jobs use GitHub-hosted runners. Existing queued or running
jobs keep their selected runner; cancel and rerun those if necessary. To retire
OVH entirely, wait for idle, disable the Ferrule service, and remove this runner
from repository settings. Wareboxes and Valheim services are independent.

The routing follows GitHub's [self-hosted runner label documentation](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/use-in-a-workflow).
Review GitHub's [runner security guidance](https://docs.github.com/en/actions/reference/security/secure-use)
before expanding trusted inputs or granting account privileges.
