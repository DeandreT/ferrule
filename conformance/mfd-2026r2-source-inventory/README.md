# Supplemental source inventory

These files review documentation bodies beyond the original topic inventory in
[`mfd-2026r2.source-contracts.json`](../mfd-2026r2.source-contracts.json).
They do not change the canonical capability ledger or establish execution support.

| File | Source coverage |
| --- | --- |
| `nonleaf.json` | 191 parent-topic bodies, including additional operations and existing-context refinements |
| `functions.json` | 319 named function leaves, parameter tables, language declarations and unresolved contract facets |
| `constructors.json` | 43 `xs:` constructor leaves and their shared lexical/type statements |
| `com-api.json` | Separate public COM API catalog; release binding remains unestablished |
| `com-name-alignment.json` | Name-only interface, enum and event comparison against the release-bound type library, preserving discrepancies and dump-rendering gaps |
| `com-enum-values.json` | All 20 release-bound enum tables and 130 signed constants, compared with the pinned online catalog without establishing runtime support |
| `com-generation-signatures.json` | Four release-bound `IDocument` generation methods and seven ordered parameters, with raw descriptor targets and a separate release-unbound online comparison |
| `com-result-getters.json` | Fourteen release-bound property getters on the two generation-result interfaces, with independent raw targets and release-unbound online property declarations |
| `com-collection-getters.json` | Ten release-bound property getters on `IAppOutputLines` and `IErrorMarkers`, twelve ordered parameters and eight release-unbound online properties |
| `com-core-declarations.json` | 114 release-bound callable records and 119 ordered parameters on `IApplication`, `IOptions`, `IDocument` and `IDocuments`, using shared typed source validation |
| `com-graph-declarations.json` | 79 release-bound mapping, component, graph and view declarations with 110 ordered parameters |
| `com-projects-declarations.json` | 68 release-bound project, project-item and library-import declarations with 79 ordered parameters |
| `com-diagnostics-declarations.json` | 28 release-bound output-line, symbol and error-marker declarations with 44 ordered parameters |
| `com-events-declarations.json` | Eight release-bound event-source declarations with seven ordered parameters; five function-body dump anchors are explicitly null |
| `com-callable-coverage.json` | Finite, disjoint membership of all 325 callable records; declaration adoption remains separate from runtime support |
| `supporting.json` | Public I/O classes, wrapper-library exceptions and the named installation-artifact requests |
| `schema-wrappers.json` | Four optional schema-wrapper source profiles, actual schema identities and build/dependency metadata; no compiled or runtime qualification |
| `external-references.json` | Linked historical DOM algorithm and redirected .NET interface contract, kept separate from vendor claims |

The manual reviews use the same pinned PDF and layout-text hashes as the original
source companion. Line numbers refer to that exact extraction; physical PDF page
numbers remain distinct from printed headers. The public PDF can be retrieved from
<https://www.altova.com/documents/MapForceEnterprise.pdf>; verify its recorded hash
before reusing those anchors. `pdftotext -layout` produces the referenced extraction.

Missing declarations, defaults, types and conflicting source claims remain
explicit. An editorial signature summary is not a literal API declaration.
Language badges describe vendor documentation, not Ferrule support or independently
verified backend applicability. These overlapping sets are not an additive parity
denominator.

The COM catalog has its own per-page hashes and explicitly provisional identities.
Its documented installation and visible-window requirements do not prove a working
vendor environment. Its scoped catalog-completeness flag does not establish
Enterprise 2026r2 inventory completeness. The canonical `inventory_complete` flag
remains false while release provenance, supporting classes and remaining source
contracts are unresolved.

The type-library name comparison does not bind the online catalog's signatures
or behavior to the installed release. It retains the different spellings of
`HighlightMyConnectionsRecursively` and `HighlightMyConnectionsRecursivly`,
additional declaration names, and dump-rendering gaps resolved by bounded raw
name-table reads. Event-source names are compared separately from interface
members. Signatures remain unqualified; name matches and accessor-record counts
are not additive capabilities or ABI proof.

The separate enum-value inventory retains exact declaration names, signed values,
record offsets and source hashes. Of 130 constants, 124 match the pinned catalog,
one differs (`eProjectTypeCount`: 18 in the type library, 16 online), and five are
type-library-only. The three `-1` sentinels use a referenced signed value, not a
literal interpretation of their encoded offset. `ENUMViewMode` values come from
bounded raw records because the pinned dump does not render that table. The
existing three-value observation in `artifact-observations.json` is preserved.
These declarations do not establish aliases, accepted runtime options, backend
applicability, or the online catalog's release binding. The canonical inventory
remains incomplete, and no execution evidence follows from this comparison.

The generation-signature fragment records `GenerateCodeEx`, `GenerateOutputEx`,
`GenerateOutputEx2` and `GenerateCodeEx2` only. It pins the HRESULT declarations,
member IDs, ordered parameter names, flags and descriptor chains to the retained
type library and dump. The online pages display the result objects as higher-level
returns, rather than raw HRESULT/out-retval signatures; that difference is not a
contradiction. Their constraints and error lists remain release-unbound, including
the visible-window wording for `GenerateOutputEx`, which is not transferred to
`GenerateOutputEx2`. The fragment does not establish ABI, calling convention,
marshaling, ownership, lifetime, runtime defaults or backend support. Independent
expected declarations and mutation controls run without vendor files or activation.

The result-getter fragment covers seven properties each on
`IOutputGenerationResult` and `ICodeGenerationResult`. Raw `Application` and
`Parent` parameters retain generic `PTR(DISPATCH)` separately from the online
`Application` type. Getter-only raw records do not rewrite the online access
labels. The message-tree and line-break descriptions do not establish runtime
ordering or lifetime, and the success/failure declarations do not establish
complementarity or a relationship to HRESULT. Enum constants remain in the existing
enum inventory, not a new cohort. Source binding rests on HTTPS acquisition and
byte identity; Authenticode integrity and trust remain unverified. No activation,
execution, ABI, default, error, ownership, coercion or marshaling claim follows.

The collection-getter fragment preserves the different declaration order and
Count member IDs on `IAppOutputLines` and `IErrorMarkers`. Item targets are
independently resolved as `IAppOutputLine` and `IErrorMarker`; its online
one-based index, Integer notation and error lists remain source claims. Both
online Parent types are retained: AppOutputLine for AppOutputLines, Application
for ErrorMarkers, separately from raw generic `PTR(DISPATCH)`. The eight
read-only labels come from pinned interface summaries. The two type-library-only
`_NewEnum` records have signed ID -4, zero function flags and one explicit
`PTR(UNKNOWN)` layer, not an inferred extra pointer or IEnumVARIANT target. This
does not establish an enumeration protocol, traversal order, ownership, lifetime,
null/error behavior, runtime defaults, ABI or backend support. Frozen independent
declarations and coherent mutation controls require no vendor files or execution.

The core declaration cohort retains 58 getters, 26 setters and 30 functions.
The four previously adopted `IDocument` generation methods are referenced by the
coverage registry, not duplicated. Raw names, spelling and order are literal,
including `GenerateCHashCode`, the unnamed setter parameters and the three
`Test` methods with function flags `0x40`. Eight records have explicitly null
online links: `IApplication.OpenMapping`/`NewMapping`, both
`IOptions.CompatibilityMode` accessors, `IDocument.Test1`/`Test2`/`Test3` and
`IDocuments._NewEnum`. An unmatched name is not an alias or a public-use promise.

`catalog_id` joins the pinned `com-api.json` member without copying its page
contract. Its displayed signature, return/property notation, parameters, access,
constraints and errors, together with catalog global constraints and source
uncertainties, remain separate release-unbound documentation claims. Generic raw
`DISPATCH` is not replaced by a concrete online class; HRESULT/out-retval is not
the same declaration level as an online result. In particular, the online return
of `Application.Quit` is unstated, not inferred as Void. Zero optional-argument
counts and absent parameter optional/default bits are observed declaration bits;
the catalog's null defaults are documentation silence, not runtime defaults.

The shared schema and validator live under
`crates/mfd-conformance/tests/source_inventory/com_declarations/`. They reject
unknown fields and require explicit nulls, exact finite set/order and identities,
owner/table arithmetic, contiguous extents, parameter-tail offsets, signed IDs,
invocation/flag values and bounded acyclic descriptor chains with typed enum or
interface targets. Existing fragments keep their own schemas and independent
checks. The registry verifies their actual record membership and anchors.
Explicit cohort descriptors register the owners, ordinary/event return families,
online owner names and exact five unavailable event bodies. Event handling does
not relax ordinary HRESULT or rendered-body requirements.

The graph cohort retains both in/out `VARIANT* pbError` declarations:
`IComponent.GenerateOutput` parameter 0 and `IComponent.Test1` parameter 1.
`Test1` has function flags `0x40`; the online `GenerateOutput` page instead
displays an out Boolean parameter. The existing partial discrepancy observation
in `artifact-observations.json` is preserved alongside the now-adopted whole
function, not counted again. Raw `Component.ID` is `UINT*`, separately from the
online Long notation. Six new records have no exact online join: both literal
`HighlightMyConnectionsRecursivly` accessors, the `_NewEnum` getters on
`IComponents`, `IMappings` and `ILibraryImports`, and `IComponent.Test1`.
The differently spelled online `HighlightMyConnectionsRecursively` is not an alias.

Project/library collections retain signed `_NewEnum` ID -4 and generic
`UNKNOWN*`, while raw `Application`/`Parent` results remain generic `DISPATCH*`.
All 25 unnamed setter parameters across the four new cohorts remain literal null
names. Diagnostic `GetLineSeverity` returns raw `I4*`, not a named enum target
inferred from prose. Displayed collection indexing, diagnostic separators,
null-delimiter behavior, errors and defaults remain documentation claims or
unstated behavior, not runtime contracts.

The event cohort belongs to `_IApplicationEvents`, `_IDocumentEvents`,
`_IProjectEvents` and `_IProjectItemEvents`, not the owning public dispatch
interfaces. All eight return raw `VOID`, unlike the 317 ordinary HRESULT
declarations. The five document/project/project-item function bodies are absent
from the retained dump. Their required `function_dump_lines` values are null;
header, name and member-table anchors do not substitute for function-body anchors.
The dump renders some of their table words as `unknown`; their line anchors are
joined to independently checked raw table coordinates, not labeled function bodies.

The compact JSONL oracles in the neighboring `oracles/` directory are independent
of the candidate JSON and binary reader: owner/type/descriptor/record tuples
and available coverage identities were transcribed from the retained dump
(`672097183d6a6b0d80f3aa5f92d02172bb3a588609cb743bb82b73c342192bb4`).
The candidate was read separately from the official-release 54,108-byte TLB
(`785c484fbaa882cae724a3568b04650300eb18941084ac23eb2d041dfb4a2789`).
For the five unrendered event bodies only, full literal record words and member
tables were deliberately read a second time directly from the pinned TLB, checked
against the three rendered application-event patterns, and combined with the
dump-transcribed names, descriptors and type headers. This second-read expected
path does not use the candidate decoder or its JSON. Null function anchors are
preserved in both paths. Neither path uses planning coverage JSON as an oracle.
JSONL rows are ordered
`T` type identities, `D` descriptor words, `O` owner tables and `R` callable
tuples; coverage `T` rows instead retain callable/variable counts. The Rust
projection defines each tuple's field order. Parameter tuples retain index,
nullable name, name/record offsets, flags, encoding, pointer depth, leaf target
and descriptor chain. Record tuples retain their source anchors and nullable
catalog identity. These are declaration facts, not copied vendor implementation.

The coverage denominator is 317 ordinary records plus eight event-source records.
The disjoint adopted cohorts are generation (4), results (14), collections (10),
core (114), graph (79), projects (68), diagnostics (28) and events (8). Together
they cover these 325 callable declarations and 392 ordered parameters exactly
once. This closes only the finite function-declaration partition, not the COM
type inventory, product scope or parity. The 75
type-info declarations also include 20 enum tables/130 variable records and 29
zero-function/zero-variable types (23 coclasses, five empty event sources and
`_IDummy`). These are separate metadata, not additional capabilities or proof of
coclass/default-interface/event-source relationship or activation coverage.
Owner TYPEFLAGS are retained declaration bits, not complete owner-type metadata.
Those metadata layers and all platform/ABI/runtime qualification remain separate.

Scalar, unknown-field and coherent mutation controls run with repository files
only; they do not require vendor artifacts, network access or activation. Every
record inherits its fragment's source-only assessment: unassessed, unknown
applicability, empty execution evidence and incomplete inventory. HTTPS acquisition
and byte identity do not verify Authenticode trust. No calling convention, ABI,
marshaling, ownership, lifetime, coercion, defaults, runtime errors, activation or
backend support is qualified by declaration adoption.

The public supporting declarations are inventoried. `schema-wrappers.json`
records bounded source bundles for C#, Java and both C++ DOM options, including
actual schema names and build dependencies. The requested compiled C# DLLs,
complete API contracts and runtime behavior remain unqualified. Running the
generated Java Ant default target executes the mapping; it is not a source-only
inspection. The requests and provenance requirements are listed in `supporting.json`.
Undocumented supporting classes are explicitly implementation-private and
changeable in the manual; including them requires a bounded choice of generation
fixtures and settings, not an assumption that all possible schema-generated
classes form a finite catalog.

Run `cargo test -p mfd-conformance --test source_inventory` to validate finite
coverage, unique source identities and the separation from execution assessments.
The `source_contracts` module also loads the original companion: it checks the
120-page API partition, page/unit cross-references, 752 source units and 789
identity mappings against the canonical ledger. It preserves the two repeated
command placements, seven case-normalized IDs and context-only null IDs.
Mutation controls reject missing/duplicate identities, dangling references,
misclassified pages and unsupported execution, applicability or completeness
claims. These are source-consistency checks, not native API or runtime proof.
Family scopes must match actual source-page placements. Relation edges are checked
against source-unit/workflow records and pinned page/command-use anchors, not just
valid endpoint names. COM member endpoints must also belong to the adopted
`existing_ID_enrichments` set; context-only pages are not member targets.
A separate 171-command numeric oracle rejects coherent value
changes across the redundant identity, contract and placement records.
