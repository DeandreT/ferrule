# Parallel Work

The [coordination issue](https://github.com/DeandreT/ferrule/issues/89) tracks
bounded work from the [roadmap](../ROADMAP.md). Issues define the problem,
in/out of scope, likely files and acceptance checks; this page helps contributors
choose a lane and avoid conflicting changes.

## Claim a lane

1. Read the issue and its linked contracts. Check current assignees and comments;
   the table below is a snapshot, not a replacement for the live issue.
2. Assign the issue to yourself before starting. Maintainer work is assigned to
   `DeandreT`; an unassigned `help wanted` issue is available. If you cannot set
   an assignee, ask the maintainer to assign you in the issue before proceeding.
3. State the files or modules you plan to own and any shared verification slot
   you will need. Agree on overlapping edits before changing the same source.
4. Keep one bounded result per PR and link its issue. For a newly discovered
   gap, open a scoped issue before implementing it; do not silently widen the
   issue you claimed.
5. Leave concise updates: result, evidence link, blocker, and next step. Release
   ownership when pausing or handing off so another contributor can take it.

## Ownership and readiness

Snapshot: 2026-10-07. Five issues are assigned to `DeandreT`; fifteen are
unassigned with `help wanted`. Available does not mean an implementation contract
has already been agreed. Design-first issues must settle their stated contract
before code changes. Follow each issue for later status and ownership updates.

| Issue | Owner | Next gate | Primary ownership / coordination |
| --- | --- | --- | --- |
| [#89 Coordination](https://github.com/DeandreT/ferrule/issues/89) | DeandreT; active | Issue map and shared reservations | Docs; coordinate all overlaps |
| [#90 Computed JSON properties](https://github.com/DeandreT/ferrule/issues/90) | DeandreT; active | Source reviewed; local checks pending | Scope inspector, workspace and GUI tests |
| [#91 Isolated Value map conversion](https://github.com/DeandreT/ferrule/issues/91) | DeandreT; active | Source authored; execution pending | Runtime helpers and both emitters |
| [#92 Workbook desktop persistence](https://github.com/DeandreT/ferrule/issues/92) | DeandreT; active | Desktop save/reopen pending | Desktop lane; setup fixes only if proved |
| [#93 XML root annotations](https://github.com/DeandreT/ferrule/issues/93) | DeandreT; active | Metadata observation pending | XML/.mfd metadata; shared display |
| [#94 JSON object conformance cells](https://github.com/DeandreT/ferrule/issues/94) | Unassigned | Ready for a bounded inventory design | Conformance docs and validator fixtures |
| [#95 Generated XML limit coverage](https://github.com/DeandreT/ferrule/issues/95) | Unassigned | Ready for an index; gaps get follow-on issues | Public-host tests and XML API index |
| [#96 Generated JSON5 adapters](https://github.com/DeandreT/ferrule/issues/96) | Unassigned | Contract first; strict JSON unchanged | Both emitters/runtimes; coordinate #95/#101 |
| [#97 Typed Value map cells](https://github.com/DeandreT/ferrule/issues/97) | Unassigned | UI contract ready; function checks need #91 | Cell editor; exclude coercion changes |
| [#98 Transposed workbook source setup](https://github.com/DeandreT/ferrule/issues/98) | Unassigned | Primary-source UI slice ready | Workbook draft; coordinate #92 |
| [#99 Browser edit history](https://github.com/DeandreT/ferrule/issues/99) | Unassigned | Browser history contract ready | Web app/canvas; native history unchanged |
| [#100 Required-only reference siblings](https://github.com/DeandreT/ferrule/issues/100) | Unassigned | Conservative exact subset design | JSON Schema facade; coordinate #101 |
| [#101 String-or-Int range constraints](https://github.com/DeandreT/ferrule/issues/101) | Unassigned | Model/backend contract first | IR, JSON constraints and both validators |
| [#102 Scalar-sequence composition](https://github.com/DeandreT/ferrule/issues/102) | Unassigned | Design milestone; code is a follow-on | Mapping/engine/codegen contract review |
| [#103 Typed SQLite query boundary](https://github.com/DeandreT/ferrule/issues/103) | Unassigned | Read-only query contract first | Boundary model, SQLite and CLI input |
| [#104 PDF extraction isolation](https://github.com/DeandreT/ferrule/issues/104) | Unassigned | Portable worker/limit contract first | PDF, CLI and applicable preview worker |
| [#105 Offline mapping bundle export](https://github.com/DeandreT/ferrule/issues/105) | Unassigned | Bundle contract first | CLI staging, resources and path codecs |
| [#106 JSON5 interchange metadata](https://github.com/DeandreT/ferrule/issues/106) | Unassigned | Observe saved metadata before implementation | Metadata lane; coordinate display with #93 |
| [#107 JSON output-set memory trials](https://github.com/DeandreT/ferrule/issues/107) | Unassigned | Bounded measurement plan first | Benchmark fixture/report; shared build lane |
| [#108 Mapping-path desktop workflow](https://github.com/DeandreT/ferrule/issues/108) | Unassigned | Existing implementation; desktop gate open | Desktop scenario; coordinate with #92/#93 |

Computed JSON properties are source-reviewed with local checks pending. The
isolated Value map correction is source-authored with execution pending. Neither
state is a passing test or desktop claim. The workbook and mapping-path desktop
lanes qualify existing implementations; metadata lanes must observe settings
before changing interchange admission.

## Dependencies and shared areas

A solid arrow below is a prerequisite for the labeled checks. A dotted arrow
means coordinate shared source or verification resources; it does not block
independent design/source work. In particular, generated JSON5 adapters and
native JSON5 metadata are separate contracts, and XML annotations do not have to
finish before JSON5 observation can begin.

```mermaid
flowchart TB
    C89["#89 Coordination"]
    subgraph gui["GUI and browser"]
        G90["#90 Computed properties"]
        G97["#97 Typed Value map cells"]
        G98["#98 Transposed workbook setup"]
        G99["#99 Browser history"]
    end
    subgraph generated["Generated execution"]
        G91["#91 Isolated conversion"]
        G95["#95 XML coverage index"]
        G96["#96 JSON5 adapters"]
    end
    subgraph schema["Schema interoperability"]
        G94["#94 Object-schema cells"]
        G100["#100 Required-only siblings"]
        G101["#101 Union ranges"]
    end
    subgraph design["Model and host contracts"]
        G102["#102 Sequence composition"]
        G103["#103 SQLite query boundary"]
        G104["#104 PDF isolation"]
        G105["#105 Offline bundle"]
        G107["#107 JSON memory trials"]
    end
    subgraph observed["Desktop and metadata qualification"]
        G92["#92 Workbook persistence"]
        G93["#93 XML annotations"]
        G106["#106 JSON5 metadata"]
        G108["#108 Mapping-path workflow"]
    end
    C89 -.-> G94
    C89 -.-> G90
    C89 -.-> G102
    C89 -.-> G107
    C89 -.-> G92
    G91 -->|Isolated conversion checks| G97
    G92 -.-> G98
    G100 -.-> G101
    G101 -.-> G96
    G95 -.-> G96
    G93 -.-> G106
    G92 -.-> G108
    G93 -.-> G108
```

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
The coverage index in #95 opens separate issues for identified gaps; design-only
#102 likewise ends with an agreed follow-on plan rather than unreviewed code.
