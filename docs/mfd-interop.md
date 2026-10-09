# `.mfd` Interoperability

ferrule provides clean-room, best-effort import and export for
`.mfd` mapping designs. Vendor samples may be used as black-box behavioral
references, but ferrule's implementation and committed fixtures are original.

## Import

```sh
cargo +nightly run -p cli -- import-mfd --mfd design.mfd --out project.json
```

For a single project that must pass static execution admission, add
`--require-executable`:

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd design.mfd --out project.json --require-executable
```

This selects `ImportProfile::Executable`: import warnings, unresolved runtime
dependencies, and engine validation findings reject before the destination
project is created or replaced. Ordinary import retains a partial design for
repair. The flag preserves legacy global failure-rule ordering; row-error-order
semantics require the separate `--item-ordered-exceptions` option. The two flags
may be combined. `--require-executable` applies to single-project imports and
cannot be combined with `--pipeline`. Static admission does not execute the
mapping or establish behavioral equivalence with another application.

For a mapping package whose resources sit above or beside the design directory,
declare the trusted package root:

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd package/maps/design.mfd \
  --package-root package \
  --out project.json
```

For a relocatable package, put `ferrule-package.json` at its root:

```json
{
  "schemaVersion": 1,
  "kind": "ferrule.mapping-package",
  "catalogs": [
    { "kind": "edi-config", "root": "resources/edi" },
    { "kind": "json-schema", "root": "resources/json-schema" }
  ]
}
```

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd package/maps/design.mfd \
  --package-manifest package/ferrule-package.json \
  --out package/projects/project.json
```

When no `--package-root` or `--package-manifest` is supplied, import searches
the design's ancestor directories for the nearest `ferrule-package.json` and
uses that manifest. Passing `--package-manifest` still selects a specific
manifest explicitly, and passing `--package-root` keeps the package root as a
host-provided trust boundary without reading a manifest. A mapping cannot grant
itself arbitrary filesystem access: the manifest's directory is the package
trust boundary, and catalog entries are relative, traversal-free directories
inside that boundary. Direct catalog flags are searched before manifest
catalogs. The GUI stores an explicitly selected manifest as a host preference,
never in the mapping project. `--package-root` and `--package-manifest` are
mutually exclusive.

Resource references accept both slash styles and may contain parent components
when their canonical target remains inside the package. Symlink escapes,
absolute Windows paths, ambiguous case-insensitive matches, and traversal above
the root are rejected.
Every local XSD opened during mapping import uses that package root, including
`xs:include` and `xs:import` chains, named-type and base-type lookup, selected
wildcard roots, and conditioned `xsi:type` refinement. This applies to ordinary
XML components, typed HTTP and WSDL bodies, and XML-valued database columns.
Embedded function parameters, lookup catalogs, and structured or recursive
definitions carry the same resolver, including definitions that a recipe
recognizer rejects or that the primary graph does not call. Refusals remain in
the mapping's diagnostics when a recognizer discards its local fallback warnings.
Contained transitive absolute paths, parent aliases, and symlinks retain their
types; a canonical target outside the authorizing root is refused before its
contents are opened. The standalone `format_xml::xsd` import APIs retain their
host-owned resource behavior; their `*_with_resource_root` variants select this
explicit boundary.

Adjacent C#, Java, XQuery, and XSLT extension modules use the same canonical
package boundary before bounded source decoding and recipe recognition. A
missing candidate permits ordinary discovery to continue; an existing escaping
or dangling symlink produces an explicit diagnostic. Contained file and
directory symlinks remain valid. Resource refusals remain visible in best-effort
import warnings and cause the executable import profile to refuse publication.
Unavailable resources retain the component's established diagnostics and fallback
rules. A missing optional database file with complete embedded column types does
not become a boundary denial; an existing canonical target outside the package
does.
Canonical checks do not isolate the host filesystem or prevent a concurrent
filesystem change between resolution and opening. Hosts performing offline
qualification use operating-system confinement with read-only input mounts and
disabled outbound network access when those guarantees are required.

The authored resource-boundary fixtures live in
`crates/format-xml/tests/resource_boundary.rs` and
`crates/mfd/tests/resource_boundary.rs`; rejected and unused embedded function
controls live in `crates/mfd/tests/udf_resource_boundary.rs`. XML-column and
conditioned-type refinement controls also run in the MFD library tests. Run the
integration cohort with
`cargo +nightly test -p format-xml -p mfd --test resource_boundary` and
`cargo +nightly test -p mfd --test udf_resource_boundary`.
Each fixture retains its authored temporary tree and complete schema/import
outcomes before assertions, including failed runs, and prints the evidence
location. Use an owned `TMPDIR` for repeatable collection, and copy those
artifacts outside the repository before interpreting them.

External FlexText `.mft`, visual PDF `.pxt`, and XBRL `.sps` compiler inputs
use this same package boundary. Their compiled layouts or fact metadata are
embedded in the imported project, and FlexText data paths remain portable
relative to the mapping even when the configuration is in a sibling directory.
Protocol Buffers components likewise resolve their declared root through the
package boundary, accepting Windows separators and safe parent traversal. The
selected package root is the virtual include root; exported `*-protobuf`
directories remain narrower self-contained include roots. Every reachable
`.proto` is loaded with bounded canonical containment and embedded under its
portable logical path, so execution and later export do not reopen the package.
Typed HTTP response XSDs, HTTP POST request JSON Schemas, and WSDL message XSDs
also resolve from the package boundary. Local references below an HTTP request
JSON Schema remain confined to the root that authorized the schema; endpoint
URLs, preview instance paths, and retained WSDL service contract locators are
host metadata and are not rewritten as package resources.
Target `cast-in-subtree` and target node-function metadata use the same bounded
resolver and UTF-8/UTF-16 decoder when they inspect a component XSD after target
construction. DTD targets are accepted without applying XSD-only scalar type or
facet annotations. A missing, oversized, malformed, or canonically escaped
schema produces an actionable postprocessing warning and leaves the resilient
entry-tree mapping intact instead of reopening the resource outside the selected
package.

If EDI configurations live in a separately managed release catalog, declare
each trusted catalog in search order:

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd package/maps/design.mfd \
  --package-root package \
  --edi-catalog-root edi-configs/current \
  --edi-catalog-root edi-configs/archive \
  --out project.json
```

Package-contained configurations take precedence. Catalog lookup accepts
portable and Windows-style locators, including leading installation-relative
parent components, but re-anchors them under the declared catalog instead of
performing filesystem traversal. Direct files and bounded adjacent ZIP packages
must remain canonically contained in that catalog.
Every transitive EDI include, selected message configuration, and SWIFT common
definition remains confined to the root that authorized the main configuration.

Separately managed JSON Schema catalogs use the same explicit ordered trust
model:

```sh
cargo +nightly run -p cli -- import-mfd \
  --mfd package/maps/design.mfd \
  --package-root package \
  --json-schema-root schemas/current \
  --json-schema-root schemas/archive \
  --out project.json
```

Package-contained schemas take precedence. A catalog lookup normalizes Windows
separators and re-anchors only leading installation-relative parent components
under each catalog root. Parent traversal after a real path segment, absolute
paths, ambiguous case-insensitive matches, and symlink escapes reject. Once the
root schema matches a catalog, its nested local `$ref` graph remains confined
to that same canonical root.

### Read-only parity surveys

The import/export, execution, and export/re-import execution surveys accept the
same host-selected package manifest and ordered catalog roots as `import-mfd`.
Set `FERRULE_MFD_SURVEY_PACKAGE_MANIFEST` to the manifest file. Set
`FERRULE_MFD_SURVEY_EDI_CATALOG_ROOTS` and
`FERRULE_MFD_SURVEY_JSON_SCHEMA_CATALOG_ROOTS` to platform path lists; roots
retain their declared order after canonical deduplication. When no manifest is
selected, the `ReferenceSamples` directory remains the package root.

For example, on Unix:

```sh
FERRULE_MFD_SURVEY_PACKAGE_MANIFEST=/path/package/ferrule-package.json \
FERRULE_MFD_SURVEY_EDI_CATALOG_ROOTS=/path/edi/current:/path/edi/archive \
FERRULE_MFD_SURVEY_JSON_SCHEMA_CATALOG_ROOTS=/path/json/current:/path/json/archive \
cargo test -p mfd --test samples_survey -- --ignored --nocapture
```

The same variables apply to `samples_execution_survey`, including its
`survey_sample_execution` and `survey_export_reimport_execution` entry points,
and to `codegen_samples_survey`. Execution reports explicitly use
`execution_purpose: design_preview` (schema version 3); export/re-import
execution reports use the same purpose (schema version 2). Unsupported preview
contracts withhold reference and semantic-match claims even when a partially
imported graph can execute. Other version-1 reports add an optional
`resource_configuration` member. It records
selection mode, effective catalog counts, and search precedence without
disclosing package, manifest, or catalog host paths.

Import resolves the supported component graph into ferrule schemas, graph
nodes, scopes, format options, and endpoints. Current coverage includes common
XML, JSON, CSV/fixed-width/FlexText, XLSX, SQLite, EDI, Protocol Buffers, XBRL,
HTTP XML, and visual PDF source components, together with a broad set of scalar
functions, aggregates, sequence controls, lookups, joins, exceptions, and
recognized user-function shapes. Adjacent XSLT extension modules also import
when a named one-parameter template returns a direct count, sum, average,
minimum, or maximum over a descendant path; ferrule lowers that template to a
native aggregate rather than retaining an XSLT runtime dependency.

Nested cloned target groups retain their declared ancestor entry ownership.
Child branches attach to that exact owner in declaration order, including
unequal branch counts; source anchors and branch conditions remain relative to
the owning parent. Redistribution requires unique explicit ancestor lineages,
a nonempty child partition for every parent, plain concatenation wrappers, and
exact provenance for every binding in a replaced placeholder. Each original
binding must survive once in its assigned descendant subtree, and each retained
binding must identify one original binding index. Identical cached expression
nodes alone do not prove that separate original bindings survived. A placeholder
with unclaimed, missing, broadcast, ambiguous, or controlled content remains
intact with a refusal diagnostic. A unique child entry may reuse its parent's
marker lineage when its deeper declared target path establishes containment;
retained provenance is
restricted to that descendant path. Numeric port order, equal branch counts,
and similar source paths
do not establish ownership. Direct IR without declared entry owners remains
subject to the ordinary concatenation validator. Multiple distinct feeds to a
singular target retain their duplicate-binding refusal; their selection
semantics are tracked separately in [#241](https://github.com/DeandreT/ferrule/issues/241).

Bounded adjacent C# and Java source modules can likewise lower direct numeric
picture wrappers to ferrule's deterministic formatter, while bounded XQuery
modules can lower scalar parameter/number arithmetic to the native call graph.
Structured XML string serializers retain the selected subtree schema and emit
attributes, nested groups, repeated children, escaping, the configured default
namespace, and optional XML declaration directly from the current source item.
Imported XSD contracts expand bounded named model and attribute groups, retain
typed scalar element/simple-content/attribute defaults, and materialize those
defaults at the XML input boundary.
Singular optional XML elements retain their declared occurrence through project
serialization and XSD export. This metadata does not change the existing lenient
instance reader or make JSON properties optional. Optional compositor wrappers
are accepted only when their complete content can be represented exactly;
correlated required members produce a typed schema diagnostic and retain the
complete entry-tree fallback during best-effort import.
Concrete declared XML default types also survive schema serialization. XML
output omits an invented `xsi:type` when the runtime value already uses that
declared type, while explicit type markers remain intact. Abstract, unknown,
malformed, and overridden declarations do not infer a default from the first
available alternative.
Optional sequence wrappers normalize only when every supported member already
accepts absence. Unbounded ordered choices can remove an empty branch without
changing their nonempty member order; bounded or correlated cases still reject.
Empty XML groups and groups containing only attributes serialize without
indentation text inside the element. Empty numeric, boolean, and nonempty fixed
text retain their typed conversion errors.
Structured XML database columns reuse that typed serializer with compact output,
so document-valued TEXT fields execute without flattening the source subtree.
Their declared root XSD or DTD accepts portable Windows separators and safe
parent traversal within an explicitly selected package, while canonical
symlink escapes reject with an actionable column warning. The imported schema
is embedded in the project, so execution remains independent of the original
root schema file.
When one compact, declaration-free serializer feeds only a plain SQLite XML
column, strict native export restores the column's `doc-xml` structure and
writes an adjacent XSD. The local XML-to-SQLite sample reimports and stores
the same XML payloads after export. Serializers with another consumer or
nontrivial row controls keep their ordinary component and retain explicit
native-profile diagnostics.
For a single SQLite table mapped directly to XML rows, a guarded `LIKE`
predicate with a literal ASCII prefix and one trailing `%`, null guards, and
sorting by that same string column exports as the native database `where`
control. Its parameter expression stays connected, including an optional named
string input with a literal prefix default. Computed `concat`/`string` row
bindings and related source fields remain wired. The local phone-list design
reimports to identical XML with four default rows and three rows when the host
supplies `NamePrefix=F`. Other `LIKE` patterns,
shared predicate nodes, or additional row controls retain the ordinary
diagnostic; the local database-filter sample reimports without warnings and
produces the same output after strict export.
The interpreter and generated Rust/C# `sql_like` calls compare text before the
first NUL, fold ASCII case only, and treat `_` as one Unicode scalar. Patterns
are limited to 50,000 UTF-8 bytes before NUL truncation, matching the bundled
SQLite limit. Ferrule also limits matching to 100 million value-scalar by
normalized-pattern-scalar cell updates. Over-budget calls report typed errors.
This work bound deliberately rejects some costly SQLite matches. An oversized
pattern on a null source row still differs: the imported null guard skips the
call, while SQLite reports a pattern-length error.
An isolated canonical decimal string feeding the second input of a supported
numeric comparison or addition can export through a native decimal input
component. The local price and temperature samples reimport with identical
XML. The imported component names `Markup`, `lower`, and `upper` survive
project serialization and strict export. Named optional native inputs now retain
their connected default expression and scalar type. The host value wins when
the name is supplied; otherwise the default evaluates lazily. Explicit null
does not select the default. String, integer, and decimal input tests cover
strict warning-free export/reimport, overrides, and typed errors. Required and
connected-default inputs retain enabled preview text separately, including
empty and malformed lexical values. Input preview settings
define those values as design-time only. `ExecutionPurpose::Preview` selects
saved preview text only when no host value was supplied; it precedes a connected
default. Explicit host null still wins. Ordinary runs and generated Rust/C#
ignore preview metadata, so required inputs remain required and connected
defaults stay lazy. The GUI preview and debugger use that explicit purpose;
normal file and pipeline runs keep the runtime contract. Native export restores
the preview settings, and the node editor can enable and edit them. Invalid
preview text reports a typed error instead of becoming null or silently using
the default. Optional inputs without a connected default remain unsupported,
whether or not they have preview text: the documented settings do not define
their omitted-value result. Import skips dependent bindings and iterations with
diagnostics, and the executable import profile rejects them. Unnamed preview-only
inputs also remain unsupported. Execution surveys withhold reference and semantic
matches for these unknown input contracts. Old serialized graphs that already
lost preview provenance cannot reconstruct it.
For the exact order-pricing graph, strict export restores the native decimal
source rules on two XML price leaves. Anchored repeated-row reads apply those
rules on reimport, preserving all three local CSV rows and the original typed
conversion error for nonfinite hand-built inputs. Changed arithmetic, shared
conversions, string leaves, and non-XML sources remain explicit blockers.
Four local employee and manager mappings also reconstruct one native database
`where` control for a guarded `%Word%` title predicate. Nested relation
selection and an independent department or office filter stay connected,
including typed optional pattern and selection inputs;
strict reimport reproduces 8, 8, 2, and 5 XML rows respectively. Other
patterns, shared predicate nodes, row controls, and changed field types reject.
The guarded `concat(prefix, "%")` family also retains required String host
inputs and their preview metadata. Two strict self-authored roundtrips cover
normal missing-input errors, preview and host overrides, and explicit null.
Null stays supplied; the existing concat semantics leave `%` and therefore match
all present text rows. The local prefix-filter tutorial exports strictly again.
Self-authored joined SQLite query tests reconstruct a native `SELECT` with a
declared foreign key, numeric threshold filter, and computed column, including
typed overflow behavior. The local `Tutorial/select-component.mfd` design has a
text-declared required input compared to numeric `Quantity`; that cross-type
query is now diagnosed and skipped rather than freezing its preview `2` as a
runtime threshold. Its former local row match does not establish compatibility.
Exact same-type optional query inputs retain host overrides rather
than freezing the default. A guarded joined integer comparison exports its
optional input through a declared native SELECT parameter; synthetic tests
cover preview and connected defaults, overrides, nulls, and typed errors.
Required inputs also retain exact same-type query parameters and preview metadata,
including native joined integer SELECT thresholds and title WHERE patterns.
Nonempty source rows require a supplied value; empty sources retain lazy reads.
Ordinary execution requires host values for those required inputs; design
preview may use the saved text. Optional inputs without defaults and cross-type
dynamic query coercion remain unsupported. In particular, a text-declared
parameter compared to a numeric column is diagnosed rather than freezing its
preview into the runtime query. A filter whose predicate could not import also
skips its dependent iteration instead of producing unfiltered rows.
If the database cannot be
resolved for the read-only foreign-key check, the exporter leaves its internal
function and reports the native limitation.
Nonempty numeric `IN` and `NOT IN` lists retain literal SQL `NULL` members.
`IN` keeps a matching non-null member even when another member is null; an
absent or null column/list operand excludes the row for `NOT IN`, preserving
SQL WHERE behavior when equality results are negated. Membership trees pair
adjacent operand ranges, retaining eager left-to-right host reads while keeping
all 256 supported members within export expression-depth limits. A quoted
string `'NULL'` remains a string operand and must pass numeric coercion; a
connected static null parameter remains unsupported. Late coercion failures
retain a partial repair design with a diagnostic and skip the dependent
iteration; executable import rejects that design. Query boundaries explicitly
qualified as SQLite by both `database_kind` and `import_kind` metadata also accept
empty `IN ()` and `NOT IN ()` lists: they reject all rows or retain all rows,
respectively, including rows with null values. The column must still exist and
be scalar. Empty membership needs no text collation, so it also works on text
columns; nonempty text membership with unknown collation and quoted identifier
operands remain unsupported. Constant filters retain source iteration, ordered
row controls, and eager reads in unrelated conjunction predicates through both
export profiles. Other database dialects are not inferred.
Two temperature designs recover their original native numeric wiring. The
annual PDF mapping retains all eight conversions inside its user function and
strictly reimports its native-shaped PDF extraction template to the same 148
CSV rows. The grouped XML mapping restores 21
target-side and six user-function conversions through an exact descendant
node-function rule; its five yearly rows and typed nonfinite conversion error
match after strict export/reimport. The recovery requires the closed source,
target, graph, and scope shapes of these two designs; changed numeric calls,
shared branches, or added row controls keep the explicit incompatibility.
Four bounded recursive construction families export as synthesized native user
functions: one repeated string path list builds a directory/file hierarchy,
one flat key/parent catalog builds a recursive adjacency tree, and a same-shape
directory tree filters its direct files at every level, and one recursive
directory collector emits a flat list of file paths. These require
plain XML boundaries and exact supported schemas and scopes. Local strict
round trips preserve the 16-directory/90-file tree and the 49-type hierarchy
byte for byte. The recursive filter retains all 16 directories and 33 `.xml`
files by default; a supplied `.xsd` search retains 16 files, with matching XML
before and after reimport. The flat collector preserves all 90 paths and
direct-file-before-child ordering, including interleaved XML input. Original function names and canvas layouts are not retained by
these normalized constructions; duplicate/cycle behavior in the native
application remains unverified.
Native XML date-time target casts can also be reconstructed in nested and
repeated target scopes when every selected leaf and graph consumer satisfies
the closed-schema guards. Invalid lexical values retain the same typed local
failure after reimport. This removes the IDoc sample's internal cast component;
its separate EDI schema/layout blockers remain.
A bounded strict IDoc descriptor parser also retains supported group, segment,
field, cardinality, and code-list metadata. Its canonical rendering reparses
identically and projects to the same legacy schema and fixed-record layout.
Unknown syntax leaves legacy import executable without certification. This is
configuration groundwork: certified descriptors now survive project format
options and versioned Ferrule design metadata only when their projected schema
and layout exactly match the executable boundary. External configurations stay
authoritative. Unknown versions, duplicate metadata, altered schema/layout, and
missing layouts discard certification with a warning. Export also renders a
certified configuration as an adjacent, uniquely named file with a relative
reference. Rendering reparses and compares the entire descriptor before
returning text; labels that would lose leading whitespace reject with the
affected group, segment, field, or code path. MFD export propagates this error
before publishing any file. Self-authored relocation tests reimport it without the original
configuration. A closed native text-settings model also retains the observed
encoding/order codes, BOM and termination flags, fixed syntax/separators,
auto-completion flag, and all 16 ordered validation actions. These codes are
preserved without inferring their byte-level meaning. Unknown, duplicate, or
incomplete settings discard settings certification with a warning; engine
validation requires the paired IDoc descriptor, schema/layout, and matching
auto-completion flag. Native schema/layout/settings compatibility remains open.
The explicit `format_edi::idoc::validate_native` and descriptor-aware read/write
APIs check per-parent occurrence limits and present scalar code values under
separate work, text, and diagnostic budgets. Ferrule accepts absent OPTIONAL
nodes and applies LOOPMIN when present; MANDATORY nodes require at least one.
This is an explicit validation policy, not a native acceptance result. Legacy
layout-only APIs and CLI behavior retain their existing policy; the retained
native validation actions are not yet applied or behaviorally verified.
SQLite `LocalRelationsStorage` declarations are retained as exact typed relation
endpoints, validated against the physical columns, and exported canonically. This
keeps nested relational reads executable when the database omits foreign-key metadata.
Filter components downstream from grouping retain their operator order: a
group survives when any member satisfies the predicate, and sparse typed member
ports resolve within that retained group.
External EDI configurations may be ordinary package resources, explicitly
trusted catalog resources, or adjacent ZIP packages. Packages are extracted
under strict path, entry-count, compressed, and expanded-size limits; the
resulting X12/EDIFACT schema and lexical metadata are embedded in the imported
project, so execution and later export do not depend on the package or catalog
remaining available.
When an external EDI configuration cannot be resolved, its original reference
is retained for `.mfd` export and re-import instead of being discarded. That
keeps the design round-trippable, but the boundary remains explicitly
non-executable until the referenced configuration is supplied and compiled.
An EDI boundary with neither a compiled configuration nor an embedded typed
layout is also retained as a distinct typed missing-configuration dependency.
It never borrows the untyped entry tree as an executable schema. Export and
re-import preserve that state without inventing a resource path, and CLI or
payload execution rejects it before publishing any output.
Zero-input `create-guid` generator components execute in the interpreter and
generated Rust/C# mappings and round-trip as native `lang` components. Scalar
and record-producing filter lookup UDFs accept typed XML, EDI, or database
inputs.
The exact decimal divide/one-place round UDF pattern fed only by implicit
Protobuf `float` fields exports with native arithmetic wires. Its internal
numeric conversions are identities for the finite float32 values accepted by
the Protobuf file boundary; a string, runtime parameter, changed arithmetic
pattern, or shared conversion keeps the explicit native-profile blocker. This
claim concerns file-backed Protobuf execution. Ferrule projects reimported from
the native design retain conversion errors for hand-built nonfinite instances.
Scalar and nested scalar UDFs can also tokenize text, split by fixed
length or bounded regular expressions, or generate an inclusive integer range,
then select one 1-based item, test a filtered sequence for a match, or apply
count, sum, average, minimum, maximum, or string-join to raw, filtered, or
per-item computed generated values. Import lowers those compositions to
ferrule's native generated-sequence reducers, so interpreter execution,
Rust/C# generation, and export/re-import share one bounded implementation.
Filtered predicates and computed values can use the generated item's 1-based
`position()` as well as the item value.

Import is deliberately resilient: unsupported constructs are skipped with one
actionable warning where possible. A design is rejected only when no usable
source or target can be recovered.

A connected target that also supplies a later target cannot be represented as
two independent targets in one `Project`. Ordinary `import-mfd` reports that
loss, and the executable import profile rejects it. For a connected XML design
with up to 64 serial pass-through targets, import the design as a typed
pipeline instead:

```sh
cargo +nightly run -p cli -- import-mfd --mfd chained.mfd --pipeline --out flow.json
```

Each computed intermediate target supplies the next stage's source, with
original source components bound as host inputs. In a bounded XML chain of at
least three stages, one nonadjacent earlier intermediate may also feed a named
source of the final stage while its immediate predecessor remains the primary
source. The final intermediate may feed one default and up to 255 connected
named XML targets. The import validates
the whole stage graph before writing `flow.json`; other intermediate branches,
cycles, bypasses, and disconnected XML boundaries reject explicitly. See
[mapping pipelines](mapping-pipelines.md) for input binding and execution.
The GUI can preview or debug the saved pipeline without publishing outputs.
Its bounded byte host retains typed intermediate results and returns each
stage's primary and named artifacts with stage-qualified traces. This local
workflow does not establish reference-application acceptance of exported designs.

Static source, target, named-source, and named-target paths are rebased when
the generated project is written somewhere other than the design directory.
HTTP URLs and graph-computed paths are unchanged. Moving or using Save As on a
project applies the same rebasing rule, including wildcard input paths.

### XML root-view interoperability

A bounded flat String-attribute profile supports one local XML source and one
local XML target. Both schemas must declare the same selected nondefault type
and exact root identity. Each output field uses the same observed primary-root
type condition, a direct source-root attribute read, and an absent false branch.
The target type is one unconditional declared constant. Attribute requirements
must agree with the source schema, and every graph node must belong to these
projections. Shared source fields can feed multiple target attributes.

Import and export retain the type condition, required reads, physical namespace
identities, and direct base-root selection. Other root-expression graphs,
additional document boundaries, failure rules, user functions, and explicit
schema hints remain outside this `.mfd` profile. Unsupported export fails before
publishing artifacts; unproved import shapes retain actionable diagnostics.

## Export

Ordinary and connected `.mfd` designs share a 64 MiB UTF-8 byte limit. Import
checks file size before reading and bounds the actual read; export checks the
complete XML, including escaping and embedded metadata, before publishing any
design or schema sibling. Each connected stage and the combined design must
fit the same limit.
MFD entries and generated XSD attributes preserve tabs and line breaks through
XML character references, including field names, paths, fixed values, and
defaults.

```sh
cargo +nightly run -p cli -- export-mfd --project project.json --out design.mfd
cargo +nightly run -p cli -- export-mfd --project project.json --out design.mfd --profile native-mfd --check --report-json
cargo +nightly run -p cli -- export-mfd --project project.json --out design.mfd --profile native-mfd
cargo +nightly run -p cli -- export-mfd --project flow.json --pipeline --out chained.mfd --profile native-mfd
```

The native editor exposes both profiles under File: “Export MFD (Ferrule)...”
and “Export native MFD...”. Both suggest an `.mfd` filename in the project's
folder. Native rejection lists each affected component and retains its
compatibility feature and emitted identity; existing design/schema files remain
unchanged. A successful native-profile export confirms the local compatibility
checks, while acceptance by the reference application remains unverified.

Export writes the representable project subset plus generated schema or layout
siblings. Component kinds are selected from endpoint format metadata and paths.
JSON Schema siblings are reimported from their exact generated text before
publication. Export rejects a schema whose finite numeric metadata changes,
including a range endpoint, enum value, alternative discriminator, or nested
predicate. The error identifies the owning boundary or function and the
affected metadata path. This check applies to both export profiles. Some
float-valued integral endpoints also reject because the JSON Schema importer
cannot establish their exact integer meaning. Captured HTTP POST responses
have only an inline entry tree; assertions that this tree cannot retain reject
instead of being discarded.

PDF sources with all-page selection can write a native-shaped `.pxt` template
when they have only direct BasicVisual text captures, or one named page group
containing only those captures. This subset requires InsertSpace word
separation, Default whitespace, and page-edge coordinates without anchors.
One additional closed shape supports a vertical boundary finder followed by an
edge-row splitter containing one named page group and direct captures. The
splitter region may use only its boundary's begin/end anchors; its search is
empty and its initial/final skips are zero. Its fallback region is inferred
from the last eligible capture on native import, so export requires that
inference to match the stored layout exactly.
Another closed shape retains a first-page scalar capture and two ordered,
independent physical-page regions, from the first page and exact page two,
feeding one edge-row splitter and direct captures in a named group. A
self-authored ruled PDF preserves its company value and four ordered rows
through two strict cycles; the local book-catalog design retains identical
extraction and XML for 52 books. Collage, ambiguous source ordering, incompatible
page selection, and altered fallback regions remain outside this shape.
Native splitter import accepts only absent or empty search controls and absent,
empty, or zero initial/final skips. Nondefault, malformed, or duplicate controls
reject the template with a diagnostic instead of silently changing extraction.
Export checks the generated template with the bounded native-template parser
and compares command structure, names, algorithms, and every retained numeric
value by its binary64 bits. Self-authored two-page PDFs map ordered text rows
through two strict export/re-import cycles, including four rows through the
boundary and splitter shape. Other supported PDF layouts use Ferrule's lossless
layout payload. Ordinary payloads retain version
1; version 2 preserves floating-point values that ordinary JSON cannot
round-trip exactly. The complete template, including XML escaping, is limited
to 1 MiB. The export report inspects the actual generated sibling and reports
`pdf_layout` when that sibling requires Ferrule's extension; strict native
export then rejects before publication. Local parser checks do not establish
that the reference application accepts or executes the template.

Two guarded anchored PDF families also export as native-shaped templates: a
first-page header/painted-edge table/footer with ordered vertical and horizontal
anchors, and a first-page named header followed by all-page boundary detection
and anchored item rows. Emitted commands, page selection, names, capture modes,
fallbacks, and every coordinate/finder bit are compared with the actual local
native-template parser. Self-authored two-page files and the two local invoice
regressions retain extracted values and serialized output through two strict
cycles. These checks do not certify reference-application execution.

Native `ObjectFind` templates retain their visible schema, connections, and
approximate layout for repair, together with a closed `repair_dependency`
marker inside `PdfLayout`. The physical reader cannot reproduce background-based
visual candidate detection or its object-level minimum extent. Executable import,
PDF file/payload execution, and both export profiles therefore reject that
boundary. The marker survives ordinary and exact project/layout save/reopen;
renaming input paths cannot select another decoder. Intentional host-preparsed
typed instances remain usable. This withdraws the earlier stock-PDF extraction
claim until the native candidate model is implemented and verified.

Inline CSV field declarations are sorted by their numeric `fieldN` suffix.
Malformed or duplicate indexes and empty or duplicate names diagnose and skip
the component; executable imports reject the warnings. Headerless source and
target values retain their columns even when declaration elements are shuffled.
CSV text boundaries accept absent encoding or the supported UTF-8 code `1000`.
Other declared encodings produce a bounded warning; repair imports continue
as editable schema/connection drafts, while executable imports reject the
unsupported contract. A closed `csv_text_repair_dependency` cause set survives
project and pipeline save/reopen. Configured file, payload, browser, and pipeline
preview byte I/O reject that dependency before format dispatch; renamed paths
cannot select another decoder. Both export profiles reject before publication,
including existing design/schema siblings. Intentional host-preparsed typed
instances remain usable.
CSV components retain `byteordermark="0"` or `"1"` as the saved
`csv_utf8_bom` option. Native export writes the observed UTF-8 tuple
`encoding="1000" byteorder="1"` and an explicit BOM flag; two strict
export/reimport cycles retain the setting and exact UTF-8 output bytes.
Input accepts either UTF-8 form, while file, payload, and browser output add
the BOM only when selected. Unsupported byte-order or BOM codes produce
bounded repair warnings and reject executable import.
The same saved repair marker retains unsupported byte-order and BOM causes.
It also retains malformed empty-field policies, empty/multiple/unsupported
separators, invalid quote settings, and unrecognized header flags. Header flags
are limited to absent, `true`, or `false`; numeric aliases are not inferred
without reference evidence. Repair drafts use a supported delimiter/quote pair
and bounded diagnostics without retaining the original unsupported declarations.
These checks establish local serialization and execution behavior; native
application execution remains unverified.
CSV sources with `removeempty="false"` (or `0`) retain present empty text cells
as `Value::String("")`; `true` (or `1`) keeps them absent. Old Ferrule projects
and imports without a flag preserve their existing absent-empty behavior.
The saved `csv_preserve_empty_strings` option applies to file, payload, and
browser reads; missing trailing columns remain `Null`. Export writes the
explicit inverse `removeempty` setting, and two strict cycles retain the
presence distinction. Invalid flags persist a repair cause and executable
imports reject them. A native source requesting present empty fields with
non-text columns retains a typed-empty repair cause; physical reads, executable
assessment after warning clearing, and both export profiles reject that
unverified contract. Typed CSV targets remain supported. Manually authored
Ferrule projects retain their defined empty numeric/boolean-cell `Null`
behavior; intentional host-preparsed typed instances remain usable. CSV
source setup exposes “Keep empty text fields.” Browser reads and writes now
also honor custom or disabled quoting, including the writer's boundary errors.


The GUI's **File → Import MFD as Pipeline** action opens the supported connected
design in the separate pipeline editor. Choose a new pipeline location; the
document stays unsaved until **Save pipeline**. Ordinary single-mapping import
and export remain separate actions. The editor can export its current applied,
validated snapshot with either **Export MFD (Ferrule)** or **Export native MFD**
without saving the pipeline JSON first. These actions retain the same native
serial-chain limits and diagnostics as the CLI; unsupported branching does not
fall back to a single mapping. The main mapping canvas and its layout are
preserved throughout. See [GUI pipeline editing](mapping-pipelines.md#editing-in-the-gui)
for save, cancellation, path, and warning behavior.

Pipeline export writes one connected design for a validated serial XML
pass-through chain of 2–65 stages. Each intermediate primary target becomes
the next stage's pass-through source. The final primary target may be XML,
delimited CSV, fixed-width text, configured FlexText, JSON, Protocol Buffers,
XBRL without presentation metadata, or a new XLSX workbook. Connected named
final targets may be XML, or one CSV or JSON document target beside an XML primary target. An original static XML
host source may connect to named inputs in multiple stages, including the
first and later intermediate stages, when its boundary and output ports still
match. Connections from a
reused output port share one vertex with multiple edges. Intermediate XML
pass-through components retain their declared output instance and source
preview instance, even when the two
paths differ. An undeclared output instance is not inferred from an input
preview or schema default. Repeated target branches connect the unique
uncloned publication port; zero or multiple base ports reject before output
artifacts are written. Preflight and native-profile checks run before any
design or schema sibling is published.
The earlier-result named source must match its producing intermediate's
schema and options and share the immediate successor's source preview path.
Independent intermediate targets, disconnected final targets, other connected later-stage
named sources, and other non-XML intermediate boundaries reject
explicitly. Other non-XML targets remain final-primary only; updating an
existing workbook is outside this profile.
Synthetic two- and four-stage export/re-import runs preserve stage results;
terminal fan-out, repeated named-host connections, and CSV, fixed-width,
FlexText, JSON, Protocol Buffers, bounded XBRL, or XLSX final targets preserve
their connected outputs. CSV and
fixed-width results pass local write/read checks; fixed-width text, FlexText,
and JSON retain exact serialized bytes after re-import, Protocol Buffers retains
exact encoded bytes and decoded messages, bounded XBRL retains exact instance
XML bytes and parsed fact/context elements, and XLSX retains decoded worksheet cells.
XML-primary chains with a named CSV or JSON target retain both exact serialized
outputs through strict export, reimport, and CLI publication. Named JSON Lines
remain outside this profile. FlexText
export writes a new `.mft` beside the combined design
and refers to that sibling by name; unsupported layouts reject before either
artifact is published. Protocol Buffers export likewise writes a `.proto`
sibling beside the design; intermediate or named binary boundaries and
unresolved schemas reject before publication.
XLSX stage-value checks match group fields by name while preserving worksheet
and row order; workbook layout, rather than group field insertion order,
determines cell coordinates. Local mappings to JSON, XLSX, fixed-width text,
FlexText, and Protocol Buffers also run unchanged after an identity XML stage.
A fixture-backed XML-to-XBRL mapping preserves its instance document after that
stage; presentation metadata and numeric fact bindings are outside this chain
profile. A local
chained-report sample and a synthetic distinct-path case preserve both
intermediate instance identities across export/reimport.
All four local chains reimport after strict native export. The date/time chain
lowers uniquely bound direct XML dateTime conversions to the document's native
`cast-in-subtree` mode and restores `xs:dateTime` on precisely those generated
XSD elements; local date-only, dateTime, null, and invalid-input checks retain
the imported execution behavior. Calls shared with another graph consumer and
target schemas with defaults or unsupported subtree shapes still reject before
publishing artifacts. The internal schema does not retain general native cast
provenance, so this lowering is limited to the proven direct-binding case.
Acceptance by the proprietary application remains unverified.
When an imported JSON boundary or JSON string parser fell back to its entry
tree because its schema was unavailable, best-effort export records that
unresolved reference beside the generated schema and re-import reports it
again. Parser references survive project serialization in versioned recipe
metadata that leaves parsing behavior unchanged. Native-profile preflight
marks the export incomplete and rejects publication; the generated schema is
not evidence that the original external schema was recovered.
JSON5 endpoint syntax currently rejects before export because the native
component setting has not been verified; emitting an ordinary JSON component
would change the document syntax.
Supported named sources, independent targets, dynamic XML paths, HTTP response
boundaries, selected joins, exception sinks, and configured format components
retain their ownership in the exported design. A complementary source-backed
exception representation is limited to one `WhenTrue(P)` rule and one plain repeated
target scope over the same exact repeating primary schema path with the unary filter
`not(P)`. Its native filter evaluates the original `P`: the true output drives
the exception and the false output drives the target. Optional messages and
original predicate error identity remain distinct; ordinary consumers of
`not(P)` remain connected. Sorting, grouping, windows, post-group filters,
first/mapped output, generated or named failure iterations, transformed
ancestors, ambiguous consumers, and multiple true-rule ordering reject before
publication. Local regression tests cover this route. Native compatibility
remains subject to design-specific checks and the error/publication limits
described below. Export omits a complementary `not(P)` component
only when that filter is its sole use; other graph consumers, target/control
roots, failure messages, and dynamic source expressions retain it. This
narrow omission is intended to avoid an observed unused-output validation
warning; warning-free native validation still requires a fresh native check.
Empty XML controls must satisfy the emitted source XSD; repeating collections
use `minOccurs="0"` without the singular optional-element flag. Schema-root names are not stripped from collection paths, and an
empty path requires the primary schema root itself to be repeating; implicit
flat-row formats are outside this new route. Structured XML string serializers
with default indentation round-trip as native components with generated XSD
siblings and structural source connections. Explicit compact serialization
still uses a Ferrule extension and is rejected by the native export profile.
Static file-backed XML iteration controls export through typed XML variables,
preserving selected source frames and final positions. Copied variables retain
their `First` or mapped-sequence output on reimport. A bounded `First` projection
uses its raw parent as the structural trigger, so a parent survives even when
none of its children pass the selection. This lowering requires direct fields
from the selected child frame and a unique target owner; unsupported shapes
produce a native compatibility diagnostic before publication.
Conditional string text also lowers directly when its output count is exactly
`exists` of the same source field, with one generated item owner and no target
attributes or other payload. Present empty strings remain present, absent
strings remain absent, and shared predicate consumers remain connected.
Computed text, numeric or nil text, and ambiguous source ownership retain their
extension dependency. The qualified `First`, conditional-text, and controlled
XML designs have been validated, executed, and saved in isolated native tests;
the saved designs replay with exact typed and expanded XML output, allowing
only explicit schema-location metadata differences. These checks cover those
designs rather than every graph accepted by static preflight.
Declared local SQLite relations round-trip with their owning
database connection.
CSV components retain a printable single-byte quote character or an explicit
disabled-quoting mode (`quote=""`) beside the delimiter and header setting.
Both modes apply to file and in-memory execution and round-trip through the
native `<settings quote>` attribute. With quoting disabled, Ferrule refuses to
write fields containing the separator or a newline because they cannot retain
their row and column boundaries; the reference application's raw-emission
behavior for those values has not been verified. Invalid or ambiguous dialect
options reject before output publication.

The fixed-per-run current date/time value exports as the corpus-backed native
`xpath2/current-dateTime` component. Both that component and `lang/now` import
to the same typed runtime value and round trip under a fixed execution clock.
Target-type date/time coercion remains a separate extension dependency.

The default `ferrule-extensions` profile preserves the existing Ferrule
round-trip representation. Export now reports whether the rendered design has
known native `.mfd` compatibility dependencies, Ferrule extension
dependencies, or lossy omissions. These findings are separate from the legacy
export warnings and identify their owning component and feature where possible.
`--diagnostics json` writes each finding as a versioned JSON Line on stderr;
`--report-json` writes the complete deterministic report as one JSON object on
stdout. A report contains `schema_version`, `command`, `profile`, `mode`,
`accepted`, and a nested `report` with `compatibility`, `issues`, and
`warnings`.

`--profile native-mfd` refuses known Ferrule-only components, retained
metadata required for execution, captured-source behavior differences, and
lossy export warnings before publishing any design or sibling files. `--check`
renders the same export for inspection without creating the target directory or
files. A strict check exits nonzero when native export would be refused.
The compatibility result is a static assessment of the emitted design. It does
not certify that a particular release of the reference application can open or
execute it, or that external schemas and connections are available; those
require vendor execution tests against the versioned conformance suite.

Export is atomic: a shape that cannot be represented safely is rejected instead
of publishing a partially wired design. Successfully exported designs are
expected to re-import and validate as ferrule projects.

## Current Boundaries

The main remaining gaps are some XML derived-type input shapes beyond compatible
`complexContent` and scalar-text/attribute-only `simpleContent` hierarchies,
XSD 1.1 wildcard exclusions, unordered wildcard compositors, unresolved strict
wildcard declaration sets, heterogeneous or correlated numeric-range scalar
unions and general heterogeneous arrays,
overlapping cross-mode, and incompatible typed-wrapper JSON union composition,
first-class sequence composition, general SQL and database mutation, broader
XLSX/PDF/FlexText configuration shapes, taxonomy-level XBRL execution, and
direct execution of unrecognized or external-service user components. Bounded
cross-namespace substitution groups, heterogeneous scalar type arrays,
pairwise-disjoint scalar `oneOf`, exact scalar `anyOf` unions, bounded exact
multi-value scalar `enum` domains, and array `anyOf` branches whose scalar item
domain subsumes every narrower branch are preserved, including required or
optional typed and JSON-null discriminators.
Flat nullable compositions may combine null with multiple compatible object,
scalar-union, or subsumed-array branches.
JSON components retain exact ordinary integer and finite-number ranges from
their referenced schemas, including nullable numeric fields and compatible
`allOf` intersections. Those constraints apply when imported mappings read
source documents and write targets; malformed, empty, or precision-ambiguous
ranges trigger the component's existing schema-fallback diagnostic instead of
silently widening the mapping boundary.
Positive finite JSON Schema `multipleOf` divisors likewise retain exact
canonical decimal semantics through referenced schemas, compatible
compositions, MFD import, and generated schema export. Native and generated
Rust/C# boundaries enforce the divisor on input and normalized output.
Correlated unions that vary both a numeric range and its paired divisor fall
back with an actionable diagnostic rather than being widened.
Referenced JSON arrays likewise retain exact `minItems`/`maxItems` intervals
through references, nullable wrappers, and compatible compositions. Invalid or
nonrepresentable item-count unions use the same actionable schema fallback;
valid constraints remain executable after MFD import.
Referenced array `uniqueItems: true` assertions also remain executable across
MFD import and generated-schema export, with exact structural comparison on
native and generated Rust/C# JSON boundaries.
Referenced arrays also retain bounded `contains` assertions through MFD import,
canonical schema export, and re-import. Plain assertions require at least one
matching member; Draft 2019-09 and newer `minContains`/`maxContains` modifiers
retain an exact match-count interval. Compatible `allOf` assertions remain
conjunctive, nullable array null bypasses them, and predicate patterns use the
same bounded document matcher budget as ordinary string constraints.
Referenced JSON objects preserve their exact openness contract as well.
Omitted or `true` `additionalProperties` remains an unconstrained dynamic
property domain, schema-valued declarations remain typed, and explicit `false`
rejects undeclared input properties instead of discarding them. MFD export
writes a canonical JSON Schema sibling and re-import retains the same open,
typed-open, or closed behavior.
Referenced schemas also retain exact closed homogeneous `patternProperties`.
The containing schema must explicitly type `object` or `object | null`, use
`additionalProperties: false`, and assign every portable selector in a
nonempty bounded map one identical exactly representable value schema. Scalar,
structured object, and homogeneous array values reuse the ordinary supported
JSON value profile. Selectors are ORed in declaration order. Any matching fixed
property must have that schema; nonmatching fixed properties remain
independent. Runtime decoding selects fixed properties first, then applies the
selector set to remaining names, while `propertyNames` continues to constrain
every key independently. Dependency rules whose triggers are neither fixed nor
selected normalize away as semantically unreachable. Nullable null bypasses
the object checks. Native and generated Rust/C# boundaries enforce the
selectors on input, normalized output, and every JSON Lines row with the shared
per-document pattern work budget. Canonical MFD schema export and re-import
preserve the selector map and closed fallback. An empty map is a no-op.
Referenced JSON objects also retain exact `minProperties`/`maxProperties`
intervals through nullable wrappers, dialect-aware references, compatible
`allOf`, and alternatives with one common effective interval. Imported
mappings enforce each distinct parsed input-property set and normalized output
object in native and generated Rust/C# boundaries; canonical MFD export and
re-import retains the interval. Unsatisfiable required/closed-object bounds and
correlated alternative intervals produce the existing actionable
schema-fallback diagnostic rather than silently widening the contract.
Referenced JSON objects retain executable property dependencies as well.
Modern `dependentRequired` and legacy property-array `dependencies` normalize
to the same bounded relation and export canonically as `dependentRequired`.
Native and generated Rust/C# boundaries require every dependent name whenever
its trigger is present, counting explicit JSON null as input presence and
checking normalized output after absent fields are omitted. Nullable object
null bypasses the rule, compatible `allOf` branches unite rules, and alternatives
must share one identical effective relation. Schema-valued legacy dependencies
and modern `dependentSchemas` are executable when their whole-object predicate
fits Ferrule's retained JSON subset. Required-only predicates lower to the
property-dependency relation; nontrivial predicates retain nested ordinary
object/array constraints and export canonically as `dependentSchemas`.
Repeated rules for a trigger remain conjunctive through export and re-import;
ordered outer `allOf` branches preserve interleaved trigger declaration order.
Nested dependent schemas retain the same recursively bounded behavior. Draft 7,
2019-09, 2020-12, and undeclared explicitly typed object schemas may also
express one of these rules as an `if` with exactly one required-property
presence trigger and a supported `then` predicate. A nullable outer object is
supported with an absent or `true` `else` only when the `if` explicitly proves
`type: "object"`. Exact `else: false` requires a trigger that can be represented
as an ordinary required field; it removes the nullable bypass when the false
branch rejects null and retains any supported `then` dependency. It does not
combine with existing object alternatives, and a closed object must already
declare the trigger. Import normalizes these forms to `required` plus
`dependentRequired` or `dependentSchemas` metadata as appropriate, which
canonical MFD schema export preserves. Value-sensitive, multi-trigger,
general-`if`, other nontrivial `else` schemas, distinct per-selector
`patternProperties` schemas, open or typed pattern-property fallbacks, general
selector-overlap intersection, value shapes outside the ordinary exact JSON
profile, pattern-property objects under active `allOf`, alternatives, or
structural `$ref` siblings, unevaluated property keywords, and heterogeneous
positional array schemas still produce the existing actionable schema-fallback
diagnostic rather than being widened or discarded. Declared Draft 4/6/7
resources use legacy schema-valued `dependencies`; declared 2019-09/2020-12 use
modern `dependentSchemas`. Schemas without `$schema` intentionally accept both
spellings while retaining modern reference-sibling behavior, which preserves
older MFD schema packages without weakening explicitly declared dialects.
Referenced JSON arrays using homogeneous legacy tuple-form `items` also remain
executable through MFD import. Draft 4, 6, 7, and 2019-09 resources, plus
schemas without `$schema`, normalize 1 to 4,096 identical positional members
to one repeated item shape. An identical `additionalItems` tail remains
unbounded, while `false` closes the array at the tuple length. An absent or
`true` tail is accepted for an arbitrary-JSON item shape or when an explicit
maximum makes the tail unreachable; a different schema tail requires that
same maximum proof. The derived maximum intersects explicit
item-count bounds. MFD schema export writes ordinary schema-valued `items` and
an optional `maxItems`, so export/re-import and generated Rust/C# use the existing
homogeneous-array path. Draft 2020-12 array-valued `items`, contradictory
counts, and reachable heterogeneous members or tails produce the actionable
schema-fallback diagnostic instead of being reinterpreted as `prefixItems`.
Referenced JSON objects retain supported `propertyNames` schemas as well.
Exact false, finite `const`/`enum` names and their finite `not` complements,
Unicode-scalar length intervals, bounded portable pattern
conjunctions/disjunctions and their exact complements, and nonasserting
`format` annotations remain executable across import, canonical schema export,
and re-import. Native and generated Rust/C# boundaries check every raw parsed
input key and normalized emitted key, including runtime-named and empty-string
properties. Unconstrained forms normalize away; correlated predicates and
complements involving length, format, or mixed assertions produce the existing
actionable schema-fallback diagnostic. Each
referenced schema resource retains its dialect: Draft 4 ignores
`propertyNames`, while Draft 6 and newer resources apply it.
Referenced string-capable JSON fields retain exact `minLength`/`maxLength`
intervals measured in Unicode scalar values, including nullable fields, array
items, typed dynamic properties, and compatible compositions. The constraints
remain executable on imported source and target boundaries and round-trip
alongside opaque `format` annotations. Referenced `pattern` assertions use
Ferrule's bounded Unicode-scalar pattern language, survive dialect-aware
references, conjunctive `allOf`, exact disjunctive `anyOf`, and disjoint scalar
`oneOf`, and export canonically. Null bypasses string assertions in nullable
domains; native and generated Rust/C# boundaries apply them identically to
input and normalized output. Nonportable regex constructs reject during schema
import and trigger the component's existing actionable schema-fallback
diagnostic rather than changing meaning between runtimes.
Expanded-name identity for ordinary elements and attributes is preserved;
foreign declarations export as an atomic graph of local XSD siblings.
Compatible strict-wildcard declarations that share one local name across
namespaces use one mapping port with exact expanded-name alternatives. Selected
QName ports narrow that ambiguity before target construction; incompatible
same-local shapes remain actionable warnings.
Namespace-constrained optional/unbounded element wildcards with
`processContents="skip"` round-trip as recursive generic element groups, while
closed strict wildcards become exact singular or repeating typed choices. Lax
element wildcards in sequences and repeating choices expose resolved global
declarations as typed fields and route only undeclared matching names to the
generic fallback. Direct or named-attribute-group wildcards preserve namespace
constraints and processing mode; known declarations remain typed, lax unknowns
remain generic, and strict unknowns reject at the XML boundary.

The exact supported surface evolves quickly. The
[workflow-parity roadmap](../ROADMAP.md) records the strategic gaps, while the
`mfd` test suite contains self-authored regression designs for executable
behavior.

## Independence

ferrule is an independent project and is not affiliated with or endorsed by
the developer of the reference application.


### Global failure priority and strict native export

Failure rules scan their complete collections in declaration order before any
target is evaluated. A native exception attached to a filtered item branch does
not establish that global priority: an earlier passing item's target can fail
before a later selected item reaches the exception. Multiple rules can also
select in a different order when executed item by item.

Strict native export reports `global_failure_ordering` and publishes no design
or schema siblings unless a conservative totality proof succeeds. The initial
subset has one primary source rule, local closed String/Int/Bool XML boundaries,
one direct repeating source collection and its plain repeated target branch.
It allows constants, exact frame-pinned singular fields, the matching position,
Boolean negation, and comparisons of present integers. Targets require matching
scalar types and compatible presence, without conversions. Additional sources
or targets, other calls, fallible messages, grouping, sorting, windows, dynamic
construction and other unproved controls are refused. Connected pipelines with
global rules also require a separate ordering proof and are refused in strict
native mode. These restrictions are
conservative; a refused expression is not necessarily fallible on every input.
The proof is bounded to 4096 graph nodes, 4096 nodes per boundary schema or scope
tree, 128 dependency or nesting levels, and 100,000 expression visits. Shared
expression results are memoized. Designs exceeding those budgets, or containing
a user-defined function registry, retain the extension profile and are refused
in strict native mode.

The proof applies to schema-conforming inputs in the IR scalar domains and
mapping evaluation errors,
with positions within the signed integer domain. It does not certify native
output publication, I/O, serialization, resource limits or arbitrary host-owned
typed values. The default extension profile continues to preserve global rules
for local round-trips, including refused strict-native cases. Existing exported
global rules have no item-order marker. Item-ordered import requires the explicit
opt-in described below and does not reinterpret legacy global rules.


Native exception import has an explicit item-order option:
`ImportOptions::with_item_ordered_exceptions()`. The CLI exposes it as
`ferrule import-mfd --mfd design.mfd --out project.json --item-ordered-exceptions`.
In the GUI, select **Preserve row error order** in the File menu before
**Import MFD...** or **Import MFD as Pipeline...**. The choice is unchecked at
startup and lasts only for the current session; it is not saved in a project or
preferences. An opted-in single-project import requires an executable result
before writing a CLI project or replacing the GUI mapping. Unsupported shapes
produce an error rather than silently using a global rule. The same choice is
passed through `--pipeline` and the GUI pipeline workflow, which currently refuse
exception stages outside the single-mapping subset. Without the flag or checked
choice, the existing import and warning behavior remains available.

Default imports continue to retain legacy ordered pre-target failure rules. The option initially accepts
one ordinary two-branch filter over one directly connected repeated primary
XML group. The opposite branch must have one exact primary target structural
port and one plain repeated scope owner. It replaces only that owner's filter
with a lazy graph `Raise` guard using the original predicate. A selected
message is evaluated in the raw item context; target bindings retain their
existing context and node identities.

This mode evaluates a surviving item's target before testing the next item.
An earlier target error can therefore precede a later selected exception.
It is distinct from a global failure rule, which scans before any target.
Extra boundaries outside the one transparent typed XML carrier described below,
shared branch consumers, chained controls, controlled
ancestors, independent target evaluation, generated or joined iteration,
recursive/open schemas, ambiguous physical ports, and over-budget expression
shapes produce import diagnostics. Executable import refuses them; best-effort
import never silently substitutes a global failure rule. The attachment is
validated as a complete project before pruning. The option propagates to
pipeline imports, whose exception stages remain outside this initial subset.

Finite regressions and saved-design checks cover admitted direct guards, lazy
messages, and earlier-target/later-exception priority. `Raise` has a
node-identified `MappingException` error, while
legacy globals retain rule-identified `MappingFailure`. These categories and
native output publication are separate compatibility boundaries; this option
does not promise identical exception categories or partial-file behavior.

Serial XML pipeline import recognizes a core exception terminal only when it has
no outputs, one connected throw input at position 0, at most one message input
at position 1, and one exception marker. The throw must be one exclusive branch
of an ordinary two-output filter; the opposite branch must reach one XML stage
target owner. That terminal is a side effect, not a document sink: every
connected XML source still needs a downstream stage target. Ordinary dead
functions, cycles, ambiguous owners, duplicate physical feeds and malformed
terminals remain unsupported. The default import retains the existing global
failure rule only on its owning stage, including a final named XML target. It
does not convert global rules to item-ordered expressions or promise native
exception ordering or output-publication agreement. The item-ordered import
option continues to refuse pipeline boundaries outside its single-mapping
subset. CLI pipeline output publication waits for every stage to succeed.

The item-ordered exception opt-in additionally confines reachable predicate,
message and owner/descendant binding expressions to scalar fields in the exact
owner item, with no further repeated element crossed. Explicit owner frame pins
and owner-prefixed absolute paths are accepted; sibling repetitions, unframed
relative broadcasts and singular root broadcasts remain outside this first
subset. Position must identify that owner collection, or the one current item
in the already admitted tree without other iterations or private frames.
Unsupported projections produce an opt-in warning before a guard is published;
strict executable import refuses it without using a global failure-rule fallback.


The item-order opt-in also accepts one transparent typed XML scope-sequence
variable for one immediate repeated primary collection. Its complete schema
must equal that collection's schema; it has no instance-file boundaries or
connected descendant inputs. The primary non-repeating document root alone
triggers computation, and the keep branch alone supplies the variable root.
That root feeds one exact target structural owner and, when needed, dedicated
filtered-target Position nodes. Scalar outputs feed only that owner's ordinary
scalar binding dependencies. Raw predicate/message Position nodes instead read
the original collection and cannot be shared with target expressions.

The existing bounded owner-field proof, plain scopes, lazy message and original
predicate identity remain required. Additional variables, modified schemas,
foreign or item-level compute triggers, unrelated consumers, cross-context
Position feeds, projection inputs, prepasses and unsupported scalar closures
produce opt-in diagnostics. Strict executable import refuses those designs
without a global-rule fallback; default import retains its legacy pre-target
rules. This is a bounded local import route, not a promise of identical native
serialization, file publication or arbitrary host-input behavior.
