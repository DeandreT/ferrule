# Filtered conditional-key reference controls

This is a self-authored synthetic mapping, not a captured customer mapping.
`reference-cases.json` contains inputs and questions to observe, not expected
outputs. Do not use generation success or Ferrule's scalar filter fallback as
evidence of the reference application's behavior.

The key is `if Kind = "DIRECT" then Code else filter(Value, Kind = "FILTER")`.
Every physical leaf is a nonrepeating String field of the current `Items` row.
The first physical-filter output feeds only the final conditional branch; its
false slot is a keyless unused placeholder. The conditional output feeds both
the visible `Code` field and the parsed-array equality. This makes missing,
explicit JSON null and present empty values observable in complete output, not
only through whether the lookup matches. `Ordinal` witnesses receiving-row order.

## Reference procedure

1. Use the approved isolated MapForce launcher and an explicit readset containing
   this MFD and its three same-directory schemas. The only saved instance paths
   are synthetic `primary.json` and relative `output.json`. Generate with
   `/CS:DOTNET60` into a fresh owned directory and preserve the generation log.
   Refuse warnings that omit a mapped edge. Audit every generated path and the
   generated `Run` signature before authoring the execution harness.
2. Preserve generated source as its own evidence epoch. Build the declared .NET
   6 target with the selected SDK, outside the generated-source directory, and
   execute with the isolated .NET 6 reference runtime. Use explicit per-case
   primary input, `LookupText` and fresh owned output destinations. Do not invoke
   a generated default console entry point before auditing its destinations.
3. Run every case in a fresh process. Retain complete output bytes, complete
   output file membership and full errors separately from Ferrule results. Compare
   true and false predicates, missing and null values, the untaken filter branch,
   malformed input demand and first-match duplicates. The reference may enforce
   less schema validation than Ferrule; record that separately and retain
   Ferrule's full-document validation requirement.
4. In a separately hashed staged MFD, add only the edge from parser output `124`
   to target input `134`. Both Label declarations are already present but
   unconnected. Repeat the full matrix with this independent root scalar demand.
   In particular compare false-filter and empty-primary malformed-input controls
   with the unconnected-root mapping. Never overwrite the baseline fixture or
   reuse its generated-source evidence for the changed graph.
5. Admit a filtered-key grammar only after the executed reference outputs and
   errors establish presence, null and lazy-demand behavior. Prove the actual
   receiving owner, exact raw pins and feeds for every branch, then verify the
   entire lowered key and any presence guard against that proof. Keep shared
   predicates, foreign frames, multiple results and other expressions refused.
   Any shared-hook or runtime change requires separate scope approval.

Reference execution and implementation qualification are separate gates. A
filtered result with no item must not be replaced by scalar `Null` merely because
the final equality might not match. The unfiltered direct-null case is a control
to measure this distinction, not permission to erase it. These bounded controls
make no parsed-input caching or large-array performance claim.
