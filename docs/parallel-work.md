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
- [ ] Qualify #203's source-reviewed 24 synthetic recipes through materialization,
  native controls and save/reopen before narrow importer/exporter implementation.
  No faithful new saved form is admitted; existing typed refusal remains.
- [ ] Publish #203's frozen fixture before #253's focused engine unit controls for
  captures, stages and counters. Public-host observations and internal unit records
  stay separate; all executable checks are unrun, and no production getter or
  allocation-attribution claim is introduced.
- [ ] Use #207's merged fixtures for #248's separate allocation/GC trace analysis,
  retaining perturbation and complete outputs; RSS alone is not attribution.
- [ ] Execute #218's reviewed 40-recipe/229-call selected JSON contract after fresh
  source binding to the merged #207/#217 baseline. Rust/native/C# source review
  and prepared adoption are separate from these unrun calls.
  Retain the actual public return before separate decoded-wire observations.
- [ ] Settle #241's explicit competing-feed contract and #250's bounded generated
  004010 format/completion contract before their independent execution checks.
- [ ] Diagnose #247's actual Pages 404 and verify a successful deployed artifact;
  successful site compilation alone is insufficient.
- [ ] Complete #252's independent source/link/issue review and render only its
  changed parallel fences under the existing limits; published main 732d7ab is bound. Publication
  status is tracked in [#252](https://github.com/DeandreT/ferrule/issues/252).

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
