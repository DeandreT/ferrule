# Parallel Work

The [coordination issue](https://github.com/DeandreT/ferrule/issues/89) tracks
bounded work from the [roadmap](../ROADMAP.md). Issues define the problem,
in/out of scope, likely files and acceptance checks; this page helps contributors
choose a lane and avoid conflicting changes.

## Claim a lane

1. Read the issue and its linked contracts. Check current assignees and comments.
2. Assign the issue to yourself before starting. Maintainer work is assigned to
   `DeandreT`; an unassigned `help wanted` issue is available. If you cannot set
   an assignee, ask the maintainer to assign you in the issue before proceeding.
3. State the files or modules you plan to own and any shared verification slot
   you will need. Agree on overlapping edits before changing the same source.
4. Keep one bounded result per PR and link its issue. For a newly discovered
   gap, open a scoped issue before implementing it; do not silently widen the
   issue you claimed.
5. Leave concise updates in first person (I/me): result, evidence link, blocker,
   and next step. Release ownership when pausing or handing off so another
   contributor can take it.

## Next acceptance gates

- [ ] Qualify #201's accepted atomic private-owner proposal: all 67 native calls and
  alternate-display duplication/save/history checks remain unrun. Its 28-control
  contract and complete canvas snapshots are frozen; preserve #213's Fit hooks.
  Disk admission for the sole shared GUI verification lane is still pending.
- [ ] Qualify #268's source-reviewed complete XML oracles through its new 68 helper,
  17 controller-policy and 41 compatibility controls, then #263's individually
  admitted native cycle. All new calls remain unrun; earlier #263 simulated 41
  evidence stays separate, and no saved-form seed is inferred.
- [ ] Publish #253 with the single test-helper let-chain prerequisite #277 in open
  [#278](https://github.com/DeandreT/ferrule/pull/278); hosted jobs remain pending.
  The original strict-warning failure and both complete focused cohorts are retained;
  repeated runs do not add distinct cases.
- [ ] Publish #248's bounded report. Source/data and one full rendered diagram are
  independently reviewed; the 100% viewport requires scrolling. Browser-helper
  cleanup was needed; closure is proven, with normal cleanup-free closure false.
  Samples/stacks are not an exact object ledger, RSS is not allocation attribution,
  and no exact phase clock or capture cause is inferred.
- [ ] Settle #241's explicit competing-feed contract before admission and execution.
- [ ] Diagnose #247's actual Pages 404 and verify a successful deployed artifact;
  successful site compilation alone is insufficient.
- [ ] Independently review and publish #271's main fef16c3 two-doc status refresh.
  Existing fences stay byte-exact; no new rendering or product qualification is inferred.

## Dependencies and shared areas

Shared-file changes need explicit coordination even when the issues are otherwise
independent. Examples include app/workspace and test registrations, the workbook
draft, the JSON Schema facade, generated runtime/adapter modules, and CLI input
or artifact staging. Record a bounded file/function ownership split or sequence
the edits; preserve the other issue's complete behavior and tests.

## Verification resources

- Reserve one build lane when contributors share Cargo artifacts. Check disk
  space before and after substantial builds, reuse compatible caches, and use
  bounded jobs and incremental settings. See [Development](development.md#build-space-and-desktop-isolation).
- Reserve graphical or external-application sessions separately. Use an alternate
  display and an isolated chooser/session route; never drive another contributor's
  process or the user's active desktop. Bind the executable to the tested source
  and retain owned process closure, including failed attempts.
- Keep source review, focused local tests, generated public-host calls, normal
  desktop interactions, external execution and memory trials distinct. A source
  review or an emission pass does not close an execution gate.
- Measurements need their own finite input/profile/trial plan and full output
  checks. An observed peak is not a universal RAM limit or a streaming promise.

## Complete or hand off

A PR should describe the concrete resulting behavior, link the issue and record
which acceptance checks ran, with evidence or a precise blocker for checks still
open. Update only the corresponding roadmap/conformance gate after its required
evidence exists. Preserve earlier failures and narrower supported profiles.

The [conformance inventory](../conformance/README.md) remains incomplete. Do not
rewrite the protected survey as part of ordinary feature or coordination work,
mark unknown cells supported, or infer whole-product parity from a local cohort.
The coverage index in #95 keeps helper-only and compiled public-route evidence
separate; #133 records the completed #116–#119 slices without closing every XML
gap. Design-only #102 ends with an agreed follow-on plan rather than unreviewed
code.
