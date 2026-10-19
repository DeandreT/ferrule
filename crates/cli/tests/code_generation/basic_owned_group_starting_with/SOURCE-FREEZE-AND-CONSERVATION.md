# #443 pre-edit source/oracle freeze and ordinary Starting baseline recipe

**Unexecuted setup specification.** Actual #442 merge is M=`f30f6ca1c56a533bd4ccb4e4f4bfb821d939e971`, tree `2748a46873ef858c9c498d49f8d865e005b3366a`, equal to qualified Q=`4cf915dba1a25b369978dfd70b543979998adfc8`. No Project was constructed by a program, decoded, serialized, validated, lowered, emitted or run for this packet. No historical Starting package/output/binary is claimed present. The parent must authorize each later preparation/execution boundary; this recipe is not that authorization.

## 1. Literal ordinary Starting saved Project

Author a fresh ordinary fixture, not a textual mutation of the #442 Adjacent oracle/artifact manifest. Use only the existing legacy wire selector; there is **no owned expression/selector, ItemAt, failure rule, named source/target, adapter opt-in, sort/filter/window/post, or called function**.

Schemas, in exact field order:

- Source root `Input`: nonrepeating Group; child `Rows`: repeating Group with `Start:Bool`, `Label:String`.
- Target root `Output`: nonrepeating Group; child `Groups`: repeating Group with `First:String`, `Position:Int`, `Members`: repeating Group with `Label:String`.
- Expressions: node1=`SourceField { path:[Label], frame:Some([Rows]) }`; node2=`SourceField { path:[Start], frame:Some([Rows]) }`; node3=`Position { collection:[Rows] }`. No unused owned-demand node.
- Root has no iteration/bindings. Its `Groups` child has legacy `source:[Rows]`, existing `group_starting_with:2`, bindings First<-1, Position<-3. Its Members child has legacy inherited `source:[]`, binding Label<-1. All construction/output/schema behavior is ordinary existing Group/Repeated behavior.

Source-only wire recipe (field spelling follows M's tracked ordinary fixture; these are literal authoring bytes, **not a claim of canonical encode_pretty output**):

```json
{
  "source": {"name":"Input","kind":{"kind":"group","children":[{"name":"Rows","repeating":true,"kind":{"kind":"group","children":[{"name":"Start","kind":{"kind":"scalar","ty":"bool"}},{"name":"Label","kind":{"kind":"scalar","ty":"string"}}]}}]}},
  "target": {"name":"Output","kind":{"kind":"group","children":[{"name":"Groups","repeating":true,"kind":{"kind":"group","children":[{"name":"First","kind":{"kind":"scalar","ty":"string"}},{"name":"Position","kind":{"kind":"scalar","ty":"int"}},{"name":"Members","repeating":true,"kind":{"kind":"group","children":[{"name":"Label","kind":{"kind":"scalar","ty":"string"}}]}}]}}]}},
  "graph": {"nodes":{"1":{"kind":"source_field","path":["Label"],"frame":["Rows"]},"2":{"kind":"source_field","path":["Start"],"frame":["Rows"]},"3":{"kind":"position","collection":["Rows"]}}},
  "root": {"children":[{"target_field":"Groups","source":["Rows"],"group_starting_with":2,"bindings":[{"target_field":"First","node":1},{"target_field":"Position","node":3}],"children":[{"target_field":"Members","source":[],"bindings":[{"target_field":"Label","node":1}]}]}]}
}
```

Literal input, with Start real Bool, not String:

```json
{"Rows":[{"Start":false,"Label":"one"},{"Start":false,"Label":"two"},{"Start":true,"Label":"three"},{"Start":true,"Label":"four"},{"Start":false,"Label":"five"}]}
```

The independently authored complete expected tree has every Group's XML origin Unknown, all Repeateds real and ordered, all strings String-tagged and positions Int-tagged:

```text
Group[
  Groups: Repeated[
    Group[First:String("one"), Position:Int(1),
          Members:Repeated[Group[Label:String("one")],Group[Label:String("two")]]],
    Group[First:String("three"), Position:Int(2),
          Members:Repeated[Group[Label:String("three")]]],
    Group[First:String("four"), Position:Int(3),
          Members:Repeated[Group[Label:String("four")],Group[Label:String("five")]]]
  ]
]
```

No expected builder may partition input, call an evaluator, copy an actual, or compute groups through another mode. All/Primary are separate calls with the same literal Primary tree and empty extras; no named-target call is fabricated. Before any candidate edits, a separately authorized historical-source preparation must freeze the real Project's **exact pretty+LF serialization**, roundtrip evidence, literal input, full lossless expected tree and normal JSON presentation independently. The ordinary old reader is M's existing reader, not a new Starting companion. This packet's recipe is not the completed roundtrip.

## 2. Historical generator/source closure before implementation edits

1. Capture an immutable **M source archive**, embedded commit/tree identity and complete file manifest, including workspace Cargo.toml/Cargo.lock, crate manifests and all source, tests, corpus/fixtures, embedded runtime source and tracked historical baseline bodies. The source-only computed `git archive --format=tar M` SHA256 is `f078fffe8ee307450b44241e88c914b62c6410a5cf70246936b5a4c37671f2d6`; do not substitute Q's different archive hash even though its tree is equal. No archive was retained here.
2. Expand the actual build/source closure from that archive and bind it **at build time** to the historical generator executable. M's current #442 `source_bindings()` (`basic_owned_group_adjacent_by.rs:133–176`) is a useful functional-generator closure pattern: engine/codegen/Rust/C#/runtime/schema/functions/format-csv/edi/json/xml/ir/json-pattern/mapping source+manifests, full runtime C# source, corpus and CLI tests, workspace Cargo files. A complete Git tree archive is the conservative primary source closure. Add any linked CLI/local path dependency, `include_*` fixture, helper/host policy, build script and command settings that the actual chosen generator build reads; do not claim the old 14-crate subset alone covers arbitrary new CLI entry builds.
3. Under later compiler authority, build/use a generator from that immutable M source in a **fresh isolated historical target/build area**, not a shared target slot or candidate checkout. Retain toolchain/SDK/resolver selector identities, explicit environment, dependencies/lockfile, binary identity and before/after source checks. Do not relabel an existing a70/7769 executable as M. Existing a70->7769 L01 bridge applies **only** to its exact ordinary Adjacent fixture, not this new Starting Project.
4. Freeze generation APIs/options: ordinary `codegen::lower`, `codegen_rust::emit`, `codegen_csharp::emit`; fixed Rust package `ferrule-generated-mapping`, explicit Path runtime operand, no adapters; ordinary C# emit with no new profile. Capture complete sorted unique artifact inventories and actual bytes from this historical generator, before any candidate emission. Each row binds path/size/SHA256. Source and options are immutable prerequisites, not values inferred from candidate output.
5. Independently review baseline provenance, complete expected products and artifact package, and the future #443 candidate runtime-source delta policy. Then parent may grant the exclusive implementation paths. No historical compiler or emission is authorized by this report; if source-only scope continues, retain this gate as pending rather than invent historical rows.

Full-tree source equality must be rechecked in the parent's implementation checkout immediately before writes. A change outside M, a different profile/options operand, or a fixture edit invalidates the affected freeze; retain old evidence and create a separately reviewed successor instead of silently refreshing hashes.

C# `include_str!` runtime bodies are embedded when the generator is built: a later source snapshot alone cannot prove which bodies that executable contains. Bind the complete embedded closure at generator build time and compare its emitted bodies with the pre-emission reviewed candidate snapshot. Rust Path runtime dependencies instead also require their exact source/dependency closure at consumer compilation. Keep those bindings separate from a generator executable hash or artifact manifest.

## 3. Exact ordinary artifact comparison

- Compare historical **ordinary vs candidate ordinary**, same Project and same options. Separately compare old By to old By, old ItemAt to old ItemAt, old Adjacent to old Adjacent. Comparing ordinary to the new Starting companion is not conservation.
- Rust ordinary package has exactly `Cargo.toml` and `src/lib.rs`. Preserve complete historical library bytes. Preserve a complete historical Cargo body and complete expected Cargo template with **one explicit runtime-path operand**. First prove the historical operand reconstructs the entire historical body and manifest row, then instantiate the known candidate operand. Do not normalize actual output or strip sections. Freeze the operand and reject unreviewed path quoting rules.
- C# ordinary package remains **78 paths: three generated +75 runtime**. Three generated and72 unaffected runtime files must remain historical-byte exact. Candidate runtime exceptions are **provisionally only** `Runtime/FerruleBasicGroupBy.cs`, `Runtime/ScopeContext.cs`, `Runtime/FerruleInstance.cs`. They are not automatically approved by #442's policy name.
- For each provisional exception, capture M source bytes and future candidate source bytes **before emission**, old/new sizes+SHA256, complete source diff, exact artifact-path binding and independent #443 delta review. Candidate emitted bytes must equal that pre-emission reviewed source; verify source unchanged afterward. Every other path stays exact historical. A fourth modified runtime file, any new/removed path or new retained field/object needs explicit independent review and a new exact policy—not `Runtime/**` exclusion or a renamed Adjacent receipt.
- `legacy_artifact_policy.rs` currently has only `ExactHistorical` and `ReviewedAdjacentRuntime`; its snapshot helper reads candidate source but does not establish Starting review approval. H exclusively owns any separately granted policy extension. Preserve the #441/#473/#442 manifests, historical bodies, original policy results and receipts byte-for-byte.
- Artifact binding establishes packaging provenance, not behavior. Later execute the ordinary fixture and old Basic positive/negative controls against the exact new runtime, with lossless complete products. Never waive this because source layouts or artifact rows match.

## 4. #443 literal/source freeze before owned implementation

The corrected finite contract remains the sole semantic oracle. Expand every listed subvariation into a separately identified genuine Project/Program/component probe with a frozen complete input and literal tree/error/catalogue **before invoking any engine**. Freeze exact All/Primary/named call selections, complete schema/declaration order, node IDs, scalar tags/float bits/Unknown XML origin, native typed fields and full causes before any portable projection. Preserve unavailable/exact Unicode names and original catalogue ordinals. Do not count runtime-cohort probes as fake public Projects or claim an unexpanded recipe count as executed coverage.

Mandatory recipe safeguards:

- S23 uses H legacy-only targets, framed legacy SourceField `[Rows]`, legacy member Source([]), and exactly one ownership-demand control per variant. Its four literal H groups are p0/[p0,p1], p2/[p2], p3/[p3,p4], p5/[p5,p6]. The post-only equality must keep the owned node as sole demand while returning true. No inherited RootMarker/owned First, no sibling accidental owned node, no base600 copy that loses RequiredLegacy.
- For otherwise owned-only catalogue cases, choose intended innermost legacy Position([]) without accidentally requiring declared named Rows. Where [Rows], [Alias] or [Batches,Rows] is the intended explicit probe, retain it and freeze its actual conservative RequiredLegacy catalogue. Do not weaken admission or relabel expected catalogues from actuals.
- Ordinary active Item identity selections preserve parent position; genuine grouped-member iteration creates independent member1..n. Preserve the distinction in X10's ordinary-root conservation and the grouped root cases; the #442 N10 literal correction is not permission to reset positions universally.
- R10's independent resource worksheet stays seed38+fixed28+windows144=210, capacity5 used39/request172/max210. R09/R12/R13 preserve zero Boolean grouping-key bytes. Old keyed By/Adjacent duplicate-string thresholds stay their own unchanged controls. Source-counted resource arithmetic is not production Footprint output.
- Freeze old By/ItemAt/Adjacent positive/negative controls, old wrappers' Starting rejection, all closed contexts/structural precedence, S/X/E/I/C/Q/R and their complete variants. No new producer, failure permission or grouping island bridge is inferred.

Later candidate generation must bind the exact implementation source/build closure and reviewed runtime deltas **before** emitting artifacts, retain raw products separately from oracles, compile public typed Rust/C# consumers and actually invoke them under the parent's serialized execution grant. Focused/component checks, old-profile conservation, full public three-engine runs and exact proposed-head repository checks are future gates; this source-only packet supplies none of those results.
