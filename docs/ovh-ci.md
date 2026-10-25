# Ferrule CI runners: Cleo and Over

Dedicated routing requires `OVH_RUNNER_ENABLED=true`. With that gate unset or
false, every job stays on `ubuntu-latest`, including explicit host choices.
Merging or installing this source does not enable the gate. When enabled,
`CI_RUNNER` selects `auto` (also the empty default), `cleo`, `over`, or
`github-hosted`. A manual dispatch's `runner` input overrides `CI_RUNNER`.
Unknown choices fail closed to hosted. Fork pull requests always stay hosted.
Existing check names, qualification commands, resource admission and evidence
retention remain unchanged; Pages keeps its separate workflow.

`auto` uses `ferrule-primary`; explicit choices use `ferrule-cleo` or
`ferrule-over`. Both Cleo workers have the primary and explicit Cleo labels.
The controller adds primary and legacy `ferrule-ovh` labels only to the known
Over registration, never removing its explicit or unrelated labels.

## Automatic overflow controller

`runner-failover.py` reads complete GitHub runner inventories. After two
observations separated by 20–120 seconds with no online idle Cleo worker, it
adds Over's automatic/legacy labels if Over is online. Both fully busy pools
and mixed busy/offline pools qualify. One online idle Cleo worker removes those
labels immediately on the next poll. Missing workers, ambiguous identities,
nonboolean busy flags and incomplete inventories refuse reconciliation.
Running jobs are never stopped, restarted or cancelled. Existing queued jobs
retain their selected labels and may use Over once it becomes eligible.

The controller uses the host's existing GitHub CLI authentication; credentials
must remain outside runner containers and workflow secrets. It defaults to
`DeandreT/ferrule`. The separately supported `wareboxes/wareboxes` pool requires
an explicit `--repository` argument; managing another repository requires its
own authorization. Repeat that option to manage both pools. API failures leave
the previous state intact; a partially applied label update is reported as
incomplete, not as unchanged. A lock prevents overlapping controller runs.

These commands are administrator-operated deployment steps, not prerequisites
for merging source and not evidence of live qualification:

```sh
python3 -B scripts/ci/test-runner-failover.py
python3 -B scripts/ci/test-runner-routing.py
python3 -B scripts/ci/test-runner-provisioning.py
# On Vecta, inspect proposals without changing labels or saved state:
python3 scripts/ci/runner-failover.py --state-dir "$HOME/.local/state/ci-runner-failover" --dry-run
# Only after separate host approval, install the Ferrule-only 30-second timer:
bash scripts/ci/install-runner-failover.sh
```

Source tests do not establish host readiness. Before enabling dedicated routing,
verify installed helper versions, both Cleo registrations, Over's exact name and
labels, the controller's API permissions, complete inventories, idle recovery,
busy/outage confirmation, service persistence and fresh native/codegen builds.
Do not manufacture outages or interrupt active jobs to qualify these controls.
Renaming an existing Over registration is a separate idle-time administrator
operation; the provisioning script retains existing registrations unchanged.

## Cleo persistent workers

Cleo uses rootless Podman on Linux x64, with homes under
`$HOME/services/ferrule` rather than SteamOS's read-only system partition.
`provision-cleo-runner.sh 1` creates `ferrule-cleo`; argument `2` creates
`ferrule-cleo-2`. Each worker has a separate home, checkout, writable caches,
SDKs and evidence; no Cargo target is shared. The second slot requires the
first service already in `ferrule-ci.slice`. The installer refuses an existing
container and requires 32 GiB free. Never replace or stop an active worker.

Each container allows two CPU threads and 5 GiB memory. The shared slice caps
CPU at four threads, memory at 7 GiB (reclaim starts at 6 GiB), and swap at
3 GiB. Containers include the current trusted pre-job helpers, including
`job-evidence.py`; the 16/8 GiB reserve and modern per-job compaction policies
are the same as Over's. Xvfb provides isolated graphical testing without host
display sockets, credentials or Docker sockets.

For an approved installation, copy `scripts/ci` to Cleo, then deliver a
short-lived registration token on stdin to `provision-cleo-runner.sh 1` or `2`.
Do not put the token in command arguments, files or logs. The resulting Quadlet
units live in `~/.config/containers/systemd`; persistent homes are
`services/ferrule/home` and `services/ferrule/slots/2/home`. User service startup
across boots requires separately verified lingering. A service restart does not
prove reboot or SteamOS-update qualification.

## Over host and isolation

The existing SSH alias `ovh` connects as `ubuntu` to the Ubuntu x86_64 host
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

The compactor covers legacy flat test directories under `ci-tmp` and admitted
per-job roots under `ci-tmp/jobs`. A per-job root must retain its immutable
`UNARCHIVED.json` preparation receipt with matching repository, run, attempt,
job, suite, nonce and directory identity. Both nonce and existing descriptive
job IDs are supported. Unknown, unprepared or changed roots stay in place.
The complete job tree and original receipts enter the verified archive; its
manifest records the original `jobs/<job-id>` location. Workflow observation
and optional metadata-canary steps remain separate from the compactor's
producer-closure and archive checks.

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

GitHub-hosted jobs continue to use the existing Actions Rust cache. OVH skips
that archive cache to avoid duplicating persistent build outputs. Nightly Rust
and .NET 10 are maintained through the existing setup actions. No GitHub
personal access token is installed on the server.
Only test/build steps use sccache; clippy runs directly. Incremental Rust
compilation stays disabled, as required by the compiler cache. Final runner
diagnostics report cache hits and misses. The cache tool is root-owned and its
download has a pinned SHA-256, following the [upstream Rust cache contract](https://github.com/mozilla/sccache/blob/main/docs/Rust.md).

## Provision or replace the runner

The administrator needs SSH sudo access and permission to administer repository
runners. The installer requires at least 8 GiB free, verifies the downloaded
runner archive against its pinned SHA-256, installs Ubuntu build prerequisites
and minimal nightly Rust, then registers the runner. Archive extraction uses
the CI account's permissions; root does not extract into its writable tree.
Dependency installation does not automatically restart unrelated services.

From a clean checkout with an authenticated GitHub CLI:

```sh
scp scripts/ci/provision-ovh-runner.sh ovh:/tmp/ferrule-provision-runner.sh
scp scripts/ci/guard-ovh-job.sh ovh:/tmp/guard-ovh-job.sh
scp scripts/ci/prepare-ovh-job.py scripts/ci/compact-job-evidence.py \
  scripts/ci/job-evidence.py scripts/ci/reclaim-build-cache.py ovh:/tmp/
gh api --method POST repos/DeandreT/ferrule/actions/runners/registration-token \
  --jq .token | ssh ovh 'sudo -n bash /tmp/ferrule-provision-runner.sh'
gh api repos/DeandreT/ferrule/actions/runners \
  --jq '.runners[] | {name, status, busy, labels}'
```

Keep shell tracing disabled around the short-lived registration token. The
installer refuses an active Ferrule service; stop it only after the runner is
idle before reprovisioning. An existing registration is retained. If replacing
the machine, remove its old registration through GitHub's runner settings
first. New registrations use name `ferrule-over` and labels `self-hosted`,
`Linux`, `X64`, and `ferrule-over`; only the controller grants automatic labels.
Existing registrations are not renamed by the installer. Builds require
16 GiB free for native CI and 8 GiB for codegen after provisioning, even though
installation itself requires less.

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
gh variable set OVH_RUNNER_ENABLED --body false --repo DeandreT/ferrule
```

Newly scheduled jobs use GitHub-hosted runners. Existing queued or running
jobs keep their selected runner; this setting does not interrupt them. To retire
OVH entirely, wait for idle, disable the Ferrule service, and remove this runner
from repository settings. Wareboxes and Valheim services are independent.

The routing follows GitHub's [self-hosted runner label documentation](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/use-in-a-workflow).
Review GitHub's [runner security guidance](https://docs.github.com/en/actions/reference/security/secure-use)
before expanding trusted inputs or granting account privileges.
