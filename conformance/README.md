# MFD conformance inventory

`mfd-2026r2.json` pins the current conformance target to an **Enterprise
2026r2 `.mfd`-compatible application**. It is a seeded, non-exhaustive
capability inventory, not a claim of full compatibility or a product-parity
percentage. The release pin is
intentional even when the vendor's release-history page changes.

Run these commands from the workspace root:

```sh
cargo run -p mfd-conformance
cargo test -p mfd-conformance
cargo run -p mfd-conformance -- summary --format json
cargo run -p mfd-conformance -- gate --profile mfd-2026r2-enterprise
```

The binary accepts another ledger path and `--root PATH` for its repository
evidence root. Relative ledger and evidence paths resolve from that root. The
workspace's ordinary tests validate the checked-in inventory and its local
references; no vendor application, network access, or private sample corpus is
needed. The default validation command exits nonzero for malformed inventory or
evidence.

`summary` reports exact status counts for each selected profile and dimension;
`--format json` emits the same deterministic counts for automation. `gate`
reports those counts plus a failure for every selected in-scope cell that is
unassessed, unverified, unsupported, partial, blocked, or missing required
evidence. It exits 0 only on a pass and 1 on a conformance failure. A
`not_applicable` cell is excluded only when it cites evidence; native MFD
export and vendor-backend exclusions need vendor documentation or execution
evidence. A selection containing no applicable cells fails rather than passing
vacuously. No skip or ignore flag exists.

Use repeated `--profile ID`, `--capability ID`, and `--dimension NAME` options
to create a focused development gate. Each option is inclusive; omitting an
axis selects all values on that axis. Unknown, duplicate, or empty
profile/capability intersections fail. Dimension names are `import`,
`interpreter`, `native_export`, `ferrule_roundtrip`, `gui`, `debug`, `rust`,
`csharp`, and `vendor_backends.xslt1`, `.xslt2`, `.xquery1`, `.cpp`, `.java`,
`.csharp`. For example:

```sh
cargo run -p mfd-conformance -- gate \
  --profile mfd-2026r2-enterprise \
  --capability interop.mfd-designs \
  --dimension native_export --format json
```

A gate covering every capability and dimension in a selected profile also
requires `inventory_complete=true`, whether those values were selected
implicitly or listed explicitly. The checked-in seeded inventory is incomplete,
so its full-profile gate intentionally fails. A focused gate can qualify a
development slice without implying that the product or profile is complete.

Each capability independently assesses MFD import, interpreter execution,
native MFD export acceptance, Ferrule self-roundtrip, GUI authoring,
debugging, Ferrule Rust/C# generation, and vendor XSLT 1.0, XSLT 2.0, XQuery 1.0,
C++, Java and C# backends. Vendor-backend applicability must be checked per
capability: the presence of a language in this schema does not imply that every
format or operation is available in that language.

| Status | Meaning |
|---|---|
| `unassessed` | This surface has not yet been inventoried. |
| `unverified` | A relevant implementation or claim exists, but its required behavioral proof is absent. |
| `unsupported` | The specified capability is not implemented on this surface. |
| `partial` | The detail identifies an implemented subset with linked evidence. |
| `supported` | The entire capability as scoped in this row has linked evidence. |
| `blocked` | A required external dependency prevents assessment or execution; explain it in the detail. |
| `not_applicable` | The surface is outside this capability's scope; explain why. |

`supported` and `partial` require evidence. On `native_export` and every
`vendor_backends` surface they specifically require `vendor_execution` evidence;
an emitter, a vendor manual or a Ferrule re-import test cannot establish vendor
execution. The validator checks evidence structure and file existence, not the
truth of a supplied execution record. Reviewers must verify that such records
actually exercise the claimed capability and pinned vendor version.

The `native_mfd` profile is distinct from `ferrule_native_extensions`.
Ferrule components, captured-service provenance and retained layout metadata can
successfully self-roundtrip without being understood by the vendor application.
Add native profile coverage only after native lowering and vendor acceptance
are proved.
Neither profile should silently inherit status from the other.

Version 1 uses required, strictly typed fields; unknown fields and unknown
enum values reject. Every ID must be unique within its namespace and use
lowercase ASCII letters, digits, dots, underscores or hyphens. All profile,
capability-dependency and evidence references must resolve. Duplicate references,
dependency cycles, empty inventories, blank explanations, unsafe or missing
local evidence paths and missing status dimensions reject. Both profiles must
have at least one capability. HTTPS documentation locations are checked
structurally but are not fetched by the validator.

To extend the inventory, split broad seeded rows into individually testable
functions, configuration commands, formats, connector operations and workflows.
Preserve existing IDs when their scope remains the same. Add repository-relative
evidence or official vendor-documentation references, assess every surface
explicitly, and run the validator and focused tests. Vendor execution records
should identify the release, backend, inputs, outcomes and comparison contract;
keep proprietary samples and outputs outside the public repository. Public
evidence can be self-authored fixtures and non-proprietary run summaries.

Keep `inventory_complete` false until the chosen vendor edition/release has been
fully itemized and reviewed. The current entries are a starting inventory drawn
from the repository's recorded support boundaries and official product/manual
references. Historical corpus counts in linked documents are not fresh run
results, and the local informational surveys are not automatically promoted to
release gates by this ledger.

## Generated XML test catalog

Keep closed ordinary `Structured` XML input and observed `RootView` input as
separate profiles. A direct reader comparison does not establish execution of
the public generated XML APIs or native interchange. Their evidence needs
independent gates:

| Gate | Required comparison |
| --- | --- |
| Schema admission and transport | Closed-input proof versus plain/versioned descriptor parsing; logical depth and JSON container depth; core-only fallback versus strict observed-policy refusal |
| Direct readers | Rust/C# string and byte calls, complete scalar tags/Int payloads/Float bits, ordered repetitions and Group type origins; namespace, nil, missing/empty and strict lexical failures |
| Resource limits | Independent physical/result counts, logical bytes, projection work, raw reservation slots, namespace references/registry bytes and parser work; typed resource units/count/limit and no result |
| Published libraries | Untouched public generation, compiler outcomes, all four XML APIs, fixed-context versus missing-context behavior, every physical mapped output occurrence and existing JSON cardinality refusals |
| Static XML output sets | All four set APIs and singular compatibility; complete primary/named typed values and physical XML; two-extra declaration order, independent schemas/hints/namespaces, mapping-before-serialization failure order, target-aware typed causes and no partial returned set |
| XML output-set limits | Exact combined UTF-8 byte and artifact-count edges, including primary; distinct per-document limits, checked counters and typed resource causes; eager materialization remains separate from admission |
| Memory | Fresh-process phase/peak measurements of eager DOM, Instance and output processing; acceptance counters do not imply streaming or an RSS limit |

The [generated XML boundary](../docs/code-generation.md#xml-host-boundary)
documents the supported profile and limits. These catalog requirements do not
change ledger statuses or imply that every gate has passed.

## JSON object-schema inventory proposal

[`json-object-schemas.proposal.json`](json-object-schemas.proposal.json) is a
separate version-1 ledger for a bounded object family. It does not replace the
canonical `formats.json` capability, change any historical assessment, or claim
that the inventory is complete. Existing IDs keep their original scope. The
new `json.objects.<operation>.native` and `.extensions` IDs are proposed child
scopes, not renamed versions of that broad row. Any canonical split needs a
separate review of the retained row and its other JSON/array/composition work.

Each operation has one native candidate and one independent extension-profile
cell for every dimension. The latter records unknown applicability until a
distinct semantic extension dependency is identified. Ordinary schema and
round-trip annotations alone do not imply such a dependency. Fixed fields here
mean declared field names; this proposal does not assess a new computed-property
editor or other ongoing product increments.

| Operation ID segment | Exact bounded operation and discriminating cases | Source regression leads |
| --- | --- | --- |
| `closed-fields` | Named string/Int/Bool fields, nested closed objects; undeclared key refuses. | `explicit_closed_objects_reject_undeclared_input_at_every_native_boundary`; generated object-openness hosts |
| `typed-extra-fields` | One scalar `additionalProperties` domain beside named fields; wrong-typed extra value refuses. | `typed_additional_properties_remain_typed_after_open_intersection`; `typed_additional_properties_roundtrip_as_dynamic_fields` |
| `arbitrary-extra-fields` | Omitted/true/empty-object `additionalProperties`; extra scalar/object/array/null values retain their JSON shape. | `omitted_additional_properties_is_open_and_roundtrips_arbitrary_values`; `explicit_unconstrained_additional_properties_roundtrip_arbitrary_values` |
| `declared-required` | Closed object with required `id` and nullable `note`; absent required value fails, explicit null is present, omitted required output fails. | `ordinary_required_properties_distinguish_absence_from_explicit_null` |
| `runtime-required` | Open object with required runtime name `x-correlation-id`; absent name fails, present extra property satisfies presence. | `required_runtime_named_properties_work_for_open_objects` |
| `nullable-object` | Optional nullable closed child: missing, explicit null, present empty and present nonempty remain distinct. Arrays are excluded. | Object cases in `nullable_objects_and_arrays_preserve_absent_null_and_empty_values` |
| `exclusive-alternatives` | Compatible closed `oneOf` branches with shared field schemas and required sets; exact one-match, no-match and ambiguous-match controls. | `compatible_object_one_of_preserves_and_roundtrips_alternatives`; `object_one_of_subtypes_import_execute_and_select_xml_types` |
| `inclusive-alternatives` | Compatible closed `anyOf` branches; one or several matches pass, no-match fails. | `compatible_object_any_of_preserves_inclusive_matching_and_roundtrips`; `incompatible_object_any_of_is_rejected_actionably` is a separate refusal control |

The proposal's repository evidence entries identify the complete source files.
They are leads for self-authored regressions, not new execution reports. Some
hosts cover only part of a row; each eventual result must state which literal
case, API and profile it exercised. General correlated compositions, incompatible
shared-field schemas, arbitrary nested dynamic construction, arrays, schema
reference dialects, JSON5/JSON Lines and additional validation keywords are not
itemized by these eight operations. Compatible `anyOf` overlap is not generally
classified as unsupported.

The assessment table below applies separately to each exact operation. The
proposal repeats the concrete missing proof in every cell, including each of
the six vendor backends.

| Dimension | Native candidate status and applicability | Extension-profile status and applicability |
| --- | --- | --- |
| `import` | `unverified`: candidate ordinary MFD JSON import; fresh complete schema/project/diagnostics missing. | `unassessed`: distinct extension fixture/dependency and import eligibility unknown. |
| `interpreter` | `unverified`: typed decoding, mapping and output success/error proof missing; decoder-only tests are insufficient. | `unassessed`: extension semantics and interpreter contract unknown. |
| `native_export` | `unverified`: exact pinned native eligibility unresolved; local render/preflight is separate from external open/validate/execute/save. | `unassessed`: no automatic exclusion; extension dependency and external interpretation unknown. |
| `ferrule_roundtrip` | `unverified`: complete original/reimported schema and behavior proof missing. | `unassessed`: exact extension metadata/semantics fixture and retention proof missing. |
| `gui` | `unassessed`: exact schema authoring/import/edit/save workflow and controls not inventoried. | `unassessed`: extension-specific reachable workflow unknown. |
| `debug` | `unassessed`: exact Preview/trace/error-observation workflow not inventoried. | `unassessed`: extension-specific observation semantics unknown. |
| `rust` | `unverified`: compiled generated public JSON text/bytes and typed/context/named-input case results missing. | `unassessed`: extension lowering/runtime eligibility unknown. |
| `csharp` | `unverified`: separate compiled generated public JSON case results missing; Rust/emission results do not transfer. | `unassessed`: extension lowering/runtime eligibility unknown. |
| `vendor_backends.xslt1` | `unassessed`: XSLT 1.0 operation/format eligibility and pinned generate/compile/run evidence unknown. | `unassessed`: independent extension/backend eligibility unknown. |
| `vendor_backends.xslt2` | `unassessed`: XSLT 2.0 operation/format eligibility and pinned generate/compile/run evidence unknown. | `unassessed`: independent extension/backend eligibility unknown. |
| `vendor_backends.xquery1` | `unassessed`: XQuery 1.0 operation/format eligibility and pinned generate/compile/run evidence unknown. | `unassessed`: independent extension/backend eligibility unknown. |
| `vendor_backends.cpp` | `unassessed`: C++ operation/format eligibility and pinned generate/compile/run evidence unknown. | `unassessed`: independent extension/backend eligibility unknown. |
| `vendor_backends.java` | `unassessed`: Java operation/format eligibility and pinned generate/compile/run evidence unknown. | `unassessed`: independent extension/backend eligibility unknown. |
| `vendor_backends.csharp` | `unassessed`: vendor C# operation/format eligibility and pinned generate/compile/run evidence unknown. | `unassessed`: independent extension/backend eligibility unknown. |

No external dependency is established as blocking these cells, so none is
labelled `blocked`; lack of an assessment or run is stated as unknown or
unverified. An actual unavailable tool/resource should later be named in a
specific blocked cell. Historical import/self-reimport, lowering/emission and
local execution surveys remain three distinct informational evidence scopes.
None automatically supplies fresh behavioral or vendor proof for this proposal.

Validate and inspect the proposal from the workspace root:

```sh
cargo run -p mfd-conformance -- conformance/json-object-schemas.proposal.json
cargo run -p mfd-conformance -- conformance/json-object-schemas.proposal.json summary --format json
cargo test -p mfd-conformance --test json_objects
cargo run -p mfd-conformance -- conformance/json-object-schemas.proposal.json gate \
  --profile mfd-2026r2-enterprise \
  --capability json.objects.declared-required.native \
  --dimension interpreter --format json
```

The focused gate intentionally fails while its cell is unverified. The new
regression source checks this failure, empty/unknown selections, missing local
evidence, unsupported proof promotion, evidence-only exclusions and incomplete
full-profile gates. Its synthetic supported-cell control tests validator
mechanics; it does not qualify JSON mapping behavior. Before changing a cell to
`supported`, retain fresh complete operation/API/profile-specific values and
causes. Native export and vendor-backend support require separate release-bound
execution records, not a source test or a successfully emitted library.
