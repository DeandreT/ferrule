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

- [ ] Qualify #201's accepted atomic private-owner proposal: three focused tests plan
  29 duplication attempts. Frozen 28 controls / 87 snapshots / 92 canvas tables retain
  independent oracles. #298's accepted capture compiles; preserve the first focused
  stale-wire failure after nine primary previews and two duplications, #300/#301's
  distinct source-review gaps and the later invocation refusal. Their revised source
  is reviewed, applied and formatted. A fresh root-owned selected test passes all ten
  admitted cases and 67 local engine previews (61 primary / six named), with independent
  result review pending in this snapshot. Refusal, keyboard, full GUI and two Main/Named
  physical sessions / ten checkpoint pairs remain pending qualification here. Preserve
  #213 Fit hooks and fresh #288 resource admission after #297's locked-cache restoration;
  all frozen expectations remain unchanged.
- [ ] Qualify #263's individually admitted native save/reopen/save2 cycle after exact
  owned-window proof. Native 180-second timeout, #285 diagnostics and #291 presentation refusal
  remain negatives; modeled 16 query checks and helper source do not prove live ownership.
- [ ] Settle #241's explicit competing-feed contract before admission and execution.
- [ ] Diagnose #247's actual Pages 404 and verify a successful deployed artifact;
  successful site compilation alone is insufficient.
- [ ] Independently review and publish #299's scoped main 0c7b2ca two-doc successor,
  separately from #201; preserve unrelated lanes and the same historical fences.

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
