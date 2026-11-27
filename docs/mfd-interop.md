# `.mfd` Interoperability

Use `import-mfd` to recover an editable mapping, or add `--require-executable`
to refuse incomplete imports. `export-mfd --profile native-mfd --check` tests
what Ferrule can represent before writing any files. Local admission and a
successful round trip do not establish execution in the reference application.

| Starting point | Contract |
| --- | --- |
| One design on disk | [Import a project](#import); [resolve schemas and catalogs](#package-roots-and-catalogs) |
| A design supplied as bytes | [Isolated byte packages](#isolated-byte-packages) |
| One target feeds the next | [Import a connected pipeline](mapping-pipelines.md#importing-a-connected-mfd-design) |
| Connected input and output filenames | [Import a document plan](#filename-driven-documents) |
| A project to exchange | [Export](#export), [format boundaries](#current-boundaries), and [failure ordering](#global-failure-priority-and-strict-native-export) |
| Evidence of external behavior | [Versioned conformance inventory](../conformance/README.md) and [survey procedure](#read-only-parity-surveys) |
## Isolated byte packages

Hosts that cannot grant filesystem access use `MfdPackage::new`,
`import_package` and `export_package`. A package contains one named design and
its immutable schema/configuration entries. Export returns both the complete
package (including every generated schema sibling) and the ordinary compatibility
report; `ExportProfile::NativeMfd` rejects known extensions or omissions before
returning any artifacts. The existing filesystem and CLI APIs remain available.
Package export also admits the exact emitted bundle through the package importer
before returning it. Retained schema/presentation references must obey the same
strict policy as package imports; unsafe aliases refuse instead of merely
normalizing the output filenames. Re-import warnings join the export report.

Packages admit at most 128 entries, 4 MiB per entry, 16 MiB in total, and 256
UTF-8 bytes per path. Every entry is a regular byte buffer; no archive extraction,
symlink, file handle or host directory is accepted. Names are slash-separated
relative paths without empty, dot or parent segments, drive prefixes, URLs,
backslashes, control characters or trailing spaces/dots. ASCII case aliases are
refused. References resolve relative to the design's directory; transitive
schema/configuration references obey the same strict path policy. There is no
filesystem, catalog, manifest-discovery or network fallback. Missing supplied
assets refuse with `MfdError::Package` rather than silently substituting an
entry-tree schema.
This includes unresolved JSON Pointer targets within supplied JSON Schemas;
ordinary filesystem imports retain their existing best-effort behavior.

XML/XSD, JSON Schema, ordinary X12/EDIFACT/HL7/TRADACOMS configurations,
FlexText, PDF templates, XBRL presentation metadata, protobuf bundles and supported
adjacent source modules consume supplied bytes. Runtime instance paths remain
metadata, not instructions to open files. Database introspection, IDoc and SWIFT
configuration import require filesystem-only backends and explicitly refuse in
package mode. Package export also refuses database boundaries, whose lowering or
re-import can require a live database. Unsupported mapping constructs retain the importer's
ordinary actionable warnings; callers must show those warnings before accepting
a best-effort project. A warning-free import is not proof of reference-application
execution equivalence.
Protobuf dependencies are read lazily from the supplied package. Only reachable
files consume protobuf graph budgets or undergo source decoding; unrelated
package entries remain subject to the package's aggregate bounds but cannot
invalidate an otherwise complete protobuf graph.

## Import

From the repository root, start with the [XML example](../crates/cli/tests/fixtures/executable-import/design.mfd).
The input's `Number` field feeds `Value` on the output:

```sh
cargo run -p cli -- import-mfd --mfd crates/cli/tests/fixtures/executable-import/design.mfd --out project.json --require-executable
```

Omit `--require-executable` to keep a repairable project and its warnings.
Document-local declarative functions have a [separate library contract](mfd-declared-libraries.md);
isolated byte packages refuse them without falling back to the filesystem.

This selects `ImportProfile::Executable`: import warnings, unresolved runtime
dependencies, and engine validation findings reject before the destination
project is created or replaced. Ordinary import retains a partial design for
repair. The flag preserves legacy global failure-rule ordering; row-error-order
semantics require the separate `--item-ordered-exceptions` option. The two flags
may be combined. `--require-executable` applies to single-project imports and
cannot be combined with `--pipeline`. Static admission does not execute the
mapping or establish behavioral equivalence with another application.

### Filters and producer ownership

Source-field filters retain their selected true or false branch. Proven singleton
scalar values use a nullable conditional; supported repeated-field selections use
collection lookup. Missing, nonboolean, or unsupported predicates never become
unfiltered bindings: repair-oriented import skips the affected binding and warns,
and executable import refuses the partial mapping.
Refusal propagates through dependent functions, defaults, UDFs, and scalar parsers;
it is not an absent value that a default may replace. Collection-position
predicates must refer to the selected collection. Repeating filtered-variable
inputs to scalar UDFs or parsers are refused where the consumer cannot retain
the active item context.
Structural serializers also refuse filtered sequences whose selection they cannot preserve.
Specialized scalar lowering and refusal checks share one producer input contract,
including structured UDF parameters. Refusal of one projected child does not
discard supported sibling projections; an invalid filter predicate still refuses
every projection of that filter branch.
UDF input admission is per pin: scalar arguments retain scalar handling, while
structural catalog inputs refuse crossed filters that cannot be preserved.

Physical scalar `count` over a filtered source has a separate bounded import
profile. It preserves the selected predicate and proves each direct scalar
receiver against the ordinary primary XML reader or configured X12 reader.
Only specifically proved counted-field predicates and Boolean literals are
admitted. Missing values are excluded before applying the filter's true/false
branch; an absent value is not replaced with a default. Independent predicate
fields, predicates that read XML nil, repeating X12 scalar selections, and
unproved constructed or document receivers remain refused.

The proof checks raw and registered producers without selecting away competing
claims, then rechecks every materialized use after graph transformations. Both
import profiles refuse an affected ambiguous or unsupported Count; neither
publishes an unfiltered Count, zero, or an absent binding in its place. Existing
ordinary Counts and earlier aggregate routes retain their own admission rules.

Complete routing classification has separately accounted input-derived work
and storage bounds. The positive ownership/construction proof shares a sticky
65,536-work-unit and 32-MiB allowance; exhaustion cannot turn an affected Count
into an ordinary one. Proved unrelated ordinary Counts do not need that proof.

This profile is a theorem about those reader-created input trees, not a runtime
contract for arbitrary hand-built `Instance` values. Its projects use the
ordinary supported save/load codec without adding a guard capability. Count
composition with guarded declared UDFs remains refused. This profile does not
establish distinct-value Count behavior or MFD export/re-import equivalence.

### Filename-driven documents

CSV, JSON and EDI document imports diagnose connected `FileInstance` filename
inputs and outputs. Their document-loading and output-document behavior is not
represented by the imported data mapping, so executable import refuses them.
Ordinary import keeps the partial mapping with that diagnostic. Unconnected
wrapper ports and static preview/output path metadata do not introduce
filename-driven document behavior.

For one connected EDI input filename and one connected strict JSON output
filename, select the explicit document route:

```sh
ferrule import-mfd --mfd design.mfd --out document.json --filename-document
ferrule run-document --project document.json --input-root inputs \
  --param InputFile=input.hl7 --param OutputFile=output.json
ferrule generate --project document.json --primary-documents --language csharp --out generated
```

Use the parameter names declared by the design. The import retains both
filename expressions as graph identities in `DocumentProject`; they are not
business fields or replacement static paths. Only literal strings and required
global string parameters are admitted. Saved parameter and instance previews
never supply execution values. The input host receives the exact filename text
and the plan's `Direct` selection, and must select exactly one document. The
mapping runs once and returns one complete strict JSON document. Native and
generated Rust/C# primary EDI byte entry points use the retained JSON output
policy for these explicit plans. `evaluate_output_filename` /
`EvaluateOutputFilename` exposes the output transport separately; library hosts
own publication. The CLI resolves its connected output relative to the saved
document project and refuses an `--output` override rather than dropping the
connection.

This route refuses wildcard selection, empty/wildcard/control-character filename
text, source-dependent filenames, wrapper controls, connected filename output
ports, ambiguous declarations, named endpoints, and repeated document roots.
Unsupported plans and filename values refuse before host calls. Path containment,
URI policy and physical I/O remain host-owned; the CLI's input host only reads
beneath its explicit local root. Ordinary import and the repair-oriented CSV
filename-retention API retain their existing behavior.
The editor refuses plans with connected output filenames rather than opening or
saving them with that transport removed. Use the CLI or a generated host.

XML imports retain their supported static connected paths and document-driven
output scopes. A runtime-connected primary input filename or a target filename
without a supported document driver produces a diagnostic rather than silently
qualifying a mapping that uses only its saved preview path.

The reference application reads a component's input instance only in a source
role; on a target it is a design-time preview. A filename wildcard there does
not make the target a file set and never becomes its output path in any target
format. Beside an output instance, any input instance, including a URL or a
wildcard folder, is ignored, and it does not refuse item-ordered exception
import.
Pass-through components keep the wildcard as a file set in their source role.

One bounded filtered filename form is supported: a primary XML root contains
repeating outer records, each record supplies a string filename expression,
and `exists` on one direct nested collection filters that filename. The filter
is evaluated in the outer record before its filename; a filtered-out empty
filename does not produce a document or a path error. Admitted records retain
their complete nested contents and document order. Negative filter outputs,
unrelated or ambiguous drivers, conditioned source ports, ambiguous literal or
pin declarations, and nested collections that could fall back to a same-named
root collection remain unqualified.
Export retains the equivalent `exists` guard on the filename. This form
re-imports under the same bounded rule; other filtered filename shapes are
refused rather than exported without their guard.

Each physical MFD input connector may have one distinct source feed. The
importer checks modern graph edges and legacy connections before lowering;
identical records are idempotent. Distinct sources on one input produce a
deterministic diagnostic and every feed to that input is skipped in the
repair-oriented project. Executable import refuses that diagnostic before
publishing a destination. Function definitions and pipeline imports refuse
conflicted physical inputs rather than choosing an expression or a default.
Source fan-out and separately declared target occurrences retain their own
cardinality and ordering rules; competing bindings to a singular target remain
subject to executable validation.

Renamed `core` kind-2 constants retain their literal identity rather than
becoming function calls named after the display label. The renamed profile
requires one well-formed string, Boolean, signed-integer or finite-number
literal, no input pins and one output pin. Extra behavior metadata and malformed
literals do not gain admission through a rename. Existing canonical constants
keep their compatibility behavior; export still uses canonical component names.
Specialized raw-metadata proofs retain their own narrower admission rules.

A native value-map with `defaultValueMode="input"` whose input and result types
are both declared exactly `string` imports as
`substitute_missing(value-map(input), input)`: a table miss returns the input
unchanged, an absent input stays absent, and a saved custom default is ignored.
This holds in the main mapping and in embedded function declarations. Every
other input-mode type pair, including absent or unrecognized type names, retains
an explicit unsupported-mode diagnostic, because the conversion of the passed
value is not established. So does an input-mode map saved beside another
value-map in the same component, because import lowers only one saved map.
Executable import refuses that diagnostic rather than admitting a map with its
fallback removed; ordinary import retains a warned partial design. Explicit
custom defaults, legacy enabled defaults and absent default metadata keep their
existing behavior.

A used scalar source output-node rule that names a missing or unsupported
function definition produces a warning with the function name and source output
key. Restore a supported definition or remove the rule before requesting an
executable import. Ordinary import keeps the field's editable mapping and skips
only the unresolved rule; supported rules and defaults retain their order.
Unconnected references, nonmatching scalar filters, and blocked inherited rules
do not produce this warning.

### Package roots and catalogs

For a mapping package whose resources sit above or beside the design directory,
declare the trusted package root:

```sh
cargo run -p cli -- import-mfd \
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
cargo run -p cli -- import-mfd \
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
`cargo test -p format-xml -p mfd --test resource_boundary` and
`cargo test -p mfd --test udf_resource_boundary`.
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
cargo run -p cli -- import-mfd \
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
An envelope's requested message type must match the selected file's single
direct `MessageType` declaration before that file's includes are read. The
comparison uses the complete XML text: comments and processing instructions
do not change the value, and whitespace is significant. A differently named
file can be discovered by its declaration when the match is unique.

Separately managed JSON Schema catalogs use the same explicit ordered trust
model:

```sh
cargo run -p cli -- import-mfd \
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

### Components and expressions
Import resolves the supported component graph into ferrule schemas, graph
nodes, scopes, format options, and endpoints. Current coverage includes common
XML, JSON, CSV/fixed-width/FlexText, XLSX, SQLite, EDI, Protocol Buffers, XBRL,
HTTP XML, and visual PDF source components, together with a broad set of scalar
functions, aggregates, sequence controls, lookups, joins, exceptions, and
recognized user-function shapes. Adjacent XSLT extension modules also import
when a named one-parameter template returns a direct count, sum, average,
minimum, or maximum over a descendant path; ferrule lowers that template to a
native aggregate rather than retaining an XSLT runtime dependency.

Collection `item-at` exports use the native two-input form: nodes at position
0 and the requested index at position 1, without a parent-context input.
Computed primary-source values must retain the selected collection's repetition
on every possible branch. Constant or broadcast-only values, nested reductions,
different repetition owners, computed named-source values and other unproved
expressions are refused before publication. Other aggregates retain their
parent-context input.

Direct generated-sequence `SequenceItemAt` uses the supported two-input form.
The `SequenceAggregate(ItemAt)` representation is refused before publication,
including plain, transformed and filtered variants: its private item and
parent-context ownership are not preserved by the current MFD importer.
Ferrule's interpreter behavior is unchanged.

Nested cloned target groups retain their declared ancestor entry ownership.
Child branches attach to that exact owner in declaration order, including
unequal branch counts; source anchors and branch conditions remain relative to
the owning parent. Redistribution requires unique explicit ancestor lineages,
a nonempty child partition for every parent, plain concatenation wrappers, and
exact provenance for every binding in a replaced placeholder. Each original
binding must survive once in its assigned descendant subtree, and each retained
binding must identify one original binding index. Identical cached expression
nodes alone do not prove that separate original bindings survived. Each
redistribution records which declared child occurrences it placed under each
parent occurrence; an ancestor proof attributes a retained binding to the
innermost recorded occurrence that holds it, so sibling clones bound to one
shared expression keep distinct provenance. That attribution never adds an
origin, and one occurrence must retain one binding for each original binding
it holds. Each level is proved completely before its scopes or diagnostics
change, and diagnostics keep child declaration order. A placeholder
with unclaimed, missing, broadcast, ambiguous, or controlled content remains
intact with a refusal diagnostic. A unique child entry may reuse its parent's
marker lineage when its deeper declared target path establishes containment;
retained provenance is
restricted to that descendant path. Numeric port order, equal branch counts,
and similar source paths
do not establish ownership. Direct IR without declared entry owners remains
subject to the ordinary concatenation validator. Duplicated sibling target entries keep one
binding each even when one expression feeds them, so a repeating scalar emits
every entry's value in entry-tree order. Multiple feeds to a singular target,
identical or distinct, retain their duplicate-binding refusal; their selection
semantics depend on the declared target shape.
The [binding rules](conformance/singular-target-feeds.md) distinguish
native same-connector validation failures from cloned entries that can emit
schema-invalid output; neither admits a singular winner rule.

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
### SQLite connections and query parameters

When one compact, declaration-free XML serializer feeds only a plain SQLite
column, native export restores `doc-xml` and writes an adjacent XSD. Other
consumers or row controls retain the serializer and its native-profile diagnostic.

A guarded `LIKE` with a literal ASCII prefix and one trailing `%`, null guards,
and sorting on the same string column can export as a native `where` control
on a single table mapped to XML rows. Its connected parameter can be an optional
named string with a literal prefix default. The `concat`/`string` bindings
remain connected. Shared predicates and additional row controls are outside
this profile.
The interpreter and generated Rust/C# `sql_like` calls compare text before the
first NUL, fold ASCII case only, and treat `_` as one Unicode scalar. Patterns
are limited to 50,000 UTF-8 bytes before NUL truncation, matching the bundled
SQLite limit. Ferrule also limits matching to 100 million value-scalar by
normalized-pattern-scalar cell updates. Over-budget calls report typed errors.
This work bound deliberately rejects some costly SQLite matches. An oversized
pattern on a null source row still differs: the imported null guard skips the
call, while SQLite reports a pattern-length error.
An isolated canonical decimal string feeding input two of a supported numeric
comparison or addition can export through a native decimal input component.
Optional native inputs retain their declared name, scalar type and connected
default. An explicit host value, including null, wins; otherwise the default
is lazy. Required and connected-default inputs retain enabled preview text,
including empty or malformed values. `ExecutionPurpose::Preview` selects
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
The order-pricing shape restores two native decimal rules on XML price leaves.
Repeated-row reads keep the conversion error for nonfinite input. Changed
arithmetic, shared conversions, string leaves or non-XML sources refuse.

A guarded `%Word%` title predicate can reconstruct one native database `where`
while preserving nested relations, an independent department or office filter,
and same-type pattern/selection inputs. For `concat(prefix, "%")`, an explicit
null is supplied: concat yields `%` and matches every present text row. Shared
predicates, extra row controls and changed field types refuse.

Joined query export supports a declared foreign key, numeric threshold and a
computed column with typed overflow. A required or optional same-type parameter
remains connected; an empty source can avoid reading a required input. Normal
runs ignore saved preview text. Cross-type coercion (for example a string host
parameter compared with `Quantity`) and optional parameters without defaults
are unsupported. An unimportable predicate disconnects its iteration instead
of returning unfiltered rows.
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
### Numeric wiring and recursive construction

Two closed temperature shapes recover native conversions: the annual PDF
mapping inside its user function and the grouped XML mapping through its
descendant node-function rule. Changed arithmetic, shared branches or added
row controls keep an explicit incompatibility.

Strict export can synthesize native user functions for four ordinary XML
constructions with proved schemas and scopes: a path-list hierarchy, a
key/parent adjacency tree, a same-shape recursive file filter, and a recursive
collector with direct files before child directories. Normalization loses the
original function names and canvas layout. Native duplicate/cycle behavior
has not been established.

Nested and repeated XML target scopes can restore a native date-time cast
when every selected leaf and graph consumer satisfies the closed-schema
guards. Invalid lexical input remains a typed conversion failure; EDI
schema/layout admission is a separate check.
Flat XML targets also preserve the evaluation-time conversion of proved direct
String-to-`xs:double` feeds into repeated scalar fields, with either explicit
`cast-in-subtree` metadata or the verified enabled default. The bounded
[float-conversion contract](design/target-float-cast.md) keeps source ownership
and failure order explicit. Its helper remains a Ferrule extension on export;
strict native export refuses it while native value fidelity is unqualified.
Explicit `suppress-cast-in-subtree` uses a separate
[target-owned XML output policy](design/xml-cast-suppression.md). Supported flat
String/double targets and ordinary nested documents preserve scalar text
without altering the declared XSD.
Unresolved shapes and overrides remain blocking, and missing metadata never
enables this mode. Native export retains the suppression token and exact double
declarations; ordinary XML and JSON writers keep their existing behavior.
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
generated Rust/C# mappings and round-trip as native `lang` components.
Zero-input `random` components in the standard `lang` library return a finite,
positive-sign floating-point value in `[0, 1)`. Native and generated execution
use their runtime's pseudorandom generator and preserve ordinary graph
evaluation rules. Supplied arguments fail with an arity error. Random draws
are evaluated separately for each target binding; sharing a graph node does
not promise reuse of one draw across bindings. Random values
are nondeterministic; generated backends preserve the type and range without
promising identical sequences. Same-name components in other libraries retain
their executable-admission refusal.
Scalar
and record-producing filter lookup UDFs accept typed XML, EDI, or database
inputs.
The standard `lang` `format-guid-string` component maps to the pure,
deterministic `format_guid_string` scalar operation. It takes one String of
exactly 32 ASCII hexadecimal characters and preserves case while inserting
hyphens in the `8-4-4-4-12` grouping. For example,
`0123456789abcdef0123456789abcdef` becomes
`01234567-89ab-cdef-0123-456789abcdef`. Ferrule's selected lexical contract
rejects whitespace, braces, already formatted strings, nonhexadecimal text,
and other scalar kinds; it does not enforce UUID version or variant bits.
Arity errors precede type and lexical errors in native and generated calls.
Import/export retain the standard function spelling and `lang` library;
unrelated namespaces do not acquire this function by name alone.
The `translate` component maps to the `translate` scalar operation with XPath
`fn:translate` semantics over Unicode scalar values. Each character of the
value found in the second input becomes the character at the same position in
the third input. Characters without a counterpart are removed, and the first
occurrence of a repeated search character wins. All three inputs use lexical
text, so absent values read as empty strings. Export writes the `core`
library, which offers the function in every execution language.
The exact decimal divide/one-place round UDF pattern fed only by implicit
Protobuf `float` fields exports with native arithmetic wires. Its internal
numeric conversions are identities for the finite float32 values accepted by
the Protobuf file boundary; a string, runtime parameter, changed arithmetic
pattern, or shared conversion keeps the explicit native-profile blocker. This
claim concerns file-backed Protobuf execution. Ferrule projects reimported from
the native design retain conversion errors for hand-built nonfinite instances.
Scalar user-defined function bodies preserve every declared condition/value
pair in an `if-else` component, in physical input-position order. The first true
condition selects its value. With at least one pair, an odd input count uses the
final input as the fallback; an even count falls back to Null.
Fewer than two inputs produce Null without reading an operand. Declared keyless
positions are retained rather than shifting later pairs. Only the selected
value or fallback is evaluated.
These rules apply to callable single-output functions and inlined multi-output
functions. Nested function substitution and intervening scalar calls share the
same expression-depth and node limits; they do not restart the allowance.

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
actionable warning where possible. Producer-ownership and safety failures can
remain hard errors in either profile; repair-oriented import does not waive
those checks.

### Connected targets and saved paths

An intermediate target feeding a later source is not two independent
`Project` targets. Ordinary import warns about the lost connection; executable
single-project import refuses. Use [`import-mfd --pipeline`](mapping-pipelines.md#importing-a-connected-mfd-design)
for a supported serial XML design. The final intermediate can feed one primary
and up to 255 named XML targets. Unsupported branches, cycles and disconnected
boundaries refuse before the pipeline is written.
Static source, target, named-source, and named-target paths are rebased when
the generated project is written somewhere other than the design directory.
HTTP URLs and graph-computed paths are unchanged. Moving or using Save As on a
project applies the same rebasing rule, including wildcard input paths.
Existing directory links are resolved before `..` so Save As preserves resource
identity across linked directories. Document and output directories remain
literal names; source glob suffixes remain patterns. The final resource entry
is not dereferenced, and nonexistent output paths remain valid. Retained editor
history uses the same basis. Offline bundle paths remain purely lexical within
their separately confined virtual root.

### XML output schema-location hint

An XML document target whose `properties/@AddSchemaLocation` is absent or `"1"`
writes a schema-location hint on its output root, as the reference application
does; `"0"` writes none. The reference application writes an absolute
`file:///` URI of the schema on the machine that runs it. Ferrule writes the
schema reference the design stores instead, converted to a URI reference:
Windows extended-length prefixes (`\\?\`) are removed, backslashes become
`/`, characters that cannot appear in a URI reference are percent-encoded (a
space becomes `%20`, as does a `%` that starts no escape), and a Windows drive
or UNC path becomes a `file:` URI.

A relative reference stays relative to the design, as stored; it is not rebased
to the output document. XML readers resolve it against the output document,
so it names the schema only when the output is written beside the design. With
`outputinstance="out/result.xml"` and `schema="target.xsd"`, the hint
`target.xsd` resolves to `out/target.xsd`, which does not exist. Absolute and
`file:` references resolve the same from any output location.

The hint is a target-only `xml_schema_hints` policy:

- An unqualified root gets `xsi:noNamespaceSchemaLocation`.
- A namespaced root gets one `xsi:schemaLocation` pair of the root namespace
  and the same location, matching the reference application's namespaced
  output.
- The stored schema must declare the root's namespace as its own
  `targetNamespace` (or none, for an unqualified root). A root declared in an
  imported schema, or a schema that cannot be read, gets no hint, so no pair
  maps the root namespace to the wrong file.
- DTD-backed targets, documents without a schema reference, WSDL messages and
  intermediate XML string serializers get no hint.
- A target root that declares the same `xsi` attribute keeps its mapped value
  and gets no automatic hint. So does a target whose mapping feeds the root's
  attribute wildcard or the whole root, since either can write `xsi`
  attributes at run time.
- Pass-through components write the hint in their target role only; their
  source role reads without schema hints.

Any other `AddSchemaLocation` value imports with a warning and no hint. The
hint combines with explicit XML scalar output, so cast suppression keeps its
profile under an absent or `"1"` setting. Generated Rust and C# write the hint
wherever their XML adapter writes the document. A generated library without an
XML adapter, such as one fed by CSV or JSON, returns typed output and writes no
XML document, so the host that serializes the document owns the hint.

The imported hint records its provenance in the project file as
`xml_schema_hints.derivation.schema_reference`, the stored schema reference it
was derived from. Hints without a derivation, including those of project files
written before provenance existed, are explicit. A derivation counts only while
it still reproduces the hint exactly for the target root: editing a derived
hint's location, namespace or recorded reference makes it explicit, and so
does a target schema change that alters the derivation, such as a new root
namespace or a declared root `xsi` attribute.

Older Ferrule builds that read `xml_schema_hints` but predate the derivation
refuse such a project file with an unknown-field error for `derivation`.

### XML root-view interoperability

A bounded flat String-attribute profile supports one local XML source and one
local XML target. Both schemas must declare the same selected nondefault type
and exact root identity. Each output field uses the same observed primary-root
type condition, a direct source-root attribute read, and an absent false branch.
The target type is one unconditional declared constant. Attribute requirements
must agree with the source schema, and every graph node must belong to these
projections. Shared source fields can feed multiple target attributes.

Import and export retain the type condition, required reads, physical namespace
identities, and direct base-root selection. The target's default
schema-location setting follows the general hint policy above. Other
root-expression graphs, additional document boundaries, failure rules, user
functions, and other explicit schema hints remain outside this `.mfd` profile. Unsupported export fails before
publishing artifacts; unproved import shapes retain actionable diagnostics.

## Export

```sh
cargo run -p cli -- export-mfd --project project.json --out design.mfd
cargo run -p cli -- export-mfd --project project.json --out design.mfd --profile native-mfd --check --report-json
cargo run -p cli -- export-mfd --project project.json --out design.mfd --profile native-mfd
cargo run -p cli -- export-mfd --project flow.json --pipeline --out chained.mfd --profile native-mfd
```

Ordinary and connected `.mfd` designs share a 64 MiB UTF-8 byte limit. Import
checks file size before reading and bounds the actual read; export checks the
complete XML, including escaping and embedded metadata, before publishing any
design or schema sibling. Each connected stage and the combined design must
fit the same limit.
Import also refuses a design whose `<entry>` elements nest more than 128
levels deep (`MAX_MFD_ENTRY_DEPTH`), anywhere in the design, before it reads
any component. Real designs nest about 20 levels. The limit keeps the
recursive entry-tree importers, and engine validation of the imported scopes,
within a default 8 MiB stack. It applies to file, package, pipeline and
filename-document import, and so to the GUI and to the re-import that package
export performs.
MFD entries and generated XSD attributes preserve tabs and line breaks through
XML character references, including field names, paths, fixed values, and
defaults.

Physical XML document targets explicitly set `AddSchemaLocation="0"` so the
reference application's automatic schema URI does not change Ferrule's normal
no-hint output. This applies to primary and named targets. A target whose hint
is still derived from the default setting exports with the setting absent, the
reference application's saved form. Export writes the target schema as its own
generated sibling (`<design>-target.xsd`), so re-importing the exported design
derives a hint naming that sibling, not the originally imported schema
reference. The location changes; the setting and hint form do not. A derived
hint that no longer matches its target schema is refused with an error saying
so; source hints and explicit XML schema hints remain outside MFD export. The
finite
[schema-reference observation](conformance/xml-schema-reference.md) covers real
controls, full outputs and a native save/reopen of an actual Ferrule export.

The native editor exposes both profiles under File: “Export MFD (Ferrule)...”
and “Export native MFD...”. Both suggest an `.mfd` filename in the project's
folder. Native rejection lists each affected component and retains its
compatibility feature and emitted identity; existing design/schema files remain
unchanged. A successful native-profile export confirms the local compatibility
checks, not general acceptance by the reference application. Separately recorded
native observations apply only to their stated fixtures and profiles.

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

### Connected pipeline export

For the CLI and editor operations, supported final formats and named-source
bindings, see [Mapping pipelines](mapping-pipelines.md). A serial chain has
2–65 stages. Repeated connections from one output port share one vertex. Each
repeated target branch must have exactly one uncloned publication port; zero
or several refuse before any design or schema sibling is published. A
pass-through component preserves both its output instance and the next source
preview path. An absent output instance is never inferred.

A uniquely bound direct XML date-time conversion can export as `cast-in-subtree`
with `xs:dateTime` restored on the corresponding XSD leaves. A shared graph
consumer, a target default or an unsupported subtree refuses before
publication. See [XML target casting](design/target-float-cast.md) for the
accepted bindings and lexical differences.

### JSON string parsers and structural variables
When an imported JSON boundary or JSON string parser fell back to its entry
tree because its schema was unavailable, best-effort export records that
unresolved reference beside the generated schema and re-import reports it
again. Parser references survive project serialization in versioned recipe
metadata that leaves parsing behavior unchanged. Native-profile preflight
marks the export incomplete and rejects publication; the generated schema is
not evidence that the original external schema was recovered.
JSON string parsers lower connected scalar property outputs. Unconnected object,
array and item views do not participate in parser admission. Connected structural
outputs outside the bounded profiles below, root arrays and missing schema paths retain
their explicit refusal. Each admitted field still parses and validates the complete
JSON document against its schema, including properties whose ports are unused.

One array of ordinary records can feed a repeating target group from a uniquely
declared required global String input and a resolved JSON schema. Direct scalar
connections below the array select fields of the same item, including scalar
fields inside ordinary nested objects. A direct array-to-group connection is
optional; the scalar connections can establish the record driver. Ordered,
one-based item indices preserve every record, including records with absent
scalar fields. Empty or absent non-nullable arrays produce no records. Explicit
JSON scalar null values retain their schema-defined null behavior, distinct from missing
fields. Malformed JSON, invalid item types and incompatible structures remain
typed parsing failures, not null or zero fallbacks.
Absent optional object parents preserve scalar absence or an empty selected
array only after the descriptor resolves to a declared scalar or array and the
complete input passes schema validation. Invalid descriptor paths, missing
required properties, incompatible values and disallowed nulls remain failures.

This adapter validates the complete document before iterating and reuses that
validated representation across main-graph field projections within the same
invocation and source owner. Exact input/schema changes require validation
again; independent source expressions never share slots. It materializes the
input and is not a streaming or large-array
performance contract. Nested arrays, scalar arrays, computed or optional parser
inputs, indirect scalar consumers, mixed record feeds and enclosing target
iterations, cloned target occurrences, target node controls and nullable containers
remain outside this profile and produce
executable-import diagnostics. Schema array constraints and the existing
generated-sequence item limit still apply.
One unchanged main-graph source owner needs one full parse, independent of
record and connected-field counts. Complete-string comparisons, descriptors,
selection and output construction still cost work. Stateless calls and
user-function body graphs keep per-call parsing. Existing input-size and
item-count ceilings do not establish elapsed-time or memory efficiency; see
[JSON projection work](design/json-projection-work.md).

A separate bounded scalar lookup accepts a parsed record-array value wired to
the first input of a core filter, with `equal` comparing a String key from the
same array to a String field of the current primary-document item. The first
filter output must connect directly to scalar targets in that item's actual
iteration frame. The parser requires a uniquely declared required global String
input and a resolved schema. The equality and connected parser fields must belong
exclusively to that filter; unconnected declared ports do not change admission.
An empty, attribute-free second output placeholder retains its unused false
slot; it never receives an invented output identity. Malformed or shared output
declarations remain refused.
The primary document owns output repetition and order, including repeated keys.
The lookup scans the parsed array in its original order and selects the first
matching record, even when that record's value is absent or JSON null. Missing
values remain absent, permitted JSON null remains distinct, and missing/null keys
do not match. Identifiers are not trimmed, case-folded or converted to numbers.

This lookup uses a private generated index and the existing ItemAt reducer, not
a physical parsed-array source. Shared predicates, mixed parser ownership,
additional connected array fields, secondary or scalar-repeating key dependencies,
inverted filter outputs, indirect consumers, and sibling or nested receiving
frames outside the primary key's actual frame retain executable-import refusals.
An ordinary primary-document ancestor chain may contain nested repeating groups,
including EDI interchange, group, message and detail owners. Every repeated
prefix must be declared and framed, and the actual receiving anchor must equal
the key's innermost repeated owner. Ancestor keys are not silently broadcast to
a different current-item frame, nor are deeper unframed repetitions read as one
scalar.
The primary key can also be a bounded pure expression: current-item String
fields, exact core String constants, core String equality predicates, and core
`if-else` with two connected String branches. All physical leaves, including
untaken branches, must share that same actual innermost primary owner. At least
one physical leaf is required. Raw pins, saved feed identities and unique output
owners are proved before lowering, with a 16-level depth and 64-visited-node work
bound and cycle refusal. The complete lowered graph must then match the proved
fields, constants, operator types and branch order; fallback or cached nodes
outside that proof are refused. Missing selected values stay absent and allowed
JSON null remains null, without choosing another branch or inventing a default.
Other functions, parameter reads, coercions, stateful or recursive expressions,
aggregates, filters, parser reads and foreign-frame leaves remain outside this
key-expression profile.
The complete parser input is validated when first demanded within an invocation,
including later duplicates, nonmatching records and unused properties.
Main-graph projections reuse it only for that source owner and exact input/schema
contents. An empty primary iteration does not itself demand the parser; an
independent connected root-field demand still validates it. Lookup scans and
content comparisons retain their costs even though repeated field demands no
longer reparse an unchanged document. This is not a large-array performance
contract; see [JSON projection work](design/json-projection-work.md).

A JSON string parser with no declared output ports can retain a directly connected,
uniquely declared required global String parameter without evaluating that parser.
It requires one valid input declaration and one `stringparse` parameter, with no
connected default or scope controls. The imported graph preserves the runtime
parameter's name, type and preview for host inspection; it creates no physical
source or parser call. Required arguments enforced by an external wrapper remain
that wrapper's contract. Schema resolution still runs, and missing or malformed
schemas still prevent executable import. Declared but unused outputs do not use
this admission path.

Required global String inputs accept the same exact absent-default forms as
optional parser inputs: no `sources` element, an empty element, or one
position-zero datapoint with no key or an unconnected valid key. The raw form is
checked before pin normalization. Duplicate, malformed and connected defaults
remain unsupported; an empty placeholder never supplies the design-time preview
as a runtime default.

JSON structure variables use the explicit parameter-root path to select their
schema value. Property steps retain their names; object views are transparent.
Variables remain intermediate values and scalar outputs resolve through their
declared input connectors. A copy-all connection can construct a closed,
non-repeating JSON record of scalar fields from constants or singleton XML/JSON
source fields when the complete target schema matches. At least one non-null
constant or required non-null JSON scalar input establishes record presence.
Connected structural inputs, competing field connectors, repeating dependencies, overlapping target
inputs, unresolved selections and variable scope controls retain refusals.

JSON variable selectors can cross a declared object array through an explicit
`array` followed by its canonical `item` / `json-item` role, then select a
non-repeating descendant. The selected schema remains complete; graph inputs
establish its value and iteration context. Selecting an item as a variable root
does not remove repetition. Nested-array wrappers, malformed item roles and
alternative views retain refusals.

A terminal `array` view can select a complete array of records for transparent
forwarding. One explicit structural input supplies the collection; descendant
outputs retain its item frame, ordering and scalar paths. Array and item facets
remain intact. A separate bounded singleton proof admits literal-established
scalar assignments when the selected and target item schemas match completely.
Other descendant overrides, competing structural drivers and variable scope
controls retain refusals.

A single direct core constant can drive a repeating group once in the existing parent
context. Its declared literal must be a string, Boolean, integer or finite
floating-point value. Empty strings, false and zero each supply one item; they
are not filter predicates. The constructed scope retains the target schema and
parent frame, including empty groups and nested source-row bindings. Malformed
constants, competing structural connectors, computed feeds, structural copy and sequence controls keep their
existing admission diagnostics.

Separately declared cloned groups can each have a direct constant driver. Each
occurrence must own exactly one connected group connector and its descendant
bindings. They contribute items in saved declaration order, even when they reuse
the same constant output, and retain the enclosing source-row context. Competing
connectors within one occurrence and inputs shared ambiguously across cloned
declarations remain refused.

A declared clone of a repeating XML target group can instead be driven by a core
kind-4 `if-else` over a plain JSON source. The clone is present when the lazily
selected scalar exists: false, zero and the empty string are present, while Null
is absent. A keyless `if-else` branch, saved as a `<datapoint/>` without a key at
raw position 1 or 2, selects Null. Each conditional controls exactly one clone,
which lowers to an optional-presence scope over `exists(selected)` in the
unchanged enclosing frame; unconnected declarations add no occurrence, and
clones keep saved declaration order. The conditional and every scalar binding of
its clone are proved separately from the strict scalar-assignment proof: each
operand is a valid scalar literal, a physical scalar field in the plain enclosing
document or collection frame, a nested conditional, a core kind-5 `equal` of two
compatible scalars, or a lang kind-5 `empty` of a String. Conditions are Boolean,
branch and binding types match, and a walk permits at most 16 nested operators
and 256 visited operands. Operators must have exact raw pins, optional empty
`properties` and `view` metadata, and one raw declared producer for their output
and for each connected input. A missing Boolean condition keeps the typed
runtime error, and an unselected branch is never evaluated. Group-valued
branches, cycles, malformed pins, priority contexts, competing producers,
uniterated or foreign frames, controlled parents, clones without a conditional
driver, user functions, aggregates, clocks and other computed operands refuse
with one diagnostic before any import state changes; the ordinary repair draft
is retained and the executable profile rejects the design. Physical EDI scalar
inputs, JSON targets and uncloned target groups keep their existing behavior.
Optional-presence scopes have no qualified `.mfd` export and the GUI shows them
read-only.

A closed, non-repeating JSON or compiled EDI variable record can also drive one
repeating target group through its structural output. Require one connected
target connector, a valid direct literal that establishes presence, and
unambiguous scalar assignments. Other inputs must resolve to primary JSON/XML
fields through direct ports or at most 64 exact scalar aliases in uncontrolled
JSON/EDI variables. Each alias has one connected scalar input at its output path
and preserves the complete selected scalar schema apart from its local name.
Cycles, connected ancestor drivers, overrides and changed scalar facets retain
their refusals. The physical fields' repeated ancestors belong to plain source scopes enclosing the
target. A sibling's frame is insufficient. Preserve complete physical schemas
and the inherited context; this rule constructs target bindings and introduces
no collection frame. Copy-All can project at most 256 scalar fields from the
same bounded record proof, including a declared record collection that permits
one item. Compare complete item schemas apart from local names and separately
proved cardinality/uniqueness metadata; retain the original selected and target
schemas. Explicit target scalar bindings take precedence over projected fields.
Scalar assignments may also use exact core kind-4 `if-else` components with
three unambiguous connected inputs and one declared output. Prove every operand
as a valid scalar literal, a qualified parent field or scalar alias, or another
such conditional. Each assignment permits at most 32 nested conditionals and
256 visited operands, including repeated references; branch evaluation remains
lazy. A conditional result does not establish record presence. Cycles, malformed
raw pin metadata, structural branches and unsupported computed operands retain
refusals even when their branch would be unselected.
Collection `contains` constraints, incompatible facets and singleton-excluding
cardinality retain refusals. Other computed inputs, competing connectors,
uniterated collection crossings, controlled parent scopes and variable controls
retain their refusals.

EDI structure variables resolve their flat declared parameter-root path against
a complete compiled EDI configuration. The selected value preserves scalar types
and cardinalities and remains an intermediate component. Scalar outputs resolve
through their input connectors without creating a physical EDI endpoint or
applying interchange-output settings. Missing configuration, unresolved roots,
ambiguous selectors and scope controls retain explicit refusals.
Parser-generated X12 acknowledgement branches belong to physical endpoints.
An X12 variable cannot introduce parser-error ports absent from its selected
configuration; configured fields whose names begin with `ParserErrors_` remain
ordinary data.

### Joins and iterations

Import also admits a computed join key: in a two-input join beside one
repeating source collection, either input may be fed by a constant, a runtime
input parameter, or a call chain of `uppercase`, `lowercase`, `concat` and
`add` over those and over non-repeating primary-source fields. The key is
evaluated in the receiving context of the iteration the join drives, and it is
`Null`, giving no tuples, whenever a field it reads is absent or nil, as the
reference application yields nothing there. A `tokenize` of such a string by a
non-empty string constant may feed the join input directly; each token then
contributes its own tuples in order, with empty and duplicate tokens kept, and
an absent or nil input gives no tokens. Calls that could raise stay refused:
`uppercase` or `lowercase` over a non-string, `add` over a string, boolean or
integers alone, and `concat` over a decimal. Two computed inputs, computed
inputs in larger joins, keys read from the tuple, keys feeding a joined
aggregate, keys over named or dynamic sources, keys that read across an
inactive repetition, other sequences, sequences inside a key's computation,
other functions, and a function or constant over a copy-all connection keep the
join and its driven scopes out with a warning naming the reason. See [computed
join keys](design/join-computed-keys.md).

Export writes core kind-32 joins whose inputs all come from source ports, each
fed by a copy-all connection; the reference application refuses an ordinary
connection on any such input. Root-context joins take their collections from
the primary source. A join nested inside one ordinary iteration over the primary
source or a static named source is evaluated again for every iterated item. It
takes singletons from the active item or from a static named source, at any
input position and in any number, and needs no singleton. Its repeating inputs
come from static named sources, each from its own component, and, inside a
primary iteration, from at most one collection below the active item, such as
`Line` inside an `Order` loop, which yields only that item's rows. A two-input
plan may instead pair one singleton with one primary repeating collection
outside the iteration. Export refuses, atomically under the native profile,
nested plans of singletons only, more than one repeating input below the
iteration, repeating inputs below a named-source iteration, primary repeating
inputs outside the iteration in any other plan, repeating paths that more than
one enclosing source frame declares, an iteration path that names both a
primary child and a named source, named inputs that an enclosing frame could
shadow, and named paths that would change when reimport strips the enclosing
iteration path.

Export writes a computed join key in exactly the form import admits, so the
design re-imports to the same plan. The key's constant, runtime parameter or
`uppercase`, `lowercase`, `concat` and `add` call chain is written as ordinary
components whose output feeds the join input; a `tokenize` key is written as
its `tokenize` component with its input and constant delimiter. A function or
constant feeds an input named `value` through an ordinary connection; a runtime
parameter feeds an input named after the parameter through a copy-all
connection, as the reference application requires. The null guard import adds
around a key that reads source fields is not written: the native functions
already yield no key for an absent or nil field, and import adds the same guard
again. The other input is a primary repeating collection: at the root, or, for
a join nested in a source iteration, outside that iteration. Export refuses,
before writing any file, every other computed key: keys import would refuse or
read back differently (a bare source field, a guard that does not cover exactly
the fields the key reads, a guard other expressions share, a sequence other
than `tokenize`, or a delimiter that is not a non-empty string constant), keys
beside a singleton, a named collection or a collection below the iteration,
keys feeding a joined aggregate, and keys in additional targets, concatenated
segments or dynamic child scopes.

<a id="json5-in-saved-designs"></a>

JSON5 component settings and `.json5` instance paths are retained as a typed
repair dependency, not executable native JSON5 mode. Legacy filename attributes
take precedence over nested input/output file roles. Strict import, runtime and
export refuse marked primary and named boundaries. A saved `json5` attribute
keeps its exact text in `FormatOptions.json5_interchange_dependency`, separately
from Ferrule's executable `json5` flag. String parser and serializer dependencies
are recorded in `mfd_json5_string_dependencies` on the primary source. Filename
selection and the handling of hexadecimal and nonfinite numbers differ across
codecs; treating the setting as ordinary JSON would change the design.

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

The fixed-per-run current date/time value exports as the native `lang/now`
component, which the built-in execution language provides. Both that component
and `xpath2/current-dateTime` import to the same typed runtime value and round
trip under a fixed execution clock. In native runs every `lang/now` read in one
execution returned the same value, whether it came from separate components, a
reused component or successive target rows, and even with `lang/sleep` delays
between the reads. Several date/time uses therefore export without refusal.
Target-type date/time coercion remains a separate extension dependency.

### Scalar functions in native designs

Exported designs select the built-in execution language, so each function call
exports only as a function that language provides with the same inputs and
meaning; otherwise export fails before writing anything and names the function:

- `round_half_ceiling` exports as `core/round` with one input and as
  `core/round-precision` with two. The native `round` has one input: a design
  that connects a second one still runs, but the precision is ignored, and
  saving drops that input. Ferrule's `round` is refused; see below.
- `format_date` and `format_datetime` export with at most three inputs (value,
  format, language) and `format_time` with at most two. The native functions have
  no calendar or place inputs, and `format-time` has no language input; extra
  connections are ignored when the mapping runs and dropped when it is saved, so
  export refuses those calls. An empty language selects English natively, as it
  does in Ferrule. A named language such as `de` gives localized month and day
  names natively, while Ferrule refuses any language other than the default.
- The unary-plus `positive`, `trim`, `ends_with`, `matches` and `replace` are
  refused. The reference application reports `core/trim`, `core/ends-with`,
  `core/matches`, `core/replace` and `core/positive` as functions it cannot find
  for the built-in language. The built-in `lang/left-trim`, `lang/right-trim`
  and `core/normalize-space` keep non-breaking and other non-ASCII spaces that
  `trim` removes. The built-in `lang/replace` substitutes literal text, so the
  regular-expression `replace` has no counterpart. `lang/match-pattern` succeeds
  only when the pattern matches the whole text, whereas `matches` succeeds on a
  match anywhere.

Native `lang/positive` returns true only for numbers greater than zero; `0` and
`-0` give false, unlike its vendor description ("equal to or greater than
zero"). Import reads it as `greater_than(add(value, 0), 0)`: adding
zero converts numeric text the way its decimal input does, and both functions
export natively. Non-numeric text fails in both. The `core/positive` spelling
written by earlier Ferrule exports still imports as unary plus.

Native `lang/replace` replaces every occurrence of its search text, left to
right and without overlap, with its replacement text. Neither input has pattern
or escape syntax: `.`, `*`, `(`, `$` and `\` in the search text match
themselves, and `$0` or `\` in the replacement is inserted as written. Matching
is case-sensitive, whether the inputs are constants or connected. An empty search
text, connected or constant, fails execution ("The search string passed to the
replace function is empty."). Import reads it as `replace_literal`, which keeps
these semantics and fails on an empty search text, and export writes it back as
`lang/replace`. The `core/replace` and `xpath2/replace` spellings still import as
regular-expression `replace`.

Native `core/round` and `core/round-precision` round exact halves toward
positive infinity (`-2.5` becomes `-2`) and work on decimal values, so `1.005`
rounds to two places as `1.01`. Ferrule's `round` rounds halves away from zero
on binary floating-point values, so negative halves, and decimal fractions that
binary floating point cannot represent exactly, can round differently.

Import therefore maps `round` and `round-precision`, in any library, to the
separate `round_half_ceiling` function, which follows the native rule. Ferrule's
own `round` keeps its behavior, so native Ferrule projects are unchanged. Export
writes `round_half_ceiling` back as `core/round` or `core/round-precision`. It
refuses Ferrule's `round`, because it can't prove that the inputs avoid the
cases where the two differ.

`round_half_ceiling` rounds decimal digits, never a scaled binary float:

- Text is rounded on its own digits. A floating-point value is rounded on its
  shortest round-trip digits, so `1.005` keeps its digits after a numeric
  conversion. Decimals with at most 15 significant digits survive that
  conversion exactly.
- Integers stay integers, including with negative precision, and fail with an
  integer overflow error beyond the 64-bit range. Other results are the nearest
  floating-point value to the rounded decimal; a zero result has no negative
  sign, and a result beyond the floating-point range becomes infinity.
- NaN and infinities pass through, as they do for `round`.
- Precision is read like `round`'s: integers directly, floating-point values
  rounded half away from zero with NaN as zero, and numeric text as `to_number`
  reads it. Precision beyond the 32-bit range saturates.
- Non-numeric text fails as an invalid argument; booleans, absent values and
  `Decimal96` values fail with a type error.

The interpreter, generated Rust and generated C# give the same results. Native
runs established the half rule and decimal text inputs; the integer, precision
and non-finite rules above follow Ferrule's numeric conventions and haven't been
checked against native runs. Arithmetic that feeds a rounding still uses binary
floating point, so a computed input can differ from the native decimal result
before it is rounded.

### Export profiles and diagnostics

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

A recovered entry tree is enough to reopen and repair a design. It does not
replace a missing schema or prove that a reader will accept the original
input. Unsupported schemas, layout settings and connections retain their
diagnostics; executable admission refuses them.

| Boundary | What survives interchange |
| --- | --- |
| Referenced JSON Schema | Admitted [object, array, numeric and string constraints](formats.md#json-schema) remain executable on input and normalized output; export writes a canonical JSON Schema sibling. An unsupported dialect/shape, nonrepresentable union or unresolved schema triggers the component diagnostic rather than widening its domain. |
| Positional JSON | Homogeneous tuples can normalize to one repeated field. MFD entries cannot address heterogeneous tuple-frame positions; import keeps the warning and entry tree, and a JSON string parser cannot embed a frame. See [tuple frames](design/json-tuple-arrays.md). |
| XML declarations | Elements and attributes retain expanded names; foreign declarations export as an atomic graph of local XSD siblings. Compatible strict element-wildcard declarations can share a port across namespaces; resolved wildcard attributes with the same local name are refused. |
| Other components | Only the admitted SQL/SQLite, workbook, FlexText, PDF and XBRL layouts execute. General database mutation, taxonomy-level XBRL, arbitrary service components and general sequence composition are outside the profile. See [format boundaries](formats.md#important-boundaries). |

### Open JSON target entries
A JSON target entry may name a property that its open object admits without
declaring it, such as `code` beside a declared `Code`. Import keeps the exact
entry name and records an explicit undeclared member
(`json_undeclared_property`) whose schema copies the object's dynamic value
schema, so it is typed only when `additionalProperties` (or a matching
`patternProperties` selector) constrains the value. Presence stays owned by
the object's `required` list. Names are never case-folded and the referenced
schema is not rewritten: JSON Schema export omits the member from
`properties`, and MFD export/re-import restores it from the target entry. A
closed object still reports the missing target path, and a name rejected by
the object's selectors or `propertyNames` is reported and skipped. When a
required member's mapped value is absent, native and generated Rust/C# output
refuse the object for the missing required property, exactly as for a declared
property of the same name, rather than emitting the object without it.
### XML wildcard selections

The [wildcard contract](design/xml-wildcard-exclusions.md) defines namespace
constraints, `skip`, `lax`, `strict`, and the placements of XSD 1.1 exclusions.
A `<selections>` QName rejected by its wildcard's namespace or exclusion list
produces a warning and no typed port. Only XML document components export
exclusions into XSD siblings as explicit names. Exclusions on non-XML or WSDL
boundaries, serializer schemas, request contracts or invalid wildcard nodes
refuse before publication.
## Global failure priority and strict native export

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

## Read-only parity surveys

The import/export, execution, and export/re-import execution surveys accept the
same host-selected package manifest and ordered catalog roots as `import-mfd`.
Set `FERRULE_MFD_SURVEY_PACKAGE_MANIFEST` to the manifest file. Set
`FERRULE_MFD_SURVEY_EDI_CATALOG_ROOTS` and
`FERRULE_MFD_SURVEY_JSON_SCHEMA_CATALOG_ROOTS` to platform path lists; roots
retain their declared order after canonical deduplication. When no manifest is
selected, the `ReferenceSamples` directory remains the package root.

The surveys, and every other test that reads the gitignored
`samples/ReferenceSamples` corpus, are ignored by ordinary test runs. An
explicit run fails with a "private compatibility corpus not present at <path>"
error when the corpus is missing; it never reports a pass without it. The
reference-output survey likewise fails when the local reference application is
missing.

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

## Independence

ferrule is an independent project and is not affiliated with or endorsed by
the developer of the reference application.
