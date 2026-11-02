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
| `com-type-relationships.json` | GUID/header enrichment of the same 75 type identities and all 32 ordered coclass references, including nine default sources; no new capabilities or activation proof |
| `supporting.json` | Public I/O classes, wrapper-library exceptions and the named installation-artifact requests |
| `supporting-io-declarations.json` | Direct public source declarations on the existing 24 documented C#/Java I/O owners, with nested C# enums and an independent offline oracle |
| `supporting-exception-declarations.json` | Direct-public source declarations for the same 16 documented exception owners; 28 new callables and two referenced existing declarations |
| `mapping-entry-declarations.json` | Two retained C#/Java mapping-entry owners, seven direct public members and profile-specific anchors, enriching existing supporting contracts |
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
`_IDummy`). These are separate metadata, not additional capabilities or activation
coverage. The supplemental type/relationship fragment enriches these same 75
identities: 22 ordinary dispatch interfaces, nine event dispatch interfaces,
`_IDummy`, 23 coclasses and 20 enums. It joins the existing callable/variable
counts without duplicating their declarations or changing the capability ledger.

The type fragment retains header/name anchors, raw TYPEFLAGS, version words,
`cImplTypes`, `datatype1`/`datatype2`, alignment, declared vtable/instance sizes and
custom-data offsets. All 55 non-enum types have GUIDs; all 20 enums explicitly
have null GUIDs and raw `ffffffff` GUID offsets. The complete 62-record GUID table
also contains the library identity, two import identities and four deliberately
unclassified metadata identities. Those seven rows are not new local types or
capabilities. Library `SYS_WIN64`/4.1 metadata and the `stdole2.tlb`/imported IID
references establish neither platform/ABI qualification nor dependency availability.

Every coclass list starts at its raw `datatype1` segment offset, traverses exactly
`cImplTypes` records and ends at `ffffffff`. The 32 records are reached exactly
once without gaps, shared ownership, cycles or unresolved local targets: 23
`FDEFAULT` (`0x1`) interfaces and nine `FDEFAULT | FSOURCE` (`0x3`) sources. These
are standard IMPLTYPEFLAG declaration labels, not behavioral defaults. Fourteen
coclasses have only a default interface. All ordinary defaults have the same stem,
with the literal `_Dummy` → `_IDummy` exception. Sources include the five empty
AppOutputLineSymbol/AppOutputLines/AppOutputLine/ErrorMarkers/ErrorMarker event
interfaces and the nonempty Application/Document/Project/ProjectItem sources.
Event headers retain `cImplTypes=1` with `datatype1=ffffffff`; that unresolved raw
distinction is not replaced with the ordinary/dummy `datatype1=1`, and no event-base
edge is invented. Dummy/source headers do not establish intended public use.

**Dump hazard:** relationship evidence comes from global RefTab hex rows at dump
lines 2615–2646, anchored by segment-relative ordinal. Their printed first address
`0000280c` is displaced from the actual segment start `0000260c` (9740); it is not
a binary position. Late `TypeInfo`/`RefRecord` renderings are invalid list evidence:
lines 15148–15161 contain event words and lines 15239–15254 contain `ffffffff`
renderings. Neither substitutes for the authoritative global rows. Type headers
use lines `194+28*i`–`221+28*i`; GUID rows use `2304+5*i`–`2308+5*i`; name rows use
`2693+6*n`–`2698+6*n`. These uniform anchors were independently checked against the
pinned dump, including all 453 name records and their bounded binary counterparts.

The type fragment's `columns` legend defines its compact arrays. Its separate
`oracles/com-type-relationships.jsonl` is a dump-only transcription, not output
from the candidate JSON, candidate binary reader or existing coverage inventory.
`T` tuples retain index, name, kind, name/GUID offsets, flags, version,
`cImplTypes`, both datatype words, function/variable counts, header-extra tuple and
name-record index. `G` tuples retain index, GUID and both raw reference/hash words;
`R` tuples retain segment offset and all four literal global-row words. The
validator independently checks deterministic anchors, nullable GUID joins, target
names/GUIDs, owner ordinals and complete list reachability. Coherent GUID, same-kind
target and list-relinking mutations remain structurally consistent but fail the
frozen source oracle. All checks use repository files only, without vendor files,
network access, native COM execution or activation. Neither declaration wiring nor
raw sizes qualify ABI, marshaling, ownership, lifetime, defaults or runtime behavior.

Scalar, unknown-field and coherent mutation controls run with repository files
only; they do not require vendor artifacts, network access or activation. Every
record inherits its fragment's source-only assessment: unassessed, unknown
applicability, empty execution evidence and incomplete inventory. HTTPS acquisition
and byte identity do not verify Authenticode trust. No calling convention, ABI,
marshaling, ownership, lifetime, coercion, defaults, runtime errors, activation or
backend support is qualified by declaration adoption.

The supporting-I/O declaration fragment enriches the same 24 research identities
from `supporting.json`, twelve owners per language. Its finite source cohort is
`Altova/io.cs` and twelve `com/altova/io` owner files. The ordinary and
wrapper-enabled retained bundles have identical bytes for all thirteen files.
The recorded generation manifests join the selected generator and four
ConvertProducts inputs to the existing HTTPS-origin installer-member comparisons;
this does not strengthen Authenticode trust or establish product-wide provenance.

The fragment retains 99 directly declared public non-type members: 26 constructors,
35 methods, 31 properties, one field and six Java constants. Two nested C# enum
declarations and their six constants remain owned by `Input` and `Output`, not new
research identities. Type spelling and resolved names are separate, including
`System.IO.TextReader` versus `java.io.Reader` and the nested enum types. Explicit
modifiers, getter visibility, overloads, parameter names/directions, absent declared
defaults (explicit null metadata) and Java throws remain source metadata. The literal
C# `Fielname` spelling is not corrected. Inherited members, implicit constructors, nonpublic declarations,
UrlInput/Web, mapping-entry and close-setting APIs, exceptions and schema-generated
types are excluded. An override is retained where directly declared, without
duplicating the rest of its inherited surface.

One-based LF declaration ranges, zero-based file/type/member/parameter indexes and
import anchors refer to freshly hashed private sources. Type indexes count excluded
top-level types as well; member indexes count only the scoped direct public value
members, with nested types listed separately. Accessor anchors identify declaration
tokens, never property bodies. No preprocessor directives occur in these thirteen
files; an empty condition list does not establish compiled availability or a default
build configuration. Literal constant values are declarations; method/property/
constructor bodies, comments, constructor initializers and nonconstant field
initializer expressions are not copied. The public field's value status is explicitly
`not_established`; its null constant-value slot does not claim a null runtime value.

`oracles/supporting-io-declarations.jsonl` is an independent manual transcription
from numbered source reads and separate manifest/hash readback, not output from the
candidate lexer, candidate JSON or partial discovery caches. `S` rows retain file
identity, byte hash, LF extent, imports and conditions; `O` rows retain owner/type
index, declaration anchor, modifiers and explicit inheritance; `N` rows retain nested
enum ownership and constants; `M` rows retain direct members, ordered parameters,
throws, accessors and explicit constant values. The Rust projection defines each
tuple's field order. A strict typed schema rejects unknown fields and missing
explicit nulls. The source oracle and coherent mutation controls pin every owner and
member, including same-shape changes to names, types, throws, accessors, source
coordinates, hashes, imports, nesting and inherited-member promotion.

The manual's Java `StreamOutput(String filename)` stays separate from the observed
public `OutputStream` constructor. This is not an automatic contradiction or a
correction to the manual. The protected filename constructor is outside this public
cohort. Existing partial observations in `artifact-observations.json` are compared,
not rewritten. Runtime ownership, closing/lifetime, buffering, encoding, defaults,
null/error behavior, dependency availability, exact Java version, ABI and backend
applicability remain unqualified. All owners inherit unassessed status, unknown
applicability and empty execution evidence/receipts; the inventory remains incomplete.
These checks need only repository metadata, not vendor files, network access or
vendor generation, compilation, activation or execution.

The exception-declaration companion enriches the existing ten C++, four C# and
two Java research IDs. Its finite direct-public cohort has 24 constructors, two
destructors and four methods, with 43 ordered parameters and nine literal `0`
default arguments. The two C++ `inner` declarations reference
`installed.cpp.exception-inner`; the Java source-instance class references
`installed.java.source-instance-unavailable`. These existing observations are not
adopted again. Empty fields, properties, accessors, nested types, member conditions
and Java `throws` lists are explicit. Inherited and compiler-implicit members are
excluded, as are the nine platform references. Other public source exceptions
(`altova::TargetUpdateFailureException`, `altova::UserException` and
`Altova.Types.TargetUpdateFailureException`) are outside this documented cohort,
not absent or private.

Six name correspondences are not aliases: documented C++
`InvalidArgumentsException` corresponds to source `InvalidArgumentException`, the
four documented C# `Altova` owners are declared in `Altova.Types`, and documented
Java `SourceInstanceUnvailableException` corresponds to source
`SourceInstanceUnavailableException`. Manual names and shared-method claims remain
unchanged and separate. C# bases and parameters retain literal `Exception` with
their `using System;` / `using Altova;` context, not inferred fully qualified
bindings. Java retains literal `RuntimeException` bases and no declared `throws`;
this does not prove runtime error absence. Four base-literal differences remain
explicit: both C# DataSource/DataTargetUnavailableException manual bases say
`System.Exception` while the source says `Exception`; both Java manual bases say
`Exception` while the source says `RuntimeException`. Neither spelling comparison
establishes name binding or equivalence. C++ `throw()` and `const` are literal
qualifiers, not platform guarantees. The global `string_type` typedef retains both
`std::wstring` under `defined(UNICODE) || defined(_UNICODE)` and the `std::string`
alternative, their inclusion guard and LF line anchors. No compiled configuration
or ABI is selected.

The companion pins generated-source hashes to the retained schema-wrapper
manifest, four unchanged generation inputs and the selected official-release
installer templates. The two C++ DOM-profile exception headers are byte-identical;
installed-template comparisons preserve the generated prologue and C# template
bracket-unescaping distinctions. HTTPS acquisition and byte identity do not verify
Authenticode integrity or trust. No vendor implementation, constructor initializer,
comment example or vendor source file is included. Returned messages, inner-object
ownership, lifetime, errors, defaults, dependency availability and language versions
remain unqualified.

`oracles/supporting-exception-declarations.jsonl` was manually transcribed from the
actual retained exception sources and `Altova/Altova.h` before the separate private
candidate extractor. Readiness outlines were discovery aids, not expected data.
The repository-only projection uses `S` source/context tuples, `O` owner/class
tuples, `M` ordered new declarations, `R` existing-declaration references and `A`
conditional typedef tuples. Nullable class payloads reference the existing Java
observation; referenced C++ methods carry no second declaration payload. Strict
key checks require every field, including explicit nulls, and reject unknown keys.
Independent exact-set/order checks and coherent mutations freeze overloads,
parameters, defaults, qualifiers, attributes, imports, conditions, hashes, anchors
and reference joins without vendor files, network access or execution. Every record
inherits the companion's unassessed status, unknown applicability, empty execution
evidence/receipts and incomplete inventory.

The mapping-entry fragment covers only the retained one-input/one-output ordinary
and wrapper-enabled C#/Java profiles. It joins the existing
`io-substitution-workflow`, `io-default-close` and `mapping-entry-contract`
identities through exact `supporting.json` pointers; it creates no research or
capability identity. Its mapping class names identify source owners, not copied
business-schema shapes. The four mapping files have different ordinary/wrapper
bytes but matching scoped public integration signatures. Each source keeps its
own hash, imports, namespace/package, LF extent, class header/closing brace and
member/accessor declaration-token anchors. LF extents include the final empty
split item. C# region directives are layout, not conditional-availability claims.
The older ordinary manifest retains its earlier unestablished-provenance wording;
the later selected-release byte comparisons are a separate joined source, not a
rewritten manifest or product-wide provenance finding.

There are two C# `Run` overloads and a `bool CloseObjectsAfterRun` property with
separate public `get`/`set` metadata; Java has two `run` overloads plus close
getter/setter methods. Together they retain nine ordered parameters and two literal
Java `throws Exception` clauses. The explicitly declared constructor lists are
empty: this does not claim that compiled constructors are absent. Inherited
TraceProvider APIs and public members inside nested nonpublic implementation
classes are excluded. No bodies, private initializers, comments, examples or
business-schema members are retained. The property name does not establish a
close default, ownership, lifetime or close-on-error behavior. The manual's close
usage/default claims remain separate and unqualified.

Literal type spellings and import context are preserved. Language keywords and
explicitly qualified names have declaration-level bindings; unqualified `String`
and `Exception` retain explicit null bindings rather than resolving potentially
ambiguous lookup without compilation. Fully qualified I/O parameter types join
the existing four public base-owner records as facts, not proof that dependencies
are available. Exact Java/CLR versions, ABI, Authenticode trust, dependencies,
backend applicability and runtime behavior remain unknown.

`oracles/mapping-entry-declarations.jsonl` was manually transcribed and frozen
before the separate brace-aware private candidate lexer was written. It uses no
candidate JSON, lexer output or cached outlines. `P` rows pin finite profiles and
manifest/version/settings joins; `S` rows pin each source and import context; `O`
rows pin class ownership, profile headers/closures, explicit inheritance and empty
constructor lists; `M` rows pin direct members, parameters, types/bindings, accessors,
throws and per-profile anchors. The Rust projection defines tuple order. Strict
schema, source-oracle and coherent mutation checks use repository files only,
without vendor files, network access, generation, compilation, activation or
execution. Status stays unassessed, applicability unknown, execution evidence and
receipts empty, and inventory completeness false. This finite source snapshot
does not establish all possible generated mapping APIs or a multi-target contract.

`schema-wrappers.json`
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
