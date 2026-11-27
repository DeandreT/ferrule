# Parallel Work

Use this guide when two issues need the same files, build outputs or display.
GitHub issues own the current assignments. [Development](development.md) lists
the checks for each kind of change.

## Claim a lane

<a id="find-an-unclaimed-lane"></a>
<a id="work-in-a-separate-branch-and-worktree"></a>

Use [Contributing](../CONTRIBUTING.md) to find an unassigned issue, claim its
paths, and start an independent branch and worktree. The issue owns the scope
and status; a coordination index is not a reservation. `help wanted` identifies
outside-contributor work; `deferred` issues are not available for implementation.
Give independent checkouts separate build outputs so a stale executable cannot
stand in for the source being reviewed.

## Shared files and handoffs

An issue's reserved paths belong to its assignee. Files outside every lane are
shared entry points, not parallel ownership. Examples include the workspace
`Cargo.toml` and `Cargo.lock`, crate `lib.rs`/`main.rs` module registrations,
app/workspace and test registrations, generated
[technical references](reference/README.md), CI definitions, the workbook draft,
the JSON Schema facade, generated runtime/adapter modules, and CLI input or
artifact staging.

Before editing a shared file, agree a handoff in the issue: the file or function
you will change, who edits it and in which order. The maintainer integrates
registrations that a lane cannot make inside its own paths. Preserve the other
issue's complete behavior and tests, and rebase after a handoff merges.

## Generated-code source ownership

The [owned-source contracts](design/source-ownership.md) and
[generation profiles](code-generation.md#owned-generation-capabilities-and-presets)
define admission and caller behavior. Coordinate changes to the shared
[`OwnedCapabilities` table](../crates/codegen/src/capabilities.rs) and
[validation](../crates/codegen/src/validate/owned_sources/) before changing
the Rust or C# renderer. Each backend owns its generated symbols and runtime
integration. Private failure-map renderers do not expand the shared public gate.

Document supported behavior and its limits in one guide; keep progress, check
results and handoffs in the issue.

## Verification resources

- Run the checks for your change locally before requesting review; see
  [Development](development.md#run-the-appropriate-checks).
- Reserve one build lane when contributors share Cargo artifacts. Check disk
  space before and after substantial builds, reuse compatible caches, and use
  bounded jobs and incremental settings. See [Development](development.md#build-space-and-desktop-isolation).
- Reuse build artifacts only within the source profile that produced them. Source
  timestamps and manifest paths alone do not establish which source produced
  cached executable bytes.
- Reserve graphical or external-application sessions separately. Use an alternate
  display and an isolated chooser/session route; never drive another contributor's
  process or the user's active desktop. Bind the executable to the tested source
  and retain owned process closure, including failed attempts.
- Keep source review, focused local tests, generated public-host calls, normal
  desktop interactions, external execution and memory trials distinct. A source
  review or an emission pass does not close an execution gate.
- Measurements need their own finite input/profile/trial plan and full output
  checks. An observed peak is not a universal RAM limit or a streaming promise.

<a id="complete-or-hand-off"></a>

Link the PR to the issue and record qualification evidence and remaining
dependencies there. Update a [conformance cell](../conformance/README.md) only
when that particular backend and workflow have the evidence its contract
requires. A local fixture cannot establish whole-product parity.
