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

- [ ] Execute #198's frozen 90 native/Rust/C# typed-selection calls after #205's
  shared C# test-source release and fresh source binding; preserve its complete
  artifact/error evidence when rebasing `crates/codegen-csharp/src/json5_output_tests.rs`.
  Check full static input admission, global rules once, lazy selected loaders and
  counters. Its accepted 15-role source changes no ordinary all-output API.
- [ ] Agree #201's atomic private-owner remapping contract before same-project
  duplication; shared GUI adoption follows #194's release. Keep general copying out.
- [ ] Review #203's faithful saved-design representation and complete synthetic
  observations before creating narrow importer/exporter implementation issues.
- [ ] Execute #205's three test-only roles and failing-comparison retention controls,
  then compare identical focused log and retained-file sizes. Source is accepted;
  no measured log reduction, memory saving or semantic change is claimed.
- [ ] Freeze #207's complete matched native/Rust/C# fixtures before large-capture
  memory trials. Retain all repeats/outputs and distinguish process RSS from allocation
  attribution; propose optimizations separately only after concrete evidence.
- [ ] After #194 releases its frame helper, qualify #211's test-only diagnostic
  summary against the same controls and complete outcomes; log bytes are not RAM use.
- [ ] On the released #194 GUI baseline, qualify #213's desktop Fit on complete
  rectangles at 900 × 700 / 1200 × 900, including load/resize/canvas changes and
  finite empty/single views; preserve Project/wires/positions/sidecars.

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
