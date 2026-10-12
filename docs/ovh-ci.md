# Ferrule CI runners: Cleo and Over

Native CI and generated-backend suites default to `auto`: the two Cleo workers
are preferred, with `ferrule-over` automatically eligible when Cleo has no idle slot.
Set repository variable `CI_RUNNER` to `auto`, `cleo`, `over`, or `github-hosted`.
Manual dispatch overrides the variable for one run. Explicit host selections pin
the host; forks always use `ubuntu-latest`. Pages keeps its separate hosted workflow.

Vecta's user timer polls GitHub's complete runner inventory every 30 seconds.
After two separated complete observations where every online Cleo worker is
busy, or both `ferrule-cleo` and `ferrule-cleo-2` are offline, the controller adds
`ferrule-primary` and the legacy `ferrule-ovh` labels to `ferrule-over`. An offline
worker does not count as an available slot. When either Cleo worker is online
and idle, the next poll removes only those labels from Over. This enables
overflow after roughly 30–60 seconds of occupied capacity; GitHub chooses among
eligible idle runners. Existing running jobs are never cancelled or restarted,
and already queued automatic/legacy jobs can use the backup during saturation
or an outage.
The explicit `ferrule-over` label is always retained.

The same controller maintains `wareboxes-primary`/`wareboxes-ci` on
`wareboxes-over` when `wareboxes-cleo` is busy or offline, with the same confirmation
and idle recovery rules. It changes labels only on the known backup registrations.
Missing/ambiguous inventories, including absent or nonboolean busy flags, refuse
decisions. API failures preserve the last saved state, although partial label
updates may already have happened. Vecta must remain running with network access and its existing
GitHub CLI authentication; credentials stay outside the CI containers and
workflow secrets. Install on Vecta from this checkout:

```sh
bash scripts/ci/install-runner-failover.sh
systemctl --user status ci-runner-failover.timer --no-pager
journalctl --user -u ci-runner-failover.service -n 50 --no-pager
```

Stop the timer before making scheduling-label changes for maintenance.

## Cleo's persistent hosting

Connect with `ssh cleo` as `deck`. Cleo runs SteamOS, so provisioning uses
rootless Podman without changing its read-only system partition. Files and
runner data live under `/home/deck/services/ferrule`; Podman image storage is
already on the persistent home partition. The runner's home inside the container
is `/home/ferrule-runner`, so both hosts use the same cache and space admission
helpers. Historical `*-ovh-*` helper filenames remain compatible with the
existing deployed pre-job hook; they apply to both runners.

From a clean checkout with an authenticated GitHub CLI:

```sh
ssh cleo 'mkdir -p /home/deck/services/ferrule/provision'
scp scripts/ci/cleo-runner.Containerfile scripts/ci/cleo-runner-entrypoint.sh \
  scripts/ci/provision-cleo-runner.sh scripts/ci/guard-ovh-job.sh \
  scripts/ci/prepare-ovh-job.py scripts/ci/compact-job-evidence.py \
  scripts/ci/reclaim-build-cache.py scripts/ci/ferrule-ci.slice cleo:/home/deck/services/ferrule/provision/
gh api --method POST repos/DeandreT/ferrule/actions/runners/registration-token \
  --jq .token | ssh cleo 'bash /home/deck/services/ferrule/provision/provision-cleo-runner.sh'
```

The installer requires 32 GiB free. A private container runs as `ferrule-runner`,
without sudo, a Docker socket, host credentials, or desktop display sockets.
Xvfb supplies isolated graphical displays. Each Ferrule worker permits two CPU
threads and 5 GiB memory, with one Cargo build at a time. Both services belong to
`ferrule-ci.slice`, which caps their combined CPU at four threads, starts memory
reclaim at 6 GiB, caps resident memory at 7 GiB and swap at 3 GiB. This leaves
physical memory for Wareboxes' independent 6 GiB container and the host.

Slot 1 uses `services/ferrule/home`; slot 2 uses `services/ferrule/slots/2/home`.
Their checkouts, Cargo targets, compiler caches, SDKs and retained evidence are
independent. The image is reused; no Cargo target tree is copied or shared.
The second registration is named `ferrule-cleo-2`, with the same primary labels.
After slot 1 belongs to the bounded slice, provision slot 2 with:

```sh
gh api --method POST repos/DeandreT/ferrule/actions/runners/registration-token \
  --jq .token | ssh cleo 'bash /home/deck/services/ferrule/provision/provision-cleo-runner.sh 2'
```

For an existing first service, drain only its scheduling labels, wait for GitHub
idle and no `Runner.Worker` process, stop it, install `ferrule-ci.slice` in
`~/.config/systemd/user`, add `Slice=ferrule-ci.slice` to its Quadlet `[Service]`,
reload/start the service and restore its labels. Do not restart an active worker.
The second-slot installer refuses an unbounded first service.

Quadlet configuration lives at
`/home/deck/.config/containers/systemd/ferrule-cleo.container`. Its `[Install]`
section and the existing user lingering setting start it on later boots;
generated Quadlet services do not use `systemctl enable`. The definition avoids
SteamOS's inactive network-online target and retries failures. Manage it with:

```sh
ssh cleo 'systemctl --user status ferrule-cleo.service --no-pager'
ssh cleo 'journalctl --user -u ferrule-cleo.service -n 100 --no-pager'
ssh cleo 'df -h /home; podman exec ferrule-cleo du -h -d 1 /home/ferrule-runner/ci-cache'
```

Wait for the runner to become idle before stopping or replacing it. Container
replacement preserves `services/ferrule/home`, including registration, caches,
and retained diagnostics. No actual reboot or SteamOS update is implied by a
service startup/restart check.

## Over: host and isolation

The existing SSH alias `over` (with `ovh` retained as a compatibility alias) connects as `ubuntu` to the Ubuntu x86_64 host
`quinta`. Provisioning installs a dedicated `ferrule-runner` account, with no
supplemental groups, sudo privileges, Docker socket, or SSH credentials. Its
login shell is disabled; the service runs its commands directly.
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

After event admission, the same root-owned hook runs `prepare-ovh-job.py`
before checkout for every job, including branches with older workflow scripts.
It keeps the native 16 GiB and codegen 8 GiB floors and aims for another 4 GiB
of headroom. With enough space it preserves all build caches and test directories.
Under pressure it stops the idle sccache daemon and requires that the dedicated
account have no processes other than the current hook and its runner ancestors.
A surviving earlier job process refuses maintenance, even if it currently has
no files open. This account-wide producer check also runs immediately before
each original test directory is removed.

Completed Ferrule test directories of at least 16 MiB are compressed, largest
first, into `/home/ferrule-runner/ci-evidence`. Each archive retains every file
body, empty directory, symbolic link, permissions, owner and modification time.
A companion manifest retains exact nanosecond modification times and SHA-256
hashes. Every member is read back and checked, and the original tree is checked
again before removing its uncompressed representation. Archives and manifests
are flushed to disk first. An interrupted cleanup leaves the complete archive
available. Nothing expires automatically. These are local copies with the same
host durability as the originals; they do not establish external preservation
or enable the separate evidence-retirement policy in #360.

This compactor covers only the legacy flat test directories under `ci-tmp`.
The newer per-job evidence roots created by `job-evidence.py` remain governed
by their own provenance and retention checks. Their workflow observation and
optional metadata-canary steps stay in place; this hook does not retire them.

If compaction is insufficient, the hook reclaims only idle, reproducible Cargo
profile caches. It refuses the job if the existing floor still cannot be met.
Redirected paths, mounted trees, foreign ownership, writable shared directories,
special files and external hard links refuse evidence compaction.

Service startup checks archive creation/extraction and Xvfb under the actual
service restrictions. Ubuntu 26.04's tar needs `openat2`; the service leaves
`RestrictSUIDSGID` off to avoid its [documented syscall conflict](https://github.com/systemd/systemd/issues/43314).
`NoNewPrivileges` and an empty capability set prevent privilege escalation.

There is one runner slot, so native and codegen matrix jobs execute sequentially.
The service has a CPU quota of 150% (1.5 cores), memory high watermark of 3 GiB,
and hard limit of 3500 MiB. Over jobs use one Cargo build job, two ordinary test
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
paths, or less than 16 GiB free for native CI and 8 GiB for codegen CI.
Before each build it seeks four additional GiB of headroom. When space is low,
`reclaim-build-cache.py` acquires exclusive Cargo profile locks and checks for
live cache users before reclaiming reproducible outputs, first from the generated
host target and then from the workspace target. It preserves profile lock inodes,
retained job evidence, compiler/download caches and installed toolchains. Redirected
paths, unsafe profile entries, unreadable live processes or busy caches refuse
reclamation; insufficient capacity still refuses the build. This policy assumes
the dedicated runner has one job slot and Cargo builds use the configured targets.
The first native build used approximately 13 GiB, mostly workspace test
executables. It checks Xvfb and native build prerequisites before toolchain setup.

The XML process-custody tests receive the resolved GNU timeout path through
`FERRULE_CODEGEN_GNU_TIMEOUT`. Ubuntu 26.04 provides `/usr/bin/gnutimeout`
alongside its default uutils symlink. This keeps the actual executable's identity
checks and bounded process commands consistent without changing system defaults.

Build outputs persist outside the cleaned checkout:

- `/home/ferrule-runner/ci-cache/workspace-target` — workspace builds. After a
  successful job, `cargo clean --workspace` removes workspace outputs while
  retaining third-party dependencies. Build artifacts are reproducible caches; later space admission can reclaim them.
  Retained failure diagnostics belong in job temporary/evidence directories. This prevents successive source/toolchain builds from retaining
  multiple complete sets of test executables on the shared disk.
- `/home/ferrule-runner/ci-cache/generated-host-target` — generated host builds
  that support `FERRULE_CODEGEN_HOST_TARGET_DIR`, separate from outer Cargo.
- `/home/ferrule-runner/ci-cache/compiler` — sccache, capped at 2 GB, reuses
  compatible Rust library compilation within stable build paths. Isolated host
  projects with different working directories still rebuild: the upstream
  [Rust cache key includes the working directory](https://github.com/mozilla/sccache/blob/v0.18.0/src/compiler/rust.rs#L1524).
- `/home/ferrule-runner/.cargo`, `.rustup`, and `.dotnet` — dependency/tool caches.
- `/home/ferrule-runner/ci-tmp` — temporary test/build outputs and retained
  failure diagnostics. Large completed directories can move into verified local
  archives before later jobs when space is low; small directories remain here.
- `/home/ferrule-runner/ci-evidence` — compressed complete test directories and
  their verified member manifests. Keep these alongside remaining `ci-tmp`
  records when inspecting historical failures. No automatic archive expiration.

GitHub-hosted jobs continue to use the existing Actions Rust cache. Self-hosted CI skips
that archive cache to avoid duplicating persistent build outputs. Nightly Rust
and .NET 10 are maintained through the existing setup actions. No GitHub
personal access token is installed on the server.
Only test/build steps use sccache; clippy runs directly. Incremental Rust
compilation stays disabled, as required by the compiler cache. Final runner
diagnostics report cache hits and misses. The cache tool is root-owned and its
download has a pinned SHA-256, following the [upstream Rust cache contract](https://github.com/mozilla/sccache/blob/main/docs/Rust.md).

## Provision or replace Over

The administrator needs SSH sudo access and permission to administer repository
runners. The installer requires at least 8 GiB free, verifies the downloaded
runner archive against its pinned SHA-256, installs Ubuntu build prerequisites
and minimal nightly Rust, then registers the runner. Archive extraction uses
the CI account's permissions; root does not extract into its writable tree.
Dependency installation does not automatically restart unrelated services.

From a clean checkout with an authenticated GitHub CLI:

```sh
scp scripts/ci/provision-ovh-runner.sh over:/tmp/ferrule-provision-runner.sh
scp scripts/ci/guard-ovh-job.sh over:/tmp/guard-ovh-job.sh
scp scripts/ci/prepare-ovh-job.py scripts/ci/compact-job-evidence.py \
  scripts/ci/reclaim-build-cache.py over:/tmp/
gh api --method POST repos/DeandreT/ferrule/actions/runners/registration-token \
  --jq .token | ssh over 'sudo -n bash /tmp/ferrule-provision-runner.sh'
gh api repos/DeandreT/ferrule/actions/runners \
  --jq '.runners[] | {name, status, busy, labels}'
gh variable set CI_RUNNER --body over --repo DeandreT/ferrule
```

Keep shell tracing disabled around the short-lived registration token. The
installer refuses an active Ferrule service; stop it only after the runner is
idle before reprovisioning. An existing registration is retained. If replacing
the machine, remove its old registration through GitHub's runner settings
first. Labels are `self-hosted`, `Linux`, `X64`, and `ferrule-over`. Builds require
16 GiB free for native CI and 8 GiB for codegen after provisioning, even though
installation itself requires less.

## Check and maintain

```sh
ssh over 'sudo systemctl status ferrule-actions-runner.service --no-pager'
ssh over 'sudo journalctl -u ferrule-actions-runner.service -n 100 --no-pager'
ssh over 'df -h /home; sudo du -h -d 1 /home/ferrule-runner/ci-cache'
gh workflow run ci.yml --repo DeandreT/ferrule -f runner=over
gh workflow run codegen.yml --repo DeandreT/ferrule -f runner=over
```

Watch the corresponding Actions jobs and confirm their runner name, conclusion,
and final disk/memory diagnostics. A memory-limit kill appears in the service
journal. Inspect cache size after substantial builds. The scripts never remove
other applications' artifacts. Cache maintenance must happen while this runner
is idle: stop its service, inspect the two Ferrule-owned target directories,
remove only generated outputs no longer needed, and restart the service.
Preserve failure logs or required qualification evidence before cleanup.

To inspect an archived test directory, locate its original directory name in
the `ci-evidence/*.json` manifests and list or stream files without extraction:

```sh
tar -tzf /home/ferrule-runner/ci-evidence/<archive>.tar.gz
tar -xOzf /home/ferrule-runner/ci-evidence/<archive>.tar.gz <member-path>
```

Restore a complete archive only into a fresh, private directory while the runner
is idle. The manifest records the original test directory name and every member;
its `archive_sha256` verifies the compressed file itself. Preserve both the
archive and its manifest. Manual archive removal is outside the automatic policy.

## Fall back to GitHub-hosted builds

```sh
gh variable set CI_RUNNER --body github-hosted --repo DeandreT/ferrule
```

Newly scheduled jobs use GitHub-hosted runners. Existing queued or running
jobs keep their selected runner; cancel and rerun those if necessary. To retire
Over entirely, wait for idle, disable the Ferrule service, and remove this runner
from repository settings. Wareboxes and Valheim services are independent.

The routing follows GitHub's [self-hosted runner label documentation](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/use-in-a-workflow).
Review GitHub's [runner security guidance](https://docs.github.com/en/actions/reference/security/secure-use)
before expanding trusted inputs or granting account privileges.

To restore Cleo preference with automatic fallback, set `CI_RUNNER=auto`.
During migration, `OVH_RUNNER_ENABLED=true` routes older workflow revisions to
`ferrule-ovh`: normally the Cleo workers, with Over eligible during confirmed
Cleo saturation or an outage. Over retains `ferrule-over` for explicit selection. The old variable
has no effect on the updated selectors.
