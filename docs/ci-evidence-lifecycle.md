# CI evidence lifecycle

`scripts/ci/evidence-lifecycle.py` is an **inactive library**, separate from the
existing pre-job compactor. It has no operational cleanup command, provider
adapter, credential discovery, service installation, or workflow integration.
Running it directly refuses activation. The existing compaction and cache
reserve policies are unchanged. Local archives and expiring Actions artifacts
are not durable-retention authority.

## Trust boundary

The controller must run from administrator-owned code/configuration with a
private state directory **outside the runner home and unwritable by jobs**.
It must be able to inspect the runner's files without granting jobs access to
controller state or provider credentials. The local ownership checks enforce
the executing account for state and the configured `runner_uid` for source
evidence; shared readers retain their current-account default for compaction.
These checks cannot establish
that deployment separation. Running this as the untrusted job account does not
qualify the security boundary. No JSON field, workflow outcome, descriptor
census, or `always()` step grants closure or deletion authority.

An administrator-supplied `Containment.lease(preparation)` must bind the existing
`job-evidence.py` preparation record (including root device/inode, nonce, and
repository/run/attempt/job/source/suite) to the supervisor's independent job
identity. Before yielding, it must prove that all job producers and descendants
have terminated, that all temporary output was confined to the admitted root,
and that no surviving process can write or reopen that root. The exclusion must
last through the entire operation. Unknown or escaped descendants, incomplete
termination, cancellation without proof, and unproven temporary-root compliance
must refuse. The built-in authority always refuses. The test authority is only
valid for process-free, isolated fixtures.

## Provider contract

The small `Provider` interface has three operations:

- `put(key, source, sha256, size)` streams one exact object and returns an
  `ObjectRef` identifying provider, approved destination, key, immutable version,
  SHA-256 and byte length. Retrying the same key/digest is idempotent; a different
  body at that key must refuse. Multipart completion must be confirmed. Names,
  latest-object lookup, and workflow success do not identify an exact artifact.
- `read(ref, destination)` exports **that exact artifact/object ID and version**,
  not a new object selected by name. It writes sequentially to a controller-owned
  bounded writer. The controller verifies exact length and compressed SHA-256,
  then checks every archive member's path, type, body hash, permissions, owner,
  modification time and inventory against the manifest. Duplicate, missing or
  extra members refuse. Full nanosecond times remain in the preserved manifest;
  tar timestamps are checked against their encoded representation.
- `retention(ref)` independently queries provider-side enforced retention for
  the same exact version. The typed result must match that object and cover the
  administrator-approved future retention deadline. An upload response, local
  manifest assertion, lifecycle/expiration rule, or deletable workflow artifact
  is insufficient. The adapter must verify the actual remote lock/immutability
  semantics and fail if enforcement or credentials cannot be established.

The complete archive **and** its canonical manifest must each pass upload,
readback and retention checks. The manifest contains preparation identity,
original directory identities, every member, and compressed archive identity.
Provider returns are never accepted from arbitrary user JSON. Credentials belong
only to a qualified trusted adapter, not to the runner or checked-in files.
The in-memory test double explicitly provides no real durability.

## State and recovery

The controller consumes existing immutable preparation metadata; it does not
change its schema or manufacture closure in that record. Supported roots are
`ci-tmp/jobs/<job-id>` with nonce or existing descriptive IDs admitted by the
shared preparation reader. Legacy flat directories and already-compacted local archives remain under the existing
compactor/manual preservation policy, not this helper's retirement authority.

| State | Meaning and retry behavior |
| --- | --- |
| `CLOSED_UNARCHIVED` | Trusted lease obtained; originals retained. Archive creation can retry. |
| `ARCHIVED_LOCAL` | Complete archive and manifest flushed locally. No external durability claim. Upload/readback/retention may retry. |
| `PRESERVED` | Both exact remote versions verified under approved retention. Originals remain unless retirement was explicitly requested. |
| `RETIRING` | Finite original manifest committed before first unlink. Resume requires a new containment lease and fresh provider verification. |
| `RETIRED` | Original root removed and journal flushed; archive, manifest and journal remain. Recreated roots refuse. |

All transitions use a private serialized controller journal, atomic replacement,
and file/directory fsync. No saved `PRESERVED` flag or provider receipt authorizes
deletion by itself: retries repeat exact readback and remote retention checks.
Failure or cancellation leaves originals; partial retirement is only possible
after both complete durable copies are verified. Recovery admits only removals
from the original finite manifest, preserving directory inode/owner/mode and
exact surviving file identities. New entries, replaced roots or files, changed
contents, mounts, hardlinks, symlinks, special files, foreign ownership, and
unsafe permissions refuse. Symlinks are deliberately unsupported for retirement,
even though the existing local compactor can preserve them.

Deletion uses descriptor-relative unlink/rmdir for each original manifest entry;
there is no recursive cleanup, path glob, cache deletion, remote deletion, or
local archive expiration. Unrelated jobs and active caches are never candidates.
Staging conservatively retains the existing 8/16 GiB reserve plus 4 GiB headroom
on both the runner and state filesystems. Failed staging can leave controller-
owned pending files for administrator inspection; these never authorize deleting
original evidence. Archive/readback failures do not reclaim space to bypass a
reserve gate.

## Qualification required before deployment

Activation needs all of the following, supplied and approved separately:

1. The actual provider, account/project, bucket/container or service destination,
   namespace, version/immutable artifact-ID semantics, region if applicable, and
   retention policy/deadline. Confirm immutability and remote deletion controls
   independently of upload success.
2. Least-privilege upload, exact-version readback, and retention-query credentials
   delivered to the protected controller, plus an approved recovery identity.
   No resource or credential is created by this library.
3. A privileged producer supervisor with independently recorded job identities,
   enforced descendant containment, termination and temporary-root confinement,
   and a lease preventing future writes. Install code/state outside runner write
   access and qualify cross-account file access without relaxing that boundary.
4. Bounded network timeouts/retries, provider-specific multipart and retention
   failure handling, and recovery behavior in a real adapter. The interface is
   synchronous; the adapter must bound its external calls.
5. Independent security review and a tiny canary using self-authored data: failed
   and canceled jobs, enforced closure, exact-version export, all member and
   metadata checks, retention enforcement, interrupted uploads/downloads,
   crash recovery and finite retirement. Verify active-cache and reserve behavior
   before explicitly enabling any automatic policy.

Run the offline tests with an on-disk private temporary directory, for example:

```sh
TMPDIR="$HOME/.cache/ferrule-evidence-tests" python3 -B scripts/ci/test-evidence-lifecycle.py
```

Create that private directory first. These tests do not contact a provider or
supervisor, start producers, or qualify a live deployment. This work is **Part
of #360**, not completion of its external-provider/containment/canary gates.
